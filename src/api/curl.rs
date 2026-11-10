//! `curl` commands: read one into a request, write a request as one.

use super::model::{Auth, AuthKind, Body, BodyKind, Method, Pair, Request, Resolved, ResolvedBody};

/// Cuts a command line into words the way a shell does: quotes, escapes, and
/// a `\` at the end of a line to go on.
pub fn words(line: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                // A line that goes on.
                Some('\n') => {}
                Some('\r') => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                Some(other) => {
                    word.push(other);
                    started = true;
                }
                None => {}
            },
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(other) => word.push(other),
                        None => return Err("A quote is not closed.".to_string()),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(e @ ('"' | '\\' | '$' | '`')) => word.push(e),
                            Some('\n') => {}
                            Some(other) => {
                                word.push('\\');
                                word.push(other);
                            }
                            None => return Err("A quote is not closed.".to_string()),
                        },
                        Some(other) => word.push(other),
                        None => return Err("A quote is not closed.".to_string()),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                // $'...': the escapes of C.
                chars.next();
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => word.push('\n'),
                            Some('r') => word.push('\r'),
                            Some('t') => word.push('\t'),
                            Some(other) => word.push(other),
                            None => return Err("A quote is not closed.".to_string()),
                        },
                        Some(other) => word.push(other),
                        None => return Err("A quote is not closed.".to_string()),
                    }
                }
            }
            c if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            other => {
                word.push(other);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    Ok(out)
}

/// `%41` and `+` read back.
pub fn percent_decode(text: &str, plus_is_space: bool) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' if plus_is_space => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The `a=1&b=2` of an address or a form, as lines of a table.
pub fn pairs_of(query: &str, plus_is_space: bool) -> Vec<Pair> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            Pair::new(
                &percent_decode(k, plus_is_space),
                &percent_decode(v, plus_is_space),
            )
        })
        .collect()
}

/// Which options of curl take a value that follows, and mean nothing here.
const SKIPPED_WITH_VALUE: [&str; 14] = [
    "-o",
    "--output",
    "-w",
    "--write-out",
    "--connect-timeout",
    "--retry",
    "--proxy",
    "-x",
    "--cacert",
    "--cert",
    "--key",
    "-E",
    "--resolve",
    "--interface",
];

