//! What an API request is made of, the collections that keep them, and how a
//! request is made ready to send: the `{{variables}}` filled in, the
//! parameters in the address, the sign-in in the headers.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Method {
    pub const ALL: [Method; 7] = [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Patch,
        Method::Delete,
        Method::Head,
        Method::Options,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }

    pub fn parse(text: &str) -> Option<Method> {
        Method::ALL
            .into_iter()
            .find(|m| m.as_str().eq_ignore_ascii_case(text.trim()))
    }

    /// The methods that usually carry a body.
    pub fn has_body(self) -> bool {
        matches!(
            self,
            Method::Post | Method::Put | Method::Patch | Method::Delete
        )
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A line of a table of keys and values (parameters, headers, a form).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pair {
    pub enabled: bool,
    pub key: String,
    pub value: String,
}

impl Default for Pair {
    fn default() -> Self {
        Pair {
            enabled: true,
            key: String::new(),
            value: String::new(),
        }
    }
}

impl Pair {
    pub fn new(key: &str, value: &str) -> Pair {
        Pair {
            enabled: true,
            key: key.to_string(),
            value: value.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    #[default]
    None,
    Json,
    Text,
    Form,
    File,
}

impl BodyKind {
    pub const ALL: [BodyKind; 5] = [
        BodyKind::None,
        BodyKind::Json,
        BodyKind::Text,
        BodyKind::Form,
        BodyKind::File,
    ];
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Body {
    pub kind: BodyKind,
    /// The text of a JSON or a plain body.
    pub text: String,
    /// The fields of a form.
    pub form: Vec<Pair>,
    /// The file sent as it is.
    pub file: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    #[default]
    None,
    Bearer,
    Basic,
    ApiKey,
}

impl AuthKind {
    pub const ALL: [AuthKind; 4] = [
        AuthKind::None,
        AuthKind::Bearer,
        AuthKind::Basic,
        AuthKind::ApiKey,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Auth {
    pub kind: AuthKind,
    pub token: String,
    pub username: String,
    pub password: String,
    pub key_name: String,
    pub key_value: String,
    /// An API key goes in a header (`true`) or in the address.
    pub key_in_header: bool,
}

impl Default for Auth {
    fn default() -> Self {
        Auth {
            kind: AuthKind::None,
            token: String::new(),
            username: String::new(),
            password: String::new(),
            key_name: String::new(),
            key_value: String::new(),
            key_in_header: true,
        }
    }
}

/// How a request is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Seconds before it gives up (0: no limit).
    pub timeout_secs: u64,
    pub follow_redirects: bool,
    /// The certificate of the server is checked.
    pub verify_tls: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            timeout_secs: 30,
            follow_redirects: true,
            verify_tls: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    pub id: u64,
    pub name: String,
    pub method: Method,
    pub url: String,
    pub params: Vec<Pair>,
    pub headers: Vec<Pair>,
    pub body: Body,
    pub auth: Auth,
    pub settings: Settings,
}

impl Request {
    pub fn new(name: &str) -> Request {
        Request {
            name: name.to_string(),
            ..Request::default()
        }
    }

    /// What the request is called in a list: its name, or else its address.
    pub fn title(&self) -> String {
        if !self.name.trim().is_empty() {
            self.name.trim().to_string()
        } else if !self.url.trim().is_empty() {
            self.url.trim().to_string()
        } else {
            String::new()
        }
    }
}

/// A folder of requests, in a collection or in another folder.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folder {
    pub id: u64,
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum Item {
    Folder(Folder),
    Request(Request),
}

impl Item {
    pub fn id(&self) -> u64 {
        match self {
            Item::Folder(f) => f.id,
            Item::Request(r) => r.id,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Collection {
    pub id: u64,
    pub name: String,
    pub items: Vec<Item>,
}

impl Collection {
    /// The request with this id, wherever it is.
    pub fn request(&self, id: u64) -> Option<&Request> {
        fn find(items: &[Item], id: u64) -> Option<&Request> {
            items.iter().find_map(|item| match item {
                Item::Request(r) if r.id == id => Some(r),
                Item::Folder(f) => find(&f.items, id),
                _ => None,
            })
        }
        find(&self.items, id)
    }

    pub fn request_mut(&mut self, id: u64) -> Option<&mut Request> {
        fn find(items: &mut [Item], id: u64) -> Option<&mut Request> {
            for item in items {
                match item {
                    Item::Request(r) if r.id == id => return Some(r),
                    Item::Folder(f) => {
                        if let Some(found) = find(&mut f.items, id) {
                            return Some(found);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        find(&mut self.items, id)
    }

    /// The list of items a folder (or the collection, for id 0) holds.
    pub fn items_of(&mut self, folder: u64) -> Option<&mut Vec<Item>> {
        fn has(items: &[Item], id: u64) -> bool {
            items.iter().any(|i| match i {
                Item::Folder(f) => f.id == id || has(&f.items, id),
                _ => false,
            })
        }
        #[allow(clippy::ptr_arg)]
        fn find(items: &mut Vec<Item>, id: u64) -> Option<&mut Vec<Item>> {
            let at = items.iter().position(|i| match i {
                Item::Folder(f) => f.id == id || has(&f.items, id),
                _ => false,
            })?;
            let Item::Folder(f) = &mut items[at] else {
                return None;
            };
            if f.id == id {
                Some(&mut f.items)
            } else {
                find(&mut f.items, id)
            }
        }
        if folder == 0 {
            Some(&mut self.items)
        } else {
            find(&mut self.items, folder)
        }
    }

    /// Takes an item out, wherever it is.
    pub fn remove(&mut self, id: u64) -> Option<Item> {
        fn take(items: &mut Vec<Item>, id: u64) -> Option<Item> {
            if let Some(at) = items.iter().position(|i| i.id() == id) {
                return Some(items.remove(at));
            }
            for item in items.iter_mut() {
                if let Item::Folder(f) = item {
                    if let Some(found) = take(&mut f.items, id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        take(&mut self.items, id)
    }

    /// Puts a copy of a request after it, under a new id; the id of the copy.
    pub fn duplicate(&mut self, id: u64, new_id: u64, suffix: &str) -> Option<u64> {
        fn go(items: &mut Vec<Item>, id: u64, new_id: u64, suffix: &str) -> bool {
            if let Some(at) = items
                .iter()
                .position(|i| matches!(i, Item::Request(r) if r.id == id))
            {
                if let Item::Request(r) = &items[at] {
                    let mut copy = r.clone();
                    copy.id = new_id;
                    copy.name = format!("{} {suffix}", r.title());
                    items.insert(at + 1, Item::Request(copy));
                    return true;
                }
            }
            items.iter_mut().any(|i| match i {
                Item::Folder(f) => go(&mut f.items, id, new_id, suffix),
                _ => false,
            })
        }
        go(&mut self.items, id, new_id, suffix).then_some(new_id)
    }

    /// Renames a request or a folder.
    pub fn rename(&mut self, id: u64, name: &str) -> bool {
        fn go(items: &mut [Item], id: u64, name: &str) -> bool {
            for item in items {
                match item {
                    Item::Request(r) if r.id == id => {
                        r.name = name.to_string();
                        return true;
                    }
                    Item::Folder(f) => {
                        if f.id == id {
                            f.name = name.to_string();
                            return true;
                        }
                        if go(&mut f.items, id, name) {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
            false
        }
        go(&mut self.items, id, name)
    }

    /// How many requests there are, in all the folders.
    pub fn count(&self) -> usize {
        fn count(items: &[Item]) -> usize {
            items
                .iter()
                .map(|i| match i {
                    Item::Request(_) => 1,
                    Item::Folder(f) => count(&f.items),
                })
                .sum()
        }
        count(&self.items)
    }

    /// The biggest id used, to give the next ones.
    pub fn max_id(&self) -> u64 {
        fn max(items: &[Item]) -> u64 {
            items
                .iter()
                .map(|i| match i {
                    Item::Request(r) => r.id,
                    Item::Folder(f) => f.id.max(max(&f.items)),
                })
                .max()
                .unwrap_or(0)
        }
        max(&self.items).max(self.id)
    }

    /// Gives each request and folder that has none an id of its own.
    pub fn number(&mut self, next: &mut u64) {
        fn go(items: &mut [Item], next: &mut u64) {
            for item in items {
                match item {
                    Item::Request(r) => {
                        if r.id == 0 {
                            *next += 1;
                            r.id = *next;
                        }
                    }
                    Item::Folder(f) => {
                        if f.id == 0 {
                            *next += 1;
                            f.id = *next;
                        }
                        go(&mut f.items, next);
                    }
                }
            }
        }
        go(&mut self.items, next);
    }
}

/// A variable of an environment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Variable {
    pub enabled: bool,
    pub key: String,
    pub value: String,
    /// Kept in the keychain, not in the file.
    pub secret: bool,
}

impl Default for Variable {
    fn default() -> Self {
        Variable {
            enabled: true,
            key: String::new(),
            value: String::new(),
            secret: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Environment {
    pub id: u64,
    pub name: String,
    pub vars: Vec<Variable>,
}

impl Environment {
    /// The variables that are on, by name.
    pub fn values(&self) -> HashMap<String, String> {
        self.vars
            .iter()
            .filter(|v| v.enabled && !v.key.trim().is_empty())
            .map(|v| (v.key.trim().to_string(), v.value.clone()))
            .collect()
    }
}

// ----------------------------------------------------------------- sending

/// What is sent: the request with everything filled in.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: ResolvedBody,
    pub settings: Settings,
    /// Variables the text asks for that nobody defined.
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedBody {
    None,
    Bytes(Vec<u8>),
    /// A file, read when the request goes.
    File(String),
}

/// Fills in the `{{name}}` of a text. What is not defined stays as it was,
/// and is noted in `missing`.
pub fn substitute(text: &str, vars: &HashMap<String, String>, missing: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = after[..end].trim();
        match vars.get(name) {
            Some(value) => out.push_str(value),
            None => {
                out.push_str(&rest[start..start + 2 + end + 2]);
                if !name.is_empty() && !missing.iter().any(|m| m == name) {
                    missing.push(name.to_string());
                }
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// A text for an address: only what cannot be mistaken is left as it is.
pub fn percent_encode(text: &str, plus_for_space: bool) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' if plus_for_space => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(BASE64[(n >> 18) as usize & 63] as char);
        out.push(BASE64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            BASE64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
}

/// The address with the parameters after it (before a `#`).
pub fn with_query(url: &str, query: &[(String, String)]) -> String {
    if query.is_empty() {
        return url.to_string();
    }
    let (base, fragment) = match url.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (url, None),
    };
    let encoded: Vec<String> = query
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k, false), percent_encode(v, false)))
        .collect();
    let joiner = if base.contains('?') {
        if base.ends_with('?') || base.ends_with('&') {
            ""
        } else {
            "&"
        }
    } else {
        "?"
    };
    let mut out = format!("{base}{joiner}{}", encoded.join("&"));
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

impl Request {
    /// The request as it goes: variables filled in, parameters in the address,
    /// the sign-in and the type of the body in the headers.
    pub fn resolve(&self, vars: &HashMap<String, String>) -> Resolved {
        let mut missing = Vec::new();
        let mut sub = |text: &str| substitute(text, vars, &mut missing);
        let mut url = sub(self.url.trim());
        let mut query: Vec<(String, String)> = self
            .params
            .iter()
            .filter(|p| p.enabled && !p.key.trim().is_empty())
            .map(|p| (sub(p.key.trim()), sub(&p.value)))
            .collect();
        let mut headers: Vec<(String, String)> = self
            .headers
            .iter()
            .filter(|h| h.enabled && !h.key.trim().is_empty())
            .map(|h| (sub(h.key.trim()), sub(&h.value)))
            .collect();
        // The sign-in, unless a header already says it.
        match self.auth.kind {
            AuthKind::None => {}
            AuthKind::Bearer => {
                let token = sub(self.auth.token.trim());
                if !token.is_empty() && !has_header(&headers, "Authorization") {
                    headers.push(("Authorization".into(), format!("Bearer {token}")));
                }
            }
            AuthKind::Basic => {
                let (user, password) = (sub(&self.auth.username), sub(&self.auth.password));
                let nobody = user.is_empty() && password.is_empty();
                if !has_header(&headers, "Authorization") && !nobody {
                    headers.push((
                        "Authorization".into(),
                        format!("Basic {}", base64(format!("{user}:{password}").as_bytes())),
                    ));
                }
            }
            AuthKind::ApiKey => {
                let (name, value) = (sub(self.auth.key_name.trim()), sub(&self.auth.key_value));
                if !name.is_empty() {
                    if self.auth.key_in_header {
                        if !has_header(&headers, &name) {
                            headers.push((name, value));
                        }
                    } else {
                        query.push((name, value));
                    }
                }
            }
        }
        // No scheme: it is a local address, most of the time.
        if !url.is_empty() && !url.contains("://") && !url.starts_with("{{") {
            url = format!("http://{url}");
        }
        let url = with_query(&url, &query);
        let (body, content_type) = match self.body.kind {
            BodyKind::None => (ResolvedBody::None, None),
            BodyKind::Json => (
                ResolvedBody::Bytes(sub(&self.body.text).into_bytes()),
                Some("application/json"),
            ),
            BodyKind::Text => (
                ResolvedBody::Bytes(sub(&self.body.text).into_bytes()),
                Some("text/plain; charset=utf-8"),
            ),
            BodyKind::Form => {
                let fields: Vec<String> = self
                    .body
                    .form
                    .iter()
                    .filter(|f| f.enabled && !f.key.trim().is_empty())
                    .map(|f| {
                        format!(
                            "{}={}",
                            percent_encode(&sub(f.key.trim()), true),
                            percent_encode(&sub(&f.value), true)
                        )
                    })
                    .collect();
                (
                    ResolvedBody::Bytes(fields.join("&").into_bytes()),
                    Some("application/x-www-form-urlencoded"),
                )
            }
            BodyKind::File => (
                ResolvedBody::File(sub(self.body.file.trim())),
                Some("application/octet-stream"),
            ),
        };
        if let Some(kind) = content_type {
            if !has_header(&headers, "Content-Type") {
                headers.push(("Content-Type".into(), kind.into()));
            }
        }
        if !has_header(&headers, "User-Agent") {
            headers.push((
                "User-Agent".into(),
                format!("SimpleEdit/{}", env!("CARGO_PKG_VERSION")),
            ));
        }
        if !has_header(&headers, "Accept") {
            headers.push(("Accept".into(), "*/*".into()));
        }
        Resolved {
            method: self.method,
            url,
            headers,
            body,
            settings: self.settings,
            missing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(list: &[(&str, &str)]) -> HashMap<String, String> {
        list.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn variables_are_filled_in_and_the_unknown_ones_are_noted() {
        let mut missing = Vec::new();
        let v = vars(&[("host", "api.test"), ("id", "7")]);
        assert_eq!(
            substitute("https://{{host}}/users/{{ id }}", &v, &mut missing),
            "https://api.test/users/7"
        );
        assert!(missing.is_empty());
        assert_eq!(
            substitute("{{nope}} and {{nope}} and {{ other }}", &v, &mut missing),
            "{{nope}} and {{nope}} and {{ other }}"
        );
        assert_eq!(missing, vec!["nope", "other"]);
        // An open brace with no end is plain text.
        assert_eq!(substitute("a {{ b", &v, &mut Vec::new()), "a {{ b");
        assert_eq!(substitute("", &v, &mut Vec::new()), "");
    }

    #[test]
    fn text_is_made_safe_for_an_address() {
        assert_eq!(percent_encode("a b&c=d/é", false), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(percent_encode("a b", true), "a+b");
        assert_eq!(percent_encode("A-z_0.9~", false), "A-z_0.9~");
    }

    #[test]
    fn base64_is_the_standard_one() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
    }

    #[test]
    fn the_parameters_go_after_the_address_before_a_fragment() {
        let q = vec![("a".to_string(), "1 2".to_string())];
        assert_eq!(with_query("http://x/p", &q), "http://x/p?a=1%202");
        assert_eq!(with_query("http://x/p?z=1", &q), "http://x/p?z=1&a=1%202");
        assert_eq!(with_query("http://x/p?", &q), "http://x/p?a=1%202");
        assert_eq!(with_query("http://x/p#top", &q), "http://x/p?a=1%202#top");
        assert_eq!(with_query("http://x/p", &[]), "http://x/p");
    }

    #[test]
    fn a_request_is_made_ready_with_its_variables_headers_and_sign_in() {
        let mut r = Request::new("one");
        r.method = Method::Post;
        r.url = "{{host}}/items".into();
        r.params = vec![Pair::new("q", "{{term}}"), {
            let mut off = Pair::new("skip", "me");
            off.enabled = false;
            off
        }];
        r.headers = vec![Pair::new("X-Id", "{{id}}")];
        r.body = Body {
            kind: BodyKind::Json,
            text: "{\"n\": \"{{id}}\"}".into(),
            ..Body::default()
        };
        r.auth = Auth {
            kind: AuthKind::Bearer,
            token: "{{token}}".into(),
            ..Auth::default()
        };
        let v = vars(&[
            ("host", "localhost:3000"),
            ("term", "a b"),
            ("id", "9"),
            ("token", "tk"),
        ]);
        let out = r.resolve(&v);
        assert_eq!(out.method, Method::Post);
        // No scheme: http. The parameter that is off is left out.
        assert_eq!(out.url, "http://localhost:3000/items?q=a%20b");
        assert_eq!(out.body, ResolvedBody::Bytes(b"{\"n\": \"9\"}".to_vec()));
        let get = |name: &str| {
            out.headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("X-Id"), Some("9"));
        assert_eq!(get("Authorization"), Some("Bearer tk"));
        assert_eq!(get("Content-Type"), Some("application/json"));
        assert!(get("User-Agent").unwrap().starts_with("SimpleEdit/"));
        assert_eq!(get("Accept"), Some("*/*"));
        assert!(out.missing.is_empty());
    }

    #[test]
    fn what_the_user_wrote_in_a_header_wins_over_the_defaults() {
        let mut r = Request::new("");
        r.url = "https://x".into();
        r.headers = vec![
            Pair::new("authorization", "Token abc"),
            Pair::new("content-type", "application/vnd.api+json"),
            Pair::new("User-Agent", "mine"),
        ];
        r.body = Body {
            kind: BodyKind::Json,
            text: "{}".into(),
            ..Body::default()
        };
        r.auth = Auth {
            kind: AuthKind::Basic,
            username: "u".into(),
            password: "p".into(),
            ..Auth::default()
        };
        let out = r.resolve(&HashMap::new());
        let count = |name: &str| {
            out.headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case(name))
                .count()
        };
        assert_eq!(
            (
                count("authorization"),
                count("content-type"),
                count("user-agent")
            ),
            (1, 1, 1)
        );
        assert_eq!(out.headers[0].1, "Token abc");
    }

    #[test]
    fn basic_and_api_key_sign_ins_are_made() {
        let mut r = Request::new("");
        r.url = "https://x/p".into();
        r.auth = Auth {
            kind: AuthKind::Basic,
            username: "user".into(),
            password: "pass".into(),
            ..Auth::default()
        };
        let out = r.resolve(&HashMap::new());
        assert!(out.headers.contains(&(
            "Authorization".to_string(),
            "Basic dXNlcjpwYXNz".to_string()
        )));
        r.auth = Auth {
            kind: AuthKind::ApiKey,
            key_name: "api_key".into(),
            key_value: "k 1".into(),
            key_in_header: false,
            ..Auth::default()
        };
        assert_eq!(r.resolve(&HashMap::new()).url, "https://x/p?api_key=k%201");
        r.auth.key_in_header = true;
        let out = r.resolve(&HashMap::new());
        assert!(out
            .headers
            .contains(&("api_key".to_string(), "k 1".to_string())));
        assert_eq!(out.url, "https://x/p");
    }

    #[test]
    fn a_form_is_encoded_and_a_file_is_left_to_be_read() {
        let mut r = Request::new("");
        r.url = "https://x".into();
        r.body = Body {
            kind: BodyKind::Form,
            form: vec![Pair::new("a b", "c&d"), Pair::new("e", "é")],
            ..Body::default()
        };
        let out = r.resolve(&HashMap::new());
        assert_eq!(
            out.body,
            ResolvedBody::Bytes(b"a+b=c%26d&e=%C3%A9".to_vec())
        );
        assert!(out.headers.contains(&(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string()
        )));
        r.body = Body {
            kind: BodyKind::File,
            file: "/tmp/{{name}}.bin".into(),
            ..Body::default()
        };
        let out = r.resolve(&vars(&[("name", "x")]));
        assert_eq!(out.body, ResolvedBody::File("/tmp/x.bin".into()));
    }

    #[test]
    fn a_collection_finds_renames_moves_and_counts() {
        let request = |id, name: &str| {
            Item::Request(Request {
                id,
                name: name.into(),
                ..Request::default()
            })
        };
        let mut c = Collection {
            id: 1,
            name: "api".into(),
            items: vec![
                request(2, "list"),
                Item::Folder(Folder {
                    id: 3,
                    name: "users".into(),
                    items: vec![
                        request(4, "get"),
                        Item::Folder(Folder {
                            id: 5,
                            name: "deep".into(),
                            items: vec![request(6, "x")],
                        }),
                    ],
                }),
            ],
        };
        assert_eq!(c.count(), 3);
        assert_eq!(c.max_id(), 6);
        assert_eq!(c.request(6).map(|r| r.name.as_str()), Some("x"));
        assert!(c.rename(5, "deeper") && c.rename(4, "fetch"));
        assert_eq!(c.request(4).unwrap().name, "fetch");
        let taken = c.remove(4).unwrap();
        assert_eq!(taken.id(), 4);
        c.items_of(5).unwrap().push(taken);
        assert_eq!(c.items_of(5).unwrap().len(), 2);
        assert!(c.items_of(99).is_none());
        assert!(c.remove(99).is_none());
        // A copy comes right after the request it copies, in its folder.
        assert_eq!(c.duplicate(6, 20, "copy"), Some(20));
        assert_eq!(c.request(20).unwrap().name, "x copy");
        assert_eq!(
            c.items_of(5)
                .unwrap()
                .iter()
                .map(Item::id)
                .collect::<Vec<_>>(),
            vec![6, 20, 4]
        );
        assert_eq!(c.duplicate(99, 21, "copy"), None);
        assert!(c.remove(20).is_some());
        c.request_mut(2).unwrap().url = "https://x".into();
        assert_eq!(c.request(2).unwrap().url, "https://x");
        // Items without an id are numbered after the last one.
        c.items.push(request(0, "new"));
        let mut next = c.max_id();
        c.number(&mut next);
        assert_eq!(next, 7);
        assert_eq!(c.items.last().unwrap().id(), 7);
    }

    #[test]
    fn an_environment_gives_the_variables_that_are_on() {
        let env = Environment {
            id: 1,
            name: "dev".into(),
            vars: vec![
                Variable {
                    key: "host".into(),
                    value: "a".into(),
                    ..Variable::default()
                },
                Variable {
                    enabled: false,
                    key: "off".into(),
                    value: "b".into(),
                    ..Variable::default()
                },
                Variable {
                    key: " ".into(),
                    ..Variable::default()
                },
            ],
        };
        assert_eq!(env.values(), vars(&[("host", "a")]));
    }

    #[test]
    fn a_request_survives_its_file_and_older_files_still_load() {
        let mut r = Request::new("n");
        r.method = Method::Patch;
        r.body.kind = BodyKind::Form;
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), r);
        // A file with only a few fields.
        let old: Request = serde_json::from_str(r#"{"name":"a","url":"x"}"#).unwrap();
        assert_eq!(old.method, Method::Get);
        assert!(old.settings.follow_redirects && old.settings.verify_tls);
        assert_eq!(Method::parse(" post "), Some(Method::Post));
        assert_eq!(Method::parse("nope"), None);
    }
}
