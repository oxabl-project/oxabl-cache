//! An HTTP proxy between the client and the server that can lie: it corrupts, truncates,
//! swaps, stalls and withholds, and records what it was sent.

use std::convert::Infallible;
use std::future::Future;
use std::net::{SocketAddr, TcpListener};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::{Body, Frame, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use oxabl_cache_format::{LookupRequest, LookupResponse};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::Sleep;

/// A request the proxy saw.
#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub authorization: Option<String>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub enum LookupRule {
    #[default]
    Pass,
    Garbage,
    SwapRecords,
    SwapLocations,
    /// Asks another cache the same question, with a token that may read it.
    Redirect {
        from: String,
        to: String,
        token: String,
    },
}

#[derive(Clone, Debug, Default)]
pub enum ObjectRule {
    #[default]
    Pass,
    /// Flips one byte at this offset of every ranged read.
    Flip(usize),
    /// Sends only this many bytes, with a consistent length.
    Truncate(usize),
    /// Sends `chunk` bytes, then waits `every`, on requests whose path holds `only`.
    Pace {
        chunk: usize,
        every: Duration,
        only: Option<String>,
    },
    /// Sends `after` bytes, then nothing, on requests whose path holds `only`.
    Stall { after: usize, only: Option<String> },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CommitRule {
    #[default]
    Pass,
    /// Forwards the first `n` commits and closes the connection instead of answering.
    DropReplies(u32),
    /// Drops the reply to the first commit, then answers every repeat with `409 pack_closed`.
    DropThenClose,
}

#[derive(Default)]
struct Rules {
    commit: CommitRule,
    commits: u32,
    fail_puts: u32,
    first_object_delay: Option<Duration>,
    lookup: LookupRule,
    objects: ObjectRule,
    roster: Option<Vec<u8>>,
    dictionary_delay: Option<Duration>,
    unauthorized_once: bool,
    oidc_session: Option<String>,
}

struct State {
    upstream: String,
    http: reqwest::Client,
    rules: Mutex<Rules>,
    seen: Mutex<Vec<Seen>>,
    connections: Mutex<Vec<tokio::task::AbortHandle>>,
    streaming: Arc<AtomicUsize>,
}

pub struct Proxy {
    pub address: SocketAddr,
    state: Arc<State>,
    task: Option<JoinHandle<()>>,
}

/// A response body that can be paced or stalled.
struct Paced {
    data: Bytes,
    position: usize,
    chunk: usize,
    every: Duration,
    stall_after: Option<usize>,
    sleep: Option<Pin<Box<Sleep>>>,
    live: Option<Live>,
}

/// Counts a response body that is still being sent.
struct Live(Arc<AtomicUsize>);

impl Live {
    fn new(counter: &Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter.clone())
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Paced {
    fn whole(data: Bytes) -> Self {
        Self {
            chunk: data.len().max(1),
            data,
            position: 0,
            every: Duration::ZERO,
            stall_after: None,
            sleep: None,
            live: None,
        }
    }
}

impl Body for Paced {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        let this = &mut *self;
        if let Some(sleep) = &mut this.sleep {
            if sleep.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            this.sleep = None;
        }
        if this.stall_after == Some(this.position) {
            return Poll::Pending;
        }
        if this.position >= this.data.len() {
            return Poll::Ready(None);
        }
        let limit = this.stall_after.unwrap_or(usize::MAX).min(this.data.len());
        let end = (this.position + this.chunk).min(limit);
        let frame = this.data.slice(this.position..end);
        this.position = end;
        if !this.every.is_zero() {
            this.sleep = Some(Box::pin(tokio::time::sleep(this.every)));
        }
        Poll::Ready(Some(Ok(Frame::data(frame))))
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        hyper::body::SizeHint::with_exact(self.data.len() as u64)
    }
}

type Reply = Response<Paced>;

enum Action {
    Pass,
    DropReply,
    Conflict,
    FailPut,
}

fn reply(status: u16, content_type: &str, body: Vec<u8>) -> Reply {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(Paced::whole(Bytes::from(body)))
        .expect("response")
}

fn envelope(status: u16, code: &str, message: &str) -> Reply {
    let body = serde_json::json!({"error": {"code": code, "reason": "pack_closed", "message": message, "retryable": false}});
    reply(status, "application/json", body.to_string().into_bytes())
}

fn tamper_lookup(request: &[u8], response: &[u8], rule: &LookupRule) -> Vec<u8> {
    let Ok(request) = LookupRequest::decode(request) else {
        return response.to_vec();
    };
    let Ok(shape) = request.shape() else {
        return response.to_vec();
    };
    let Ok(mut decoded) = LookupResponse::decode(response, &shape) else {
        return response.to_vec();
    };
    let located: Vec<usize> = (0..decoded.hits.len())
        .filter(|&i| decoded.hits[i].record.is_some())
        .collect();
    if located.len() >= 2 {
        let (a, b) = (located[0], located[1]);
        let (first, second) = decoded.hits.split_at_mut(b);
        let (x, y) = (
            first[a].record.as_mut().expect("record"),
            second[0].record.as_mut().expect("record"),
        );
        match rule {
            LookupRule::SwapRecords => std::mem::swap(&mut x.record, &mut y.record),
            LookupRule::SwapLocations => {
                std::mem::swap(&mut x.pack_idx, &mut y.pack_idx);
                std::mem::swap(&mut x.offset, &mut y.offset);
                std::mem::swap(&mut x.length, &mut y.length);
            }
            _ => {}
        }
    }
    decoded.encode(&shape).unwrap_or_else(|_| response.to_vec())
}

async fn forward(
    state: &State,
    parts: &hyper::http::request::Parts,
    body: Vec<u8>,
    path: &str,
    token: Option<&str>,
) -> Option<(u16, Vec<(String, String)>, Vec<u8>)> {
    let query = parts
        .uri
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let mut request = state
        .http
        .request(
            parts.method.clone(),
            format!("http://{}{path}{query}", state.upstream),
        )
        .body(body);
    for (name, value) in &parts.headers {
        let replaced = token.is_some() && name == "authorization";
        if !replaced
            && !matches!(
                name.as_str(),
                "host" | "connection" | "content-length" | "transfer-encoding"
            )
        {
            request = request.header(name, value);
        }
    }
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.ok()?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter(|(n, _)| {
            !matches!(
                n.as_str(),
                "connection" | "content-length" | "transfer-encoding"
            )
        })
        .filter_map(|(n, v)| Some((n.to_string(), v.to_str().ok()?.to_owned())))
        .collect();
    Some((status, headers, response.bytes().await.ok()?.to_vec()))
}

async fn handle(state: Arc<State>, request: Request<Incoming>) -> Result<Reply, std::io::Error> {
    let (parts, incoming) = request.into_parts();
    let body = incoming
        .collect()
        .await
        .map(|b| b.to_bytes().to_vec())
        .unwrap_or_default();
    let path = parts.uri.path().to_owned();
    state.seen.lock().expect("seen").push(Seen {
        method: parts.method.to_string(),
        path: format!(
            "{path}{}",
            parts
                .uri
                .query()
                .map(|q| format!("?{q}"))
                .unwrap_or_default()
        ),
        authorization: parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        body: body.clone(),
    });
    let mut action = Action::Pass;
    let mut object_delay = None;
    let (lookup, objects, roster, delay, oidc, unauthorized) = {
        let mut rules = state.rules.lock().expect("rules");
        if path.ends_with("/commit") {
            rules.commits += 1;
            action = match (rules.commit, rules.commits) {
                (CommitRule::DropReplies(n), calls) if calls <= n => Action::DropReply,
                (CommitRule::DropThenClose, 1) => Action::DropReply,
                (CommitRule::DropThenClose, _) => Action::Conflict,
                _ => Action::Pass,
            };
        }
        if path.contains("/objects/") && parts.method == hyper::Method::PUT && rules.fail_puts > 0 {
            rules.fail_puts -= 1;
            action = Action::FailPut;
        }
        if path.contains("/objects/") && parts.method == hyper::Method::GET {
            object_delay = rules.first_object_delay.take();
        }
        let unauthorized = rules.unauthorized_once && path.starts_with("/v1/caches/");
        if unauthorized {
            rules.unauthorized_once = false;
        }
        (
            rules.lookup.clone(),
            rules.objects.clone(),
            rules.roster.clone(),
            rules.dictionary_delay,
            rules.oidc_session.clone(),
            unauthorized,
        )
    };
    if path == "/v1/auth/oidc"
        && let Some(session) = oidc
    {
        let body = serde_json::json!({"token": session, "expires_at": 4_102_444_800u64});
        return Ok(reply(
            200,
            "application/json",
            body.to_string().into_bytes(),
        ));
    }
    if unauthorized {
        return Ok(envelope(
            401,
            "unauthenticated",
            "audience should be someone-else",
        ));
    }
    match action {
        Action::Conflict => return Ok(envelope(409, "conflict", "the pack is closed")),
        Action::FailPut => return Ok(envelope(500, "internal", "the store failed")),
        Action::Pass | Action::DropReply => {}
    }
    if let Some(delay) = object_delay {
        tokio::time::sleep(delay).await;
    }
    if path.ends_with("/dictionary")
        && let Some(delay) = delay
    {
        tokio::time::sleep(delay).await;
    }
    let mut upstream_path = path.clone();
    let mut token = None;
    if let LookupRule::Redirect {
        from,
        to,
        token: other,
    } = &lookup
        && path.ends_with("/lookup")
    {
        upstream_path = path.replace(from.as_str(), to);
        token = Some(other.as_str());
    }
    let Some((status, headers, mut payload)) =
        forward(&state, &parts, body.clone(), &upstream_path, token).await
    else {
        return Ok(envelope(502, "unavailable", "upstream"));
    };
    if matches!(action, Action::DropReply) {
        return Err(std::io::Error::other("the reply is lost"));
    }
    let ok = (200..300).contains(&status);
    let is_lookup = path.ends_with("/lookup") && ok;
    let is_object = path.contains("/objects/") && ok;
    if is_lookup {
        match &lookup {
            LookupRule::Garbage => {
                payload = (0..payload.len().max(64))
                    .map(|i| (i * 31 % 251) as u8)
                    .collect()
            }
            LookupRule::SwapRecords | LookupRule::SwapLocations => {
                payload = tamper_lookup(&body, &payload, &lookup);
            }
            _ => {}
        }
    }
    if path.ends_with("/roster")
        && parts.method == hyper::Method::GET
        && ok
        && let Some(held) = roster
    {
        payload = held;
    }
    let mut paced = Paced::whole(Bytes::new());
    if is_object {
        match &objects {
            ObjectRule::Flip(at) if !payload.is_empty() => {
                let at = (*at).min(payload.len() - 1);
                payload[at] ^= 0x01;
            }
            ObjectRule::Truncate(keep) => payload.truncate(*keep),
            _ => {}
        }
    }
    let total = payload.len();
    let data = Bytes::from(payload);
    paced.data = data;
    paced.chunk = total.max(1);
    if is_object {
        match &objects {
            ObjectRule::Pace { chunk, every, only }
                if only.as_ref().is_none_or(|p| path.contains(p.as_str())) =>
            {
                paced.live = Some(Live::new(&state.streaming));
                paced.chunk = *chunk;
                paced.every = *every;
            }
            ObjectRule::Stall { after, only }
                if only.as_ref().is_none_or(|p| path.contains(p.as_str())) =>
            {
                paced.stall_after = Some(*after);
            }
            _ => {}
        }
    }
    let mut builder =
        Response::builder().status(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY));
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    Ok(builder.body(paced).expect("response"))
}

