//! A small server on this machine, for the tests: it keeps what it is asked
//! and answers what it is told to.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// What the little server was asked.
#[derive(Debug, Clone, Default)]
pub struct Seen {
    pub request_line: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A server on this machine that answers `answers` one connection after
/// the other, and keeps what it was asked.
pub fn serve(answers: impl FnOnce(u16) -> Vec<Vec<u8>>) -> (u16, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let answers = answers(port);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for answer in answers {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            let (head_end, length) = loop {
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break (data.len(), 0);
                }
                data.extend_from_slice(&buf[..n]);
                if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&data[..at]).to_lowercase();
                    let length = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    break (at + 4, length);
                }
            };
            while data.len() < head_end + length {
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                data.extend_from_slice(&buf[..n]);
            }
            let head = String::from_utf8_lossy(&data[..head_end.saturating_sub(4)]).to_string();
            let mut lines = head.lines();
            let request_line = lines.next().unwrap_or("").to_string();
            let headers = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .collect();
            log.lock().unwrap().push(Seen {
                request_line,
                headers,
                body: data[head_end.min(data.len())..].to_vec(),
            });
            let _ = stream.write_all(&answer);
            let _ = stream.flush();
        }
    });
    (port, seen)
}

pub fn answer(status: &str, headers: &[&str], body: &str) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\n");
    for h in headers {
        out.push_str(h);
        out.push_str("\r\n");
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    out.into_bytes()
}
