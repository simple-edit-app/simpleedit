//! Sending a request and reading what comes back.

use std::time::{Duration, Instant};

use super::model::{Resolved, ResolvedBody};

/// The most of a body that is kept.
pub const MAX_BODY: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// What follows the value: `Path=/; HttpOnly`.
    pub attributes: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub cookies: Vec<Cookie>,
    pub body: Vec<u8>,
    /// There was more than `MAX_BODY`.
    pub truncated: bool,
    pub ms: u64,
    /// The address the answer came from, after the redirects.
    pub url: String,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The text of the body, as far as it is one.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// `name=value; Path=/` as a cookie.
pub fn parse_cookie(line: &str) -> Option<Cookie> {
    let (pair, attributes) = match line.split_once(';') {
        Some((p, a)) => (p, a.trim()),
        None => (line, ""),
    };
    let (name, value) = pair.split_once('=')?;
    let name = name.trim();
    (!name.is_empty()).then(|| Cookie {
        name: name.to_string(),
        value: value.trim().to_string(),
        attributes: attributes.to_string(),
    })
}

/// Sends a request and waits for the whole answer.
pub async fn send(request: Resolved) -> Result<Response, String> {
    let url = request.url.trim();
    if url.is_empty() {
        return Err(t!("api.err_address_empty").to_string());
    }
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| t!("api.err_address_invalid", error = e).to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(t!("api.err_scheme", scheme = parsed.scheme()).to_string());
    }
    // Redirects are followed here, not by the client: the headers of the
    // request (`X-API-Key`, tokens of any name) must not go along to another
    // site.
    let mut client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .danger_accept_invalid_certs(!request.settings.verify_tls);
    if request.settings.timeout_secs > 0 {
        client = client.timeout(Duration::from_secs(request.settings.timeout_secs));
    }
    let client = client.build().map_err(|e| e.to_string())?;
    let mut method = reqwest::Method::from_bytes(request.method.as_str().as_bytes())
        .map_err(|e| e.to_string())?;
    let mut headers_out: Vec<(reqwest::header::HeaderName, reqwest::header::HeaderValue)> =
        Vec::new();
    for (shown, value) in &request.headers {
        let name = reqwest::header::HeaderName::from_bytes(shown.as_bytes())
            .map_err(|_| t!("api.err_header_name", name = shown).to_string())?;
        let value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| t!("api.err_header_value", name = shown).to_string())?;
        headers_out.push((name, value));
    }
    let mut body: Option<Vec<u8>> = match request.body {
        ResolvedBody::None => None,
        ResolvedBody::Bytes(bytes) => Some(bytes),
        ResolvedBody::File(path) if path.trim().is_empty() => {
            return Err(t!("api.err_file_not_chosen").to_string());
        }
        ResolvedBody::File(path) => Some(
            tokio::fs::read(&path)
                .await
                .map_err(|e| format!("{path}: {e}"))?,
        ),
    };
    let started = Instant::now();
    let mut target = parsed;
    let mut hops = 0;
    let mut response = loop {
        let mut builder = client.request(method.clone(), target.clone());
        for (name, value) in &headers_out {
            builder = builder.header(name.clone(), value.clone());
        }
        if let Some(bytes) = &body {
            builder = builder.body(bytes.clone());
        }
        let response = builder
            .send()
            .await
            .map_err(|e| explain(&e, &request.settings))?;
        let status = response.status();
        let next = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|l| target.join(l).ok());
        let (true, true, Some(next)) = (
            request.settings.follow_redirects,
            status.is_redirection(),
            next,
        ) else {
            break response;
        };
        hops += 1;
        if hops > 10 || !matches!(next.scheme(), "http" | "https") {
            return Err(t!("api.err_redirects").to_string());
        }
        // Like a browser: 303 (and 301/302 for a POST) turn into a GET.
        if status == reqwest::StatusCode::SEE_OTHER
            || (matches!(status.as_u16(), 301 | 302) && method == reqwest::Method::POST)
        {
            if method != reqwest::Method::HEAD {
                method = reqwest::Method::GET;
            }
            body = None;
            headers_out.retain(|(n, _)| !n.as_str().starts_with("content-"));
        }
        if !same_origin(&target, &next) {
            headers_out.retain(|(n, _)| safe_across_sites(n.as_str()));
            if body.is_some() && !matches!(status.as_u16(), 307 | 308) {
                body = None;
            }
        }
        target = next;
    };
    let status = response.status();
    let version = format!("{:?}", response.version());
    let final_url = response.url().to_string();
    let mut headers = Vec::new();
    let mut cookies = Vec::new();
    for (name, value) in response.headers() {
        let text = String::from_utf8_lossy(value.as_bytes()).into_owned();
        if name.as_str().eq_ignore_ascii_case("set-cookie") {
            if let Some(cookie) = parse_cookie(&text) {
                cookies.push(cookie);
            }
        }
        headers.push((name.as_str().to_string(), text));
    }
    let mut body = Vec::new();
    let mut truncated = false;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| explain(&e, &request.settings))?
    {
        if body.len() + chunk.len() > MAX_BODY {
            let room = MAX_BODY - body.len();
            body.extend_from_slice(&chunk[..room]);
            truncated = true;
            break;
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Response {
        status: status.as_u16(),
        reason: status.canonical_reason().unwrap_or("").to_string(),
        version,
        headers,
        cookies,
        body,
        truncated,
        ms: started.elapsed().as_millis() as u64,
        url: final_url,
    })
}