/// Reads a `curl` command into a request.
pub fn parse(command: &str) -> Result<Request, String> {
    let mut args = words(command)?.into_iter().peekable();
    if args.peek().is_some_and(|w| w == "curl") {
        args.next();
    }
    let mut request = Request::default();
    request.settings.follow_redirects = false;
    let mut url: Option<String> = None;
    let mut method: Option<Method> = None;
    let mut data: Vec<String> = Vec::new();
    let mut urlencoded: Vec<Pair> = Vec::new();
    let mut file_body: Option<String> = None;
    let mut get = false;
    let mut json_flag = false;
    while let Some(arg) = args.next() {
        // `--long=value` is `--long value`.
        let (flag, attached) = match arg.strip_prefix("--").and_then(|_| arg.split_once('=')) {
            Some((f, v)) => (f.to_string(), Some(v.to_string())),
            None => (arg.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            match attached.clone() {
                Some(v) => Ok(v),
                None => args.next().ok_or_else(|| format!("{name} needs a value.")),
            }
        };
        match flag.as_str() {
            "-X" | "--request" => {
                let text = value(&flag)?;
                method = Some(
                    Method::parse(&text)
                        .ok_or_else(|| format!("The method {text} is not known."))?,
                );
            }
            "-H" | "--header" => {
                let line = value(&flag)?;
                if let Some((k, v)) = line.split_once(':') {
                    request.headers.push(Pair::new(k.trim(), v.trim()));
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" => {
                let text = value(&flag)?;
                match text.strip_prefix('@') {
                    Some(path) if flag != "--data-raw" => file_body = Some(path.to_string()),
                    _ => data.push(text),
                }
            }
            "--data-urlencode" => {
                let text = value(&flag)?;
                let (name, v) = text.split_once('=').unwrap_or(("", &text));
                urlencoded.push(Pair::new(name, v));
            }
            "--json" => {
                json_flag = true;
                data.push(value(&flag)?);
            }
            "-u" | "--user" => {
                let text = value(&flag)?;
                let (user, password) = text.split_once(':').unwrap_or((&text, ""));
                request.auth = Auth {
                    kind: AuthKind::Basic,
                    username: user.to_string(),
                    password: password.to_string(),
                    ..Auth::default()
                };
            }
            "-A" | "--user-agent" => {
                let v = value(&flag)?;
                request.headers.push(Pair::new("User-Agent", &v));
            }
            "-e" | "--referer" => {
                let v = value(&flag)?;
                request.headers.push(Pair::new("Referer", &v));
            }
            "-b" | "--cookie" => {
                let v = value(&flag)?;
                request.headers.push(Pair::new("Cookie", &v));
            }
            "-m" | "--max-time" => {
                request.settings.timeout_secs = value(&flag)?.parse().unwrap_or(30);
            }
            "--url" => url = Some(value(&flag)?),
            "-G" | "--get" => get = true,
            "-I" | "--head" => method = Some(Method::Head),
            "-L" | "--location" => request.settings.follow_redirects = true,
            "-k" | "--insecure" => request.settings.verify_tls = false,
            "-F" | "--form" | "--form-string" => {
                return Err("Forms with files (multipart) are not supported yet.".to_string());
            }
            f if SKIPPED_WITH_VALUE.contains(&f) => {
                value(f)?;
            }
            f if f.starts_with("--") => {}
            f if f.starts_with('-') && f.len() > 1 => {
                // Short flags written together: `-sSL`.
                let letters: Vec<char> = f.chars().skip(1).collect();
                for letter in &letters {
                    match letter {
                        'L' => request.settings.follow_redirects = true,
                        'k' => request.settings.verify_tls = false,
                        'G' => get = true,
                        'I' => method = Some(Method::Head),
                        _ => {}
                    }
                }
            }
            _ => {
                if url.is_none() {
                    url = Some(arg.clone());
                }
            }
        }
    }
    let url = url.ok_or_else(|| "There is no address in the command.".to_string())?;
    // The parameters of the address are lines of the table.
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b.to_string(), Some(q.to_string())),
        None => (url.clone(), None),
    };
    request.url = base;
    if let Some(query) = query {
        let (query, fragment) = match query.split_once('#') {
            Some((q, f)) => (q.to_string(), Some(f.to_string())),
            None => (query, None),
        };
        request.params = pairs_of(&query, false);
        if let Some(f) = fragment {
            request.url.push('#');
            request.url.push_str(&f);
        }
    }
    let has_body = !data.is_empty() || !urlencoded.is_empty() || file_body.is_some();
    if get {
        // `-G`: what is sent as data goes in the address.
        for text in &data {
            request.params.extend(pairs_of(text, false));
        }
        request.params.append(&mut urlencoded);
        data.clear();
    }
    request.method = method.unwrap_or(if has_body && !get {
        Method::Post
    } else {
        Method::Get
    });
    // The sign-in that a header says.
    if let Some(at) = request
        .headers
        .iter()
        .position(|h| h.key.eq_ignore_ascii_case("authorization"))
    {
        let value = request.headers[at].value.clone();
        if let Some(token) = value.strip_prefix("Bearer ") {
            request.auth = Auth {
                kind: AuthKind::Bearer,
                token: token.trim().to_string(),
                ..Auth::default()
            };
            request.headers.remove(at);
        }
    }
    let content_type = request
        .headers
        .iter()
        .find(|h| h.key.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.to_lowercase())
        .unwrap_or_default();
    if file_body.is_some() {
        // A command from elsewhere does not get to pick a file of this
        // machine to send: the file is chosen again, by hand.
        request.body = Body {
            kind: BodyKind::File,
            file: String::new(),
            ..Body::default()
        };
    } else if !urlencoded.is_empty()
        || (!data.is_empty() && content_type.contains("x-www-form-urlencoded"))
    {
        let mut form = urlencoded;
        for text in &data {
            form.extend(pairs_of(text, true));
        }
        request.body = Body {
            kind: BodyKind::Form,
            form,
            ..Body::default()
        };
        request.headers.retain(|h| {
            !(h.key.eq_ignore_ascii_case("content-type")
                && h.value.to_lowercase().contains("x-www-form-urlencoded"))
        });
    } else if !data.is_empty() {
        let text = data.join("&");
        let looks_json = content_type.contains("json")
            || json_flag
            || (text.trim_start().starts_with(['{', '['])
                && serde_json::from_str::<serde_json::Value>(&text).is_ok());
        request.body = Body {
            kind: if looks_json {
                BodyKind::Json
            } else {
                BodyKind::Text
            },
            text,
            ..Body::default()
        };
        if looks_json {
            // Said by the kind of the body already.
            request.headers.retain(|h| {
                !(h.key.eq_ignore_ascii_case("content-type")
                    && h.value.trim().eq_ignore_ascii_case("application/json"))
            });
        } else {
            request.headers.retain(|h| {
                !(h.key.eq_ignore_ascii_case("content-type")
                    && h.value.to_lowercase().starts_with("text/plain"))
            });
        }
    }
    Ok(request)
}