impl Proxy {
    /// Serves on `listener` and forwards to `upstream`.
    pub fn start(listener: TcpListener, upstream: &str) -> Self {
        listener.set_nonblocking(true).expect("nonblocking");
        let address = listener.local_addr().expect("address");
        let state = Arc::new(State {
            upstream: upstream.to_owned(),
            http: reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("client"),
            rules: Mutex::default(),
            seen: Mutex::default(),
            connections: Mutex::default(),
            streaming: Arc::default(),
        });
        let accept_state = state.clone();
        let task = tokio::spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let state = accept_state.clone();
                let connection = tokio::spawn(serve(stream, state.clone()));
                state
                    .connections
                    .lock()
                    .expect("connections")
                    .push(connection.abort_handle());
            }
        });
        Self {
            address,
            state,
            task: Some(task),
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Every request seen so far.
    pub fn seen(&self) -> Vec<Seen> {
        self.state.seen.lock().expect("seen").clone()
    }

    pub fn lookup(&self, rule: LookupRule) {
        self.state.rules.lock().expect("rules").lookup = rule;
    }

    pub fn objects(&self, rule: ObjectRule) {
        self.state.rules.lock().expect("rules").objects = rule;
    }

    /// Answers every roster read with these bytes.
    pub fn hold_roster(&self, bytes: Option<Vec<u8>>) {
        self.state.rules.lock().expect("rules").roster = bytes;
    }

    pub fn commits(&self, rule: CommitRule) {
        let mut rules = self.state.rules.lock().expect("rules");
        rules.commit = rule;
        rules.commits = 0;
    }

    /// Answers the next `n` pack writes with a 500.
    pub fn fail_puts(&self, n: u32) {
        self.state.rules.lock().expect("rules").fail_puts = n;
    }

    /// Holds the next object read this long before forwarding it.
    pub fn delay_first_object(&self, delay: Duration) {
        self.state.rules.lock().expect("rules").first_object_delay = Some(delay);
    }

    pub fn delay_dictionary(&self, delay: Option<Duration>) {
        self.state.rules.lock().expect("rules").dictionary_delay = delay;
    }

    /// Answers the next cache request with a 401.
    pub fn unauthorized_once(&self) {
        self.state.rules.lock().expect("rules").unauthorized_once = true;
    }

    /// Answers OIDC exchanges itself with this session token.
    pub fn oidc_session(&self, token: Option<String>) {
        self.state.rules.lock().expect("rules").oidc_session = token;
    }

    /// Object responses whose bodies are still being sent.
    pub fn streaming(&self) -> usize {
        self.state.streaming.load(Ordering::SeqCst)
    }

    /// Stops listening: the next connection is refused.
    pub fn go_down(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        for connection in self
            .state
            .connections
            .lock()
            .expect("connections")
            .drain(..)
        {
            connection.abort();
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.go_down();
    }
}

async fn serve(stream: TcpStream, state: Arc<State>) {
    let service = service_fn(move |request| handle(state.clone(), request));
    let _ = hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await;
}