/// Whether two addresses have the same scheme, host and port.
fn same_origin(a: &reqwest::Url, b: &reqwest::Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// The headers that follow a redirect to another site: those that say what
/// is wanted, never those that say who asks.
fn safe_across_sites(name: &str) -> bool {
    matches!(
        name,
        "accept"
            | "accept-language"
            | "accept-encoding"
            | "user-agent"
            | "content-type"
            | "content-length"
            | "content-language"
    )
}

/// What went wrong, in words that say what to do.
fn explain(error: &reqwest::Error, settings: &super::model::Settings) -> String {
    if error.is_timeout() {
        return format!(
            "The request timed out after {} s: the server did not answer in time.",
            settings.timeout_secs
        );
    }
    let mut chain = vec![error.to_string()];
    let mut source = std::error::Error::source(error);
    while let Some(inner) = source {
        chain.push(inner.to_string());
        source = inner.source();
    }
    let deepest = chain.last().cloned().unwrap_or_default();
    if error.is_connect() {
        return format!("Could not connect: {deepest}");
    }
    if error.is_redirect() {
        return "Too many redirects.".to_string();
    }
    deepest
}

// ------------------------------------------------------------------ reading

/// What kind of text a body is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Json,
    Xml,
    Html,
    Text,
    /// Not text: bytes.
    Binary,
}

/// What a body is, from its type and its first bytes.
pub fn kind_of(content_type: Option<&str>, body: &[u8]) -> Kind {
    let head = &body[..body.len().min(8000)];
    if head.contains(&0) {
        return Kind::Binary;
    }
    let ct = content_type.unwrap_or("").to_lowercase();
    if ct.contains("json") {
        return Kind::Json;
    }
    if ct.contains("html") {
        return Kind::Html;
    }
    if ct.contains("xml") {
        return Kind::Xml;
    }
    if ct.starts_with("image/")
        || ct.contains("octet-stream")
        || ct.contains("pdf")
        || ct.contains("zip")
        || ct.starts_with("audio/")
        || ct.starts_with("video/")
    {
        return Kind::Binary;
    }
    // No type, or a type that says nothing: look at what it is.
    let text = String::from_utf8_lossy(head);
    let trimmed = text.trim_start();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_slice::<serde_json::Value>(body).is_ok()
    {
        Kind::Json
    } else if trimmed.starts_with("<!DOCTYPE html") || trimmed.to_lowercase().starts_with("<html") {
        Kind::Html
    } else if trimmed.starts_with("<?xml") {
        Kind::Xml
    } else {
        Kind::Text
    }
}

