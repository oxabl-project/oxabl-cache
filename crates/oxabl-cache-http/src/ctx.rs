//! Per-request context: the request id and the fields the access log may carry.

use serde_json::Value;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Default)]
struct Fields {
    tenant: Option<String>,
    cache: Option<String>,
    principal: Option<String>,
    counts: Vec<(&'static str, u64)>,
}

/// Shared between the envelope middleware, the extractors and the handlers.
#[derive(Debug, Clone)]
pub struct RequestCtx {
    pub(crate) id: String,
    fields: Arc<Mutex<Fields>>,
    route: Arc<OnceLock<String>>,
}

impl RequestCtx {
    pub(crate) fn new(id: String) -> Self {
        Self {
            id,
            fields: Arc::default(),
            route: Arc::default(),
        }
    }

    fn with(&self, update: impl FnOnce(&mut Fields)) {
        update(&mut self.fields.lock().unwrap_or_else(|e| e.into_inner()));
    }

    pub(crate) fn set_route(&self, route: &str) {
        let _ = self.route.set(route.to_owned());
    }

    pub(crate) fn route(&self) -> String {
        self.route
            .get()
            .cloned()
            .unwrap_or_else(|| "unmatched".to_owned())
    }

    pub(crate) fn set_tenant(&self, tenant: String, principal: String) {
        self.with(|f| {
            f.tenant = Some(tenant);
            f.principal = Some(principal);
        });
    }

    pub(crate) fn set_cache(&self, cache: String) {
        self.with(|f| f.cache = Some(cache));
    }

    /// Records a count or byte size for the access log.
    pub(crate) fn count(&self, name: &'static str, value: u64) {
        self.with(|f| f.counts.push((name, value)));
    }

    pub(crate) fn log_fields(&self) -> Vec<(&'static str, Value)> {
        let fields = self.fields.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        for (name, value) in [
            ("tenant", &fields.tenant),
            ("cache", &fields.cache),
            ("principal", &fields.principal),
        ] {
            if let Some(value) = value {
                out.push((name, Value::from(value.as_str())));
            }
        }
        for (name, value) in &fields.counts {
            out.push((name, Value::from(*value)));
        }
        out
    }
}