/// A word safe to paste in a shell.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// A request as a `curl` command, as it is sent.
pub fn export(request: &Resolved) -> String {
    let mut head = String::from("curl");
    match request.method {
        Method::Get => {}
        Method::Head => head.push_str(" -I"),
        other => head.push_str(&format!(" -X {}", other.as_str())),
    }
    head.push(' ');
    head.push_str(&quote(&request.url));
    let mut parts: Vec<String> = vec![head];
    if request.settings.follow_redirects {
        parts.push("-L".to_string());
    }
    if !request.settings.verify_tls {
        parts.push("--insecure".to_string());
    }
    if request.settings.timeout_secs > 0 {
        parts.push(format!("--max-time {}", request.settings.timeout_secs));
    }
    for (k, v) in &request.headers {
        // What the app adds on its own is not part of the command.
        let default = (k.eq_ignore_ascii_case("user-agent") && v.starts_with("SimpleEdit/"))
            || (k.eq_ignore_ascii_case("accept") && v == "*/*");
        if !default {
            parts.push(format!("-H {}", quote(&format!("{k}: {v}"))));
        }
    }
    match &request.body {
        ResolvedBody::None => {}
        ResolvedBody::Bytes(bytes) => {
            parts.push(format!(
                "--data-raw {}",
                quote(&String::from_utf8_lossy(bytes))
            ));
        }
        ResolvedBody::File(path) => {
            parts.push(format!("--data-binary {}", quote(&format!("@{path}"))))
        }
    }
    parts.join(" \\\n  ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::model::Settings;
    use std::collections::HashMap;

    #[test]
    fn a_command_line_is_cut_into_words_like_a_shell_does() {
        assert_eq!(
            words("curl -X POST 'http://x/a b' -H \"A: \\\"q\\\"\" plain\\ word").unwrap(),
            vec![
                "curl",
                "-X",
                "POST",
                "http://x/a b",
                "-H",
                "A: \"q\"",
                "plain word"
            ]
        );
        // A line that goes on, and a quote of the C kind.
        assert_eq!(
            words("curl \\\n  -d $'a\\nb' \\\n  url").unwrap(),
            vec!["curl", "-d", "a\nb", "url"]
        );
        assert_eq!(words("''").unwrap(), vec![""]);
        assert!(words("curl 'open").is_err());
        assert!(words("   ").unwrap().is_empty());
    }

    #[test]
    fn percent_signs_and_pluses_are_read_back() {
        assert_eq!(percent_decode("a%20b+c%2Fd", false), "a b+c/d");
        assert_eq!(percent_decode("a+b", true), "a b");
        assert_eq!(percent_decode("100%", false), "100%");
        assert_eq!(percent_decode("%zz", false), "%zz");
        assert_eq!(percent_decode("caf%C3%A9", false), "café");
        assert_eq!(percent_decode("a%2", false), "a%2");
    }

    #[test]
    fn a_json_post_is_read_with_its_header_its_body_and_its_sign_in() {
        let r = parse(
            "curl -X POST 'https://api.test/items?page=2&q=a%20b' \\\n  -H 'Content-Type: application/json' \\\n  -H 'Authorization: Bearer tok123' \\\n  -H 'X-Id: 7' \\\n  -d '{\"name\":\"x\"}'",
        )
        .unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.url, "https://api.test/items");
        assert_eq!(
            r.params,
            vec![Pair::new("page", "2"), Pair::new("q", "a b")]
        );
        assert_eq!(r.auth.kind, AuthKind::Bearer);
        assert_eq!(r.auth.token, "tok123");
        assert_eq!(r.body.kind, BodyKind::Json);
        assert_eq!(r.body.text, "{\"name\":\"x\"}");
        // The type is said by the body; only the others stay.
        assert_eq!(r.headers, vec![Pair::new("X-Id", "7")]);
        // curl does not follow redirects unless asked.
        assert!(!r.settings.follow_redirects);
    }

    #[test]
    fn the_options_of_curl_are_read() {
        let r = parse("curl -sSL -k -u me:secret -A agent -m 5 https://x/y").unwrap();
        assert!(r.settings.follow_redirects && !r.settings.verify_tls);
        assert_eq!(r.settings.timeout_secs, 5);
        assert_eq!(
            (
                r.auth.kind,
                r.auth.username.as_str(),
                r.auth.password.as_str()
            ),
            (AuthKind::Basic, "me", "secret")
        );
        assert_eq!(r.headers, vec![Pair::new("User-Agent", "agent")]);
        assert_eq!(r.method, Method::Get);
        assert_eq!(parse("curl -I https://x").unwrap().method, Method::Head);
        assert_eq!(
            parse("curl --request=PUT --url=https://x").unwrap().method,
            Method::Put
        );
        // Options that mean nothing here are skipped, with their value.
        let r = parse("curl -o out.txt --compressed -s https://x/z").unwrap();
        assert_eq!(r.url, "https://x/z");
    }

    #[test]
    fn data_makes_a_post_a_form_or_a_get_with_parameters() {
        let r = parse("curl https://x -d a=1 -d 'b=two words'").unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.body.kind, BodyKind::Text);
        assert_eq!(r.body.text, "a=1&b=two words");
        let r = parse("curl https://x -H 'Content-Type: application/x-www-form-urlencoded' -d 'a=1&b=two+words'").unwrap();
        assert_eq!(r.body.kind, BodyKind::Form);
        assert_eq!(
            r.body.form,
            vec![Pair::new("a", "1"), Pair::new("b", "two words")]
        );
        assert!(r.headers.is_empty());
        let r = parse("curl -G https://x -d a=1 --data-urlencode 'q=x y'").unwrap();
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.params, vec![Pair::new("a", "1"), Pair::new("q", "x y")]);
        assert_eq!(r.body.kind, BodyKind::None);
        let r = parse("curl https://x --data-binary @/tmp/p.bin").unwrap();
        assert_eq!(
            (r.body.kind, r.body.file.as_str()),
            (BodyKind::File, ""),
            "the path of an imported command is not kept"
        );
        // The JSON is told by what it is, without a header.
        let r = parse("curl https://x -d '[1,2]'").unwrap();
        assert_eq!(r.body.kind, BodyKind::Json);
        let r = parse("curl https://x --json '{\"a\":1}'").unwrap();
        assert_eq!(r.body.kind, BodyKind::Json);
    }

    #[test]
    fn what_cannot_be_read_is_said() {
        assert!(parse("curl -X NOPE https://x")
            .unwrap_err()
            .contains("NOPE"));
        assert!(parse("curl -H").unwrap_err().contains("needs a value"));
        assert!(parse("curl -X POST").unwrap_err().contains("no address"));
        assert!(parse("curl https://x -F a=@f")
            .unwrap_err()
            .contains("multipart"));
        // The word curl is not needed.
        assert_eq!(parse("https://x/only").unwrap().url, "https://x/only");
    }

    #[test]
    fn a_request_is_written_as_a_command_that_reads_back_the_same() {
        let mut r = Request::new("");
        r.method = Method::Post;
        r.url = "https://api.test/items".into();
        r.params = vec![Pair::new("a", "x y")];
        r.headers = vec![Pair::new("X-Id", "it's")];
        r.body = Body {
            kind: BodyKind::Json,
            text: "{\"k\": \"it's\"}".into(),
            ..Body::default()
        };
        r.settings = Settings {
            timeout_secs: 10,
            follow_redirects: true,
            verify_tls: false,
        };
        let command = export(&r.resolve(&HashMap::new()));
        assert!(command.starts_with("curl -X POST 'https://api.test/items?a=x%20y' \\\n  -L"));
        assert!(command.contains("--insecure"));
        assert!(command.contains("--max-time 10"));
        assert!(command.contains("-H 'X-Id: it'\\''s'"));
        assert!(command.contains("-H 'Content-Type: application/json'"));
        // The headers the app adds on its own are not in it.
        assert!(!command.contains("User-Agent") && !command.contains("Accept"));
        // And it reads back as the same request.
        let back = parse(&command).unwrap();
        assert_eq!(back.method, Method::Post);
        assert_eq!(back.url, "https://api.test/items");
        assert_eq!(back.params, vec![Pair::new("a", "x y")]);
        assert_eq!(back.body.kind, BodyKind::Json);
        assert_eq!(back.body.text, "{\"k\": \"it's\"}");
        assert_eq!(back.headers, vec![Pair::new("X-Id", "it's")]);
        assert_eq!(
            (
                back.settings.timeout_secs,
                back.settings.follow_redirects,
                back.settings.verify_tls
            ),
            (10, true, false)
        );
        // GET and HEAD, a file.
        let mut g = Request::new("");
        g.url = "https://x".into();
        assert!(export(&g.resolve(&HashMap::new())).starts_with("curl 'https://x'"));
        g.method = Method::Head;
        assert!(export(&g.resolve(&HashMap::new())).starts_with("curl -I "));
        g.method = Method::Put;
        g.body = Body {
            kind: BodyKind::File,
            file: "/tmp/a b".into(),
            ..Body::default()
        };
        assert!(export(&g.resolve(&HashMap::new())).contains("--data-binary '@/tmp/a b'"));
    }
}