/// The body laid out for reading: JSON indented, markup indented. `None` when
/// there is nothing to do.
pub fn pretty(kind: Kind, text: &str) -> Option<String> {
    match kind {
        Kind::Json => serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok()),
        Kind::Xml | Kind::Html => Some(pretty_markup(text)),
        _ => None,
    }
}

const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// XML or HTML indented, a tag by line (a tag with only a short text in it
/// stays on one line).
pub fn pretty_markup(text: &str) -> String {
    #[derive(Debug, PartialEq)]
    enum Token {
        Open(String),
        Close(String),
        /// A tag that stands alone: `<br/>`, a comment, a declaration.
        Alone(String),
        Text(String),
    }
    let mut tokens: Vec<Token> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(inner) = rest.strip_prefix('<') {
            let end = if inner.starts_with("!--") {
                inner.find("-->").map(|i| i + 3)
            } else {
                inner.find('>').map(|i| i + 1)
            };
            let Some(end) = end else {
                tokens.push(Token::Text(rest.to_string()));
                break;
            };
            let tag = format!("<{}", &inner[..end]);
            rest = &inner[end..];
            let body = tag.trim_start_matches('<');
            let name = |s: &str| {
                s.trim_start_matches('/')
                    .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
                    .next()
                    .unwrap_or("")
                    .to_lowercase()
            };
            tokens.push(if body.starts_with('/') {
                Token::Close(tag.clone())
            } else if body.starts_with('!')
                || body.starts_with('?')
                || tag.ends_with("/>")
                || VOID.contains(&name(body).as_str())
            {
                Token::Alone(tag.clone())
            } else {
                Token::Open(tag.clone())
            });
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            let chunk = rest[..end].trim();
            if !chunk.is_empty() {
                tokens.push(Token::Text(
                    chunk.split_whitespace().collect::<Vec<_>>().join(" "),
                ));
            }
            rest = &rest[end..];
        }
    }
    let mut out = String::new();
    let mut depth = 0usize;
    let mut i = 0;
    let pad = |depth: usize| "  ".repeat(depth);
    while i < tokens.len() {
        match &tokens[i] {
            Token::Open(tag) => {
                // `<a>text</a>` on one line.
                if let (Some(Token::Text(t)), Some(Token::Close(close))) =
                    (tokens.get(i + 1), tokens.get(i + 2))
                {
                    if t.len() <= 80 {
                        out.push_str(&format!("{}{tag}{t}{close}\n", pad(depth)));
                        i += 3;
                        continue;
                    }
                }
                out.push_str(&format!("{}{tag}\n", pad(depth)));
                depth += 1;
            }
            Token::Close(tag) => {
                depth = depth.saturating_sub(1);
                out.push_str(&format!("{}{tag}\n", pad(depth)));
            }
            Token::Alone(tag) => out.push_str(&format!("{}{tag}\n", pad(depth))),
            Token::Text(t) => out.push_str(&format!("{}{t}\n", pad(depth))),
        }
        i += 1;
    }
    out.trim_end().to_string()
}

/// What an answer is called by its status: its class decides the color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Info,
    Success,
    Redirect,
    ClientError,
    ServerError,
}

