//! The export is the billing input: the CLI fixes its order and refuses anything else.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

fn answer_once(body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut head = [0u8; 4096];
        let _ = stream.read(&mut head);
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/csv\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(reply.as_bytes()).expect("reply");
    });
    url
}

fn export(url: &str, token_file: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oxabl-cache-admin"))
        .args(["--server", url, "--token-file"])
        .arg(token_file)
        .args(["usage", "export", "--month", "2026-10"])
        .output()
        .expect("run")
}

#[test]
fn rows_come_out_sorted_and_a_malformed_export_is_refused() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("op-order-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let token_file = dir.join("token");
    std::fs::write(&token_file, "oxo_x_y\n").expect("token");

    let unsorted = "month,tenant,cache,metric,value\n\
        2026-10,t_b,c_1,requests,5\n2026-10,t_a,c_2,hits,1\n2026-10,t_a,c_1,requests,2\n2026-10,t_a,c_1,hits,3\n";
    let sorted = export(&answer_once(unsorted), &token_file);
    assert_eq!(sorted.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&sorted.stdout),
        "month,tenant,cache,metric,value\n\
         2026-10,t_a,c_1,hits,3\n2026-10,t_a,c_1,requests,2\n2026-10,t_a,c_2,hits,1\n2026-10,t_b,c_1,requests,5\n"
    );

    for body in [
        "tenant,month\n",
        "month,tenant,cache,metric,value\n2026-10,t_a,c_1,hits\n",
        "",
    ] {
        let refused = export(&answer_once(body), &token_file);
        assert_eq!(refused.status.code(), Some(1), "{body:?}");
        assert!(refused.stdout.is_empty(), "nothing is printed for {body:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
