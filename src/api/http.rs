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
        return Err("The address is empty.".to_string());
    }
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("The address is not valid: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "Only http and https addresses are sent, not {}.",
            parsed.scheme()
        ));
    }
    let mut client = reqwest::Client::builder()
        .redirect(if request.settings.follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        })
        .danger_accept_invalid_certs(!request.settings.verify_tls);
    if request.settings.timeout_secs > 0 {
        client = client.timeout(Duration::from_secs(request.settings.timeout_secs));
    }
    let client = client.build().map_err(|e| e.to_string())?;
    let method = reqwest::Method::from_bytes(request.method.as_str().as_bytes())
        .map_err(|e| e.to_string())?;
    let mut builder = client.request(method, parsed);
    for (shown, value) in &request.headers {
        let name = reqwest::header::HeaderName::from_bytes(shown.as_bytes())
            .map_err(|_| format!("The header name \"{shown}\" is not valid."))?;
        let value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| format!("The value of the header {shown} is not valid."))?;
        builder = builder.header(name, value);
    }
    builder = match request.body {
        ResolvedBody::None => builder,
        ResolvedBody::Bytes(bytes) => builder.body(bytes),
        ResolvedBody::File(path) => {
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| format!("{path}: {e}"))?;
            builder.body(bytes)
        }
    };
    let started = Instant::now();
    let mut response = builder
        .send()
        .await
        .map_err(|e| explain(&e, &request.settings))?;
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