pub fn class_of(status: u16) -> Class {
    match status {
        100..=199 => Class::Info,
        200..=299 => Class::Success,
        300..=399 => Class::Redirect,
        400..=499 => Class::ClientError,
        _ => Class::ServerError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::model::{Method, Settings};
    use std::net::TcpListener;

    use crate::api::testing::{answer, serve};

    fn resolved(method: Method, url: String) -> Resolved {
        Resolved {
            method,
            url,
            headers: vec![("X-Test".into(), "yes".into())],
            body: ResolvedBody::None,
            settings: Settings::default(),
            missing: Vec::new(),
        }
    }

    fn run<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn a_request_goes_and_the_answer_is_read() {
        let (port, seen) = serve(|_| {
            vec![answer(
                "201 Created",
                &[
                    "Content-Type: application/json",
                    "Set-Cookie: sid=abc; Path=/; HttpOnly",
                ],
                "{\"ok\":true}",
            )]
        });
        let mut request = resolved(Method::Post, format!("http://127.0.0.1:{port}/items?a=1"));
        request.body = ResolvedBody::Bytes(b"{\"n\":1}".to_vec());
        let response = run(send(request)).unwrap();
        assert_eq!(
            (response.status, response.reason.as_str()),
            (201, "Created")
        );
        assert_eq!(response.text(), "{\"ok\":true}");
        assert_eq!(response.header("content-type"), Some("application/json"));
        assert_eq!(
            response.cookies,
            vec![Cookie {
                name: "sid".into(),
                value: "abc".into(),
                attributes: "Path=/; HttpOnly".into()
            }]
        );
        assert!(!response.truncated && response.url.ends_with("/items?a=1"));
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].request_line, "POST /items?a=1 HTTP/1.1");
        assert_eq!(seen[0].header("x-test"), Some("yes"));
        assert_eq!(seen[0].body, b"{\"n\":1}");
    }

    #[test]
    fn a_redirect_is_followed_or_shown_as_it_is() {
        let answers = |port: u16| {
            vec![
                answer(
                    "302 Found",
                    &[&format!("Location: http://127.0.0.1:{port}/final")],
                    "",
                ),
                answer("200 OK", &[], "end"),
            ]
        };
        // Followed: the second request answers.
        let (port, seen) = serve(answers);
        let response = run(send(resolved(
            Method::Get,
            format!("http://127.0.0.1:{port}/start"),
        )))
        .unwrap();
        assert_eq!((response.status, response.text()), (200, "end".to_string()));
        assert!(response.url.ends_with("/final"));
        assert_eq!(seen.lock().unwrap().len(), 2);
        // Not followed: the redirect itself is what comes back.
        let (port, _) = serve(answers);
        let mut request = resolved(Method::Get, format!("http://127.0.0.1:{port}/start"));
        request.settings.follow_redirects = false;
        let response = run(send(request)).unwrap();
        assert_eq!(response.status, 302);
        assert!(response.header("location").unwrap().ends_with("/final"));
    }

    #[test]
    fn what_the_server_receives_is_what_the_request_says() {
        use crate::api::model::{Auth, AuthKind, Body, BodyKind, Pair, Request};
        use std::collections::HashMap;
        let (port, seen) = serve(|_| {
            vec![
                answer("200 OK", &[], "one"),
                answer("200 OK", &[], "two"),
                answer("200 OK", &[], "three"),
            ]
        });
        let vars: HashMap<String, String> =
            [("host", format!("127.0.0.1:{port}")), ("tk", "abc".into())]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
        // A JSON body, a parameter and a bearer token.
        let mut one = Request::new("one");
        one.method = Method::Post;
        one.url = "http://{{host}}/items".into();
        one.params = vec![Pair::new("q", "a b")];
        one.auth = Auth {
            kind: AuthKind::Bearer,
            token: "{{tk}}".into(),
            ..Auth::default()
        };
        one.body = Body {
            kind: BodyKind::Json,
            text: "{\"n\": 1}".into(),
            ..Body::default()
        };
        assert_eq!(run(send(one.resolve(&vars))).unwrap().text(), "one");
        // A form, and the sign-in of a user.
        let mut two = Request::new("two");
        two.method = Method::Put;
        two.url = "{{host}}/f".into();
        two.auth = Auth {
            kind: AuthKind::Basic,
            username: "u".into(),
            password: "p".into(),
            ..Auth::default()
        };
        two.body = Body {
            kind: BodyKind::Form,
            form: vec![Pair::new("a b", "c&d")],
            ..Body::default()
        };
        assert_eq!(run(send(two.resolve(&vars))).unwrap().text(), "two");
        // A file, sent as it is.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("data.bin");
        std::fs::write(&file, [0u8, 1, 2, 255]).unwrap();
        let mut three = Request::new("three");
        three.method = Method::Patch;
        three.url = "{{host}}/file".into();
        three.body = Body {
            kind: BodyKind::File,
            file: file.to_string_lossy().to_string(),
            ..Body::default()
        };
        assert_eq!(run(send(three.resolve(&vars))).unwrap().text(), "three");
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].request_line, "POST /items?q=a%20b HTTP/1.1");
        assert_eq!(seen[0].header("authorization"), Some("Bearer abc"));
        assert_eq!(seen[0].header("content-type"), Some("application/json"));
        assert_eq!(seen[0].body, b"{\"n\": 1}");
        assert_eq!(seen[1].request_line, "PUT /f HTTP/1.1");
        assert_eq!(seen[1].header("authorization"), Some("Basic dTpw"));
        assert_eq!(
            seen[1].header("content-type"),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(seen[1].body, b"a+b=c%26d");
        assert_eq!(seen[2].request_line, "PATCH /file HTTP/1.1");
        assert_eq!(seen[2].body, vec![0u8, 1, 2, 255]);
    }

    #[test]
    fn a_broken_compressed_answer_is_an_error_and_a_slow_server_times_out() {
        // The server says it is gzip and is not: that is an error, said as one.
        let (port, _) = serve(|_| vec![answer("200 OK", &["Content-Encoding: gzip"], "not gzip")]);
        let error = run(send(resolved(
            Method::Get,
            format!("http://127.0.0.1:{port}/"),
        )))
        .unwrap_err();
        assert!(!error.is_empty());
        // A server that never answers: the time limit ends it.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hold = std::thread::spawn(move || {
            let _held = listener.accept();
            std::thread::sleep(std::time::Duration::from_millis(2500));
        });
        let mut slow = resolved(Method::Get, format!("http://127.0.0.1:{port}/"));
        slow.settings.timeout_secs = 1;
        let error = run(send(slow)).unwrap_err();
        assert!(error.contains("timed out after 1 s"), "{error}");
        drop(hold);
    }

    #[test]
    fn a_missing_address_or_scheme_is_refused_before_anything_is_sent() {
        let empty = run(send(resolved(Method::Get, String::new())));
        assert!(empty.unwrap_err().contains("empty"));
        let ftp = run(send(resolved(Method::Get, "ftp://example.com/x".into())));
        assert!(ftp.unwrap_err().contains("http"));
        let bad = run(send(resolved(Method::Get, "http://".into())));
        assert!(bad.unwrap_err().contains("not valid"));
    }

    #[test]
    fn a_header_that_cannot_be_sent_is_said_not_dropped() {
        let mut request = resolved(Method::Get, "http://127.0.0.1:1/".into());
        request.headers = vec![("Bad Name".into(), "x".into())];
        assert!(run(send(request)).unwrap_err().contains("Bad Name"));
        let mut request = resolved(Method::Get, "http://127.0.0.1:1/".into());
        request.headers = vec![("X-Ok".into(), "line\nbreak".into())];
        assert!(run(send(request)).unwrap_err().contains("X-Ok"));
    }

    #[test]
    fn a_server_that_is_not_there_is_a_connection_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let error = run(send(resolved(
            Method::Get,
            format!("http://127.0.0.1:{port}/"),
        )))
        .unwrap_err();
        assert!(error.starts_with("Could not connect"), "{error}");
    }

    #[test]
    fn a_redirect_to_another_site_does_not_carry_the_custom_headers() {
        let (port_b, seen_b) = serve(|_| vec![answer("200 OK", &[], "there")]);
        let (port_a, seen_a) = serve(|_| {
            vec![answer(
                "302 Found",
                &[&format!("Location: http://localhost:{port_b}/next")],
                "",
            )]
        });
        let mut request = resolved(Method::Get, format!("http://127.0.0.1:{port_a}/"));
        request.headers.push(("X-API-Key".into(), "secret".into()));
        request.settings.follow_redirects = true;
        let response = run(send(request)).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(
            seen_a.lock().unwrap()[0].header("X-API-Key"),
            Some("secret")
        );
        let b = seen_b.lock().unwrap();
        assert!(b[0].header("X-API-Key").is_none() && b[0].header("X-Test").is_none());
    }

    #[test]
    fn a_cookie_line_is_read() {
        assert_eq!(
            parse_cookie("a=b=c; Secure"),
            Some(Cookie {
                name: "a".into(),
                value: "b=c".into(),
                attributes: "Secure".into()
            })
        );
        assert_eq!(parse_cookie("no equal sign"), None);
        assert_eq!(parse_cookie("=x"), None);
    }

    #[test]
    fn a_body_is_told_by_its_type_or_by_what_it_holds() {
        assert_eq!(
            kind_of(Some("application/json; charset=utf-8"), b"{}"),
            Kind::Json
        );
        assert_eq!(kind_of(Some("application/vnd.api+json"), b"{}"), Kind::Json);
        assert_eq!(kind_of(Some("text/html"), b"<p>"), Kind::Html);
        assert_eq!(kind_of(Some("application/xml"), b"<a/>"), Kind::Xml);
        assert_eq!(kind_of(Some("image/png"), b"abc"), Kind::Binary);
        assert_eq!(kind_of(None, b"a\0b"), Kind::Binary);
        // No type: look at it.
        assert_eq!(kind_of(None, b" {\"a\": 1}"), Kind::Json);
        // An API that says text/plain and sends JSON is read as JSON.
        assert_eq!(kind_of(Some("text/plain"), b"{\"a\": 1}"), Kind::Json);
        assert_eq!(kind_of(Some("text/plain"), b"plain words"), Kind::Text);
        assert_eq!(kind_of(None, b"<?xml version=\"1.0\"?><a/>"), Kind::Xml);
        assert_eq!(kind_of(None, b"<!DOCTYPE html><html></html>"), Kind::Html);
        assert_eq!(kind_of(None, b"{not json"), Kind::Text);
    }

    #[test]
    fn json_and_markup_are_laid_out_for_reading() {
        assert_eq!(
            pretty(Kind::Json, "{\"a\":[1,2],\"b\":{\"c\":null}}").unwrap(),
            "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": {\n    \"c\": null\n  }\n}"
        );
        assert_eq!(pretty(Kind::Json, "not json"), None);
        assert_eq!(pretty(Kind::Text, "x"), None);
        assert_eq!(
            pretty(
                Kind::Xml,
                "<?xml version=\"1.0\"?><root><a>1</a><b><c/></b></root>"
            )
            .unwrap(),
            "<?xml version=\"1.0\"?>\n<root>\n  <a>1</a>\n  <b>\n    <c/>\n  </b>\n</root>"
        );
        assert_eq!(
            pretty_markup("<div><p>Hello   <b>you</b></p><br><img src=x></div>"),
            "<div>\n  <p>\n    Hello\n    <b>you</b>\n  </p>\n  <br>\n  <img src=x>\n</div>"
        );
        // A comment and a declaration do not open anything.
        assert_eq!(
            pretty_markup("<!DOCTYPE html><!-- c --><p>x</p>"),
            "<!DOCTYPE html>\n<!-- c -->\n<p>x</p>"
        );
    }

    #[test]
    fn a_status_has_a_class() {
        assert_eq!(class_of(101), Class::Info);
        assert_eq!(class_of(204), Class::Success);
        assert_eq!(class_of(301), Class::Redirect);
        assert_eq!(class_of(404), Class::ClientError);
        assert_eq!(class_of(503), Class::ServerError);
    }
}
