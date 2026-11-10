//! Collections and environments in the format of Postman (v2.1): read what a
//! person already has, write what they made here.

use serde_json::{json, Map, Value};

use super::curl::pairs_of;
use super::model::{
    Auth, AuthKind, Body, BodyKind, Collection, Environment, Folder, Item, Method, Pair, Request,
    Variable,
};

pub const SCHEMA: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

/// What was read, and what could not be kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Imported {
    pub collection: Collection,
    /// The variables the collection carried.
    pub variables: Vec<Variable>,
    /// Things that were left out or changed, to tell the user.
    pub warnings: Vec<String>,
}

/// The text of a field that may be a string or missing.
fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn key_values(value: &Value) -> Vec<Pair> {
    value
        .as_array()
        .map(|list| {
            list.iter()
                .map(|p| Pair {
                    enabled: !p["disabled"].as_bool().unwrap_or(false),
                    key: text(&p["key"]),
                    value: text(&p["value"]),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The entries of an auth block (`[{key, value}]`) by key.
fn entry(list: &Value, key: &str) -> String {
    list.as_array()
        .and_then(|l| l.iter().find(|e| e["key"] == key))
        .map(|e| text(&e["value"]))
        .unwrap_or_default()
}

fn read_auth(auth: &Value, warnings: &mut Vec<String>, name: &str) -> Option<Auth> {
    let kind = auth["type"].as_str()?;
    Some(match kind {
        "noauth" => Auth::default(),
        "bearer" => Auth {
            kind: AuthKind::Bearer,
            token: entry(&auth["bearer"], "token"),
            ..Auth::default()
        },
        "basic" => Auth {
            kind: AuthKind::Basic,
            username: entry(&auth["basic"], "username"),
            password: entry(&auth["basic"], "password"),
            ..Auth::default()
        },
        "apikey" => Auth {
            kind: AuthKind::ApiKey,
            key_name: entry(&auth["apikey"], "key"),
            key_value: entry(&auth["apikey"], "value"),
            key_in_header: entry(&auth["apikey"], "in") != "query",
            ..Auth::default()
        },
        other => {
            warnings.push(format!(
                "{name}: the sign-in \"{other}\" is not supported, it was left out."
            ));
            Auth::default()
        }
    })
}

fn read_request(item: &Value, inherited: &Option<Auth>, warnings: &mut Vec<String>) -> Request {
    let name = text(&item["name"]);
    let source = &item["request"];
    let mut request = Request::new(&name);
    // A request written as just an address.
    if let Value::String(url) = source {
        request.url = url.clone();
        return request;
    }
    request.method = Method::parse(source["method"].as_str().unwrap_or("GET")).unwrap_or_default();
    request.headers = key_values(&source["header"]);
    // The address: its raw text, without the query that has a table of its own.
    let url = &source["url"];
    let (raw, query): (String, Vec<Pair>) = match url {
        Value::String(s) => (s.clone(), Vec::new()),
        Value::Object(_) => {
            let raw = match url["raw"].as_str() {
                Some(r) => r.to_string(),
                None => {
                    let host = url["host"]
                        .as_array()
                        .map(|h| h.iter().map(text).collect::<Vec<_>>().join("."))
                        .unwrap_or_else(|| text(&url["host"]));
                    let path = url["path"]
                        .as_array()
                        .map(|p| p.iter().map(text).collect::<Vec<_>>().join("/"))
                        .unwrap_or_else(|| text(&url["path"]));
                    let scheme = url["protocol"].as_str().unwrap_or("https");
                    format!("{scheme}://{host}/{path}")
                }
            };
            (raw, key_values(&url["query"]))
        }
        _ => (String::new(), Vec::new()),
    };
    let (base, raw_query) = match raw.split_once('?') {
        Some((b, q)) => (b.to_string(), Some(q.to_string())),
        None => (raw, None),
    };
    request.url = base;
    request.params = if !query.is_empty() {
        query
    } else {
        raw_query.map(|q| pairs_of(&q, false)).unwrap_or_default()
    };
    // The body.
    let body = &source["body"];
    match body["mode"].as_str() {
        Some("raw") => {
            let raw = text(&body["raw"]);
            let language = body["options"]["raw"]["language"].as_str().unwrap_or("");
            let said_json = request.headers.iter().any(|h| {
                h.key.eq_ignore_ascii_case("content-type")
                    && h.value.to_lowercase().contains("json")
            });
            let json = language == "json"
                || said_json
                || (raw.trim_start().starts_with(['{', '['])
                    && serde_json::from_str::<Value>(&raw).is_ok());
            request.body = Body {
                kind: if json { BodyKind::Json } else { BodyKind::Text },
                text: raw,
                ..Body::default()
            };
        }
        Some("urlencoded") => {
            request.body = Body {
                kind: BodyKind::Form,
                form: key_values(&body["urlencoded"]),
                ..Body::default()
            };
        }
        Some("formdata") => {
            // Multipart is not sent here: the text fields are kept as a form.
            let fields: Vec<Pair> = body["formdata"]
                .as_array()
                .map(|l| {
                    l.iter()
                        .filter(|f| f["type"].as_str() != Some("file"))
                        .map(|f| Pair {
                            enabled: !f["disabled"].as_bool().unwrap_or(false),
                            key: text(&f["key"]),
                            value: text(&f["value"]),
                        })
                        .collect()
                })
                .unwrap_or_default();
            warnings.push(format!(
                "{name}: a multipart form is kept as a simple form (files are left out)."
            ));
            request.body = Body {
                kind: BodyKind::Form,
                form: fields,
                ..Body::default()
            };
        }
        Some("file") => {
            // A collection from elsewhere does not get to pick a file of this
            // machine to send: it is chosen again, by hand.
            let src = text(&body["file"]["src"]);
            if !src.is_empty() {
                warnings.push(format!(
                    "{name}: the file {src} of the body was not kept, choose it again."
                ));
            }
            request.body = Body {
                kind: BodyKind::File,
                file: String::new(),
                ..Body::default()
            };
        }
        Some("graphql") => {
            let payload = json!({
                "query": text(&body["graphql"]["query"]),
                "variables": serde_json::from_str::<Value>(&text(&body["graphql"]["variables"])).unwrap_or(json!({})),
            });
            request.body = Body {
                kind: BodyKind::Json,
                text: serde_json::to_string_pretty(&payload).unwrap_or_default(),
                ..Body::default()
            };
        }
        _ => {}
    }
    // The sign-in: its own, or the one of the folder or the collection.
    request.auth = read_auth(&source["auth"], warnings, &name)
        .or_else(|| inherited.clone())
        .unwrap_or_default();
    request
}

fn read_items(items: &Value, inherited: &Option<Auth>, warnings: &mut Vec<String>) -> Vec<Item> {
    items
        .as_array()
        .map(|list| {
            list.iter()
                .map(|item| {
                    if item["item"].is_array() {
                        let own = read_auth(&item["auth"], warnings, &text(&item["name"]))
                            .or_else(|| inherited.clone());
                        Item::Folder(Folder {
                            id: 0,
                            name: text(&item["name"]),
                            items: read_items(&item["item"], &own, warnings),
                        })
                    } else {
                        Item::Request(read_request(item, inherited, warnings))
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_variables(list: &Value) -> Vec<Variable> {
    list.as_array()
        .map(|l| {
            l.iter()
                .map(|v| Variable {
                    enabled: v["enabled"]
                        .as_bool()
                        .unwrap_or(!v["disabled"].as_bool().unwrap_or(false)),
                    key: text(&v["key"]),
                    value: text(&v["value"]),
                    secret: v["type"].as_str() == Some("secret"),
                })
                .filter(|v| !v.key.trim().is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// What a file is: a collection, or an environment.
pub fn is_environment(json: &str) -> bool {
    serde_json::from_str::<Value>(json)
        .map(|v| v["values"].is_array() && !v["item"].is_array())
        .unwrap_or(false)
}

/// Reads a collection of Postman.
pub fn import_collection(json: &str) -> Result<Imported, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("Not a JSON file: {e}"))?;
    if !value["item"].is_array() {
        return Err("This is not a Postman collection (it has no \"item\").".to_string());
    }
    let schema = value["info"]["schema"].as_str().unwrap_or("");
    let mut warnings = Vec::new();
    if schema.contains("v1.0") || schema.contains("v2.0") {
        warnings.push(format!(
            "The collection is in an older format ({schema}): some parts may be missing."
        ));
    }
    let auth = read_auth(&value["auth"], &mut warnings, "collection");
    let items = read_items(&value["item"], &auth, &mut warnings);
    Ok(Imported {
        collection: Collection {
            id: 0,
            name: {
                let n = text(&value["info"]["name"]);
                if n.is_empty() {
                    "Imported".to_string()
                } else {
                    n
                }
            },
            items,
        },
        variables: read_variables(&value["variable"]),
        warnings,
    })
}

/// Reads an environment of Postman.
pub fn import_environment(json: &str) -> Result<Environment, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("Not a JSON file: {e}"))?;
    if !value["values"].is_array() {
        return Err("This is not a Postman environment (it has no \"values\").".to_string());
    }
    Ok(Environment {
        id: 0,
        name: {
            let n = text(&value["name"]);
            if n.is_empty() {
                "Imported".to_string()
            } else {
                n
            }
        },
        vars: read_variables(&value["values"]),
    })
}

// ------------------------------------------------------------------ writing

fn pairs_json(pairs: &[Pair]) -> Value {
    Value::Array(
        pairs
            .iter()
            .map(|p| {
                let mut o = Map::new();
                o.insert("key".into(), json!(p.key));
                o.insert("value".into(), json!(p.value));
                if !p.enabled {
                    o.insert("disabled".into(), json!(true));
                }
                Value::Object(o)
            })
            .collect(),
    )
}

fn auth_json(auth: &Auth) -> Option<Value> {
    let list = |entries: &[(&str, &str)]| {
        Value::Array(
            entries
                .iter()
                .map(|(k, v)| json!({"key": k, "value": v, "type": "string"}))
                .collect(),
        )
    };
    match auth.kind {
        AuthKind::None => None,
        AuthKind::Bearer => {
            Some(json!({"type": "bearer", "bearer": list(&[("token", &auth.token)])}))
        }
        AuthKind::Basic => Some(json!({"type": "basic", "basic": list(&[
            ("username", &auth.username), ("password", &auth.password)])})),
        AuthKind::ApiKey => Some(json!({"type": "apikey", "apikey": list(&[
            ("key", &auth.key_name),
            ("value", &auth.key_value),
            ("in", if auth.key_in_header { "header" } else { "query" })])})),
    }
}

fn request_json(request: &Request) -> Value {
    // The address as Postman keeps it: the raw text, and the query apart.
    let mut raw = request.url.clone();
    let enabled: Vec<&Pair> = request
        .params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .collect();
    if !enabled.is_empty() {
        let joiner = if raw.contains('?') { "&" } else { "?" };
        raw.push_str(joiner);
        raw.push_str(
            &enabled
                .iter()
                .map(|p| format!("{}={}", p.key, p.value))
                .collect::<Vec<_>>()
                .join("&"),
        );
    }
    let mut out = Map::new();
    out.insert("method".into(), json!(request.method.as_str()));
    out.insert("header".into(), pairs_json(&request.headers));
    out.insert(
        "url".into(),
        json!({"raw": raw, "query": pairs_json(&request.params)}),
    );
    match request.body.kind {
        BodyKind::None => {}
        BodyKind::Json => {
            out.insert(
                "body".into(),
                json!({"mode": "raw", "raw": request.body.text,
                       "options": {"raw": {"language": "json"}}}),
            );
        }
        BodyKind::Text => {
            out.insert(
                "body".into(),
                json!({"mode": "raw", "raw": request.body.text,
                       "options": {"raw": {"language": "text"}}}),
            );
        }
        BodyKind::Form => {
            out.insert(
                "body".into(),
                json!({"mode": "urlencoded", "urlencoded": pairs_json(&request.body.form)}),
            );
        }
        BodyKind::File => {
            out.insert(
                "body".into(),
                json!({"mode": "file", "file": {"src": request.body.file}}),
            );
        }
    }
    if let Some(auth) = auth_json(&request.auth) {
        out.insert("auth".into(), auth);
    }
    Value::Object(out)
}

fn items_json(items: &[Item]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| match item {
                Item::Folder(f) => json!({"name": f.name, "item": items_json(&f.items)}),
                Item::Request(r) => json!({"name": r.title(), "request": request_json(r)}),
            })
            .collect(),
    )
}

/// A collection as a file Postman reads.
pub fn export_collection(collection: &Collection) -> String {
    let value = json!({
        "info": {"name": collection.name, "schema": SCHEMA},
        "item": items_json(&collection.items),
    });
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

/// An environment as a file Postman reads. A secret goes out without its
/// value: the file is made to be shared.
pub fn export_environment(environment: &Environment, with_secrets: bool) -> String {
    let values: Vec<Value> = environment
        .vars
        .iter()
        .map(|v| {
            json!({
                "key": v.key,
                "value": if v.secret && !with_secrets { "" } else { v.value.as_str() },
                "type": if v.secret { "secret" } else { "default" },
                "enabled": v.enabled,
            })
        })
        .collect();
    let value = json!({
        "name": environment.name,
        "values": values,
        "_postman_variable_scope": "environment",
    });
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLLECTION: &str = r#"{
      "info": {"name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
      "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}"}]},
      "variable": [{"key": "base", "value": "https://shop.test"}],
      "item": [
        {"name": "List", "request": {
          "method": "GET",
          "header": [{"key": "Accept", "value": "application/json"}, {"key": "X-Off", "value": "1", "disabled": true}],
          "url": {"raw": "{{base}}/items?page=1&q=a%20b", "query": [{"key": "page", "value": "1"}, {"key": "q", "value": "a b", "disabled": true}]}
        }},
        {"name": "Users", "auth": {"type": "basic", "basic": [{"key": "username", "value": "u"}, {"key": "password", "value": "p"}]}, "item": [
          {"name": "Create", "request": {
            "method": "POST",
            "url": "{{base}}/users",
            "body": {"mode": "raw", "raw": "{\"a\": 1}", "options": {"raw": {"language": "json"}}}
          }},
          {"name": "Own sign-in", "request": {"method": "GET", "url": {"raw": "{{base}}/me"},
            "auth": {"type": "apikey", "apikey": [{"key": "key", "value": "k"}, {"key": "value", "value": "v"}, {"key": "in", "value": "query"}]}}},
          {"name": "Form", "request": {"method": "POST", "url": {"raw": "{{base}}/f"},
            "body": {"mode": "urlencoded", "urlencoded": [{"key": "a", "value": "1"}]}}},
          {"name": "Multipart", "request": {"method": "POST", "url": {"raw": "{{base}}/m"},
            "body": {"mode": "formdata", "formdata": [{"key": "t", "value": "x", "type": "text"}, {"key": "f", "type": "file", "src": "/tmp/a"}]}}},
          {"name": "Oauth", "request": {"method": "GET", "url": {"raw": "{{base}}/o"}, "auth": {"type": "oauth2"}}}
        ]},
        {"name": "Just an address", "request": "https://x.test/plain"}
      ]
    }"#;

    #[test]
    fn a_collection_of_postman_is_read_with_its_folders_and_requests() {
        let got = import_collection(COLLECTION).unwrap();
        assert_eq!(got.collection.name, "Shop");
        assert_eq!(
            got.variables,
            vec![Variable {
                enabled: true,
                key: "base".into(),
                value: "https://shop.test".into(),
                secret: false
            }]
        );
        let Item::Request(list) = &got.collection.items[0] else {
            panic!("a request")
        };
        assert_eq!(
            (list.method, list.url.as_str()),
            (Method::Get, "{{base}}/items")
        );
        // The table of the query wins over the raw text, disabled lines kept.
        assert_eq!(list.params.len(), 2);
        assert!(list.params[0].enabled && !list.params[1].enabled);
        assert_eq!(list.headers.len(), 2);
        assert!(!list.headers[1].enabled);
        // The sign-in of the collection goes down to a request that has none.
        assert_eq!(
            (list.auth.kind, list.auth.token.as_str()),
            (AuthKind::Bearer, "{{token}}")
        );
        let Item::Folder(users) = &got.collection.items[1] else {
            panic!("a folder")
        };
        assert_eq!(users.name, "Users");
        let Item::Request(create) = &users.items[0] else {
            panic!()
        };
        assert_eq!(
            (create.method, create.body.kind),
            (Method::Post, BodyKind::Json)
        );
        assert_eq!(create.body.text, "{\"a\": 1}");
        // The folder's own sign-in comes down; a request's own one wins.
        assert_eq!(create.auth.kind, AuthKind::Basic);
        let Item::Request(own) = &users.items[1] else {
            panic!()
        };
        assert_eq!(
            (own.auth.kind, own.auth.key_in_header),
            (AuthKind::ApiKey, false)
        );
        let Item::Request(form) = &users.items[2] else {
            panic!()
        };
        assert_eq!((form.body.kind, form.body.form.len()), (BodyKind::Form, 1));
        let Item::Request(multi) = &users.items[3] else {
            panic!()
        };
        assert_eq!(
            (multi.body.kind, multi.body.form.len()),
            (BodyKind::Form, 1)
        );
        let Item::Request(plain) = &got.collection.items[2] else {
            panic!()
        };
        assert_eq!(plain.url, "https://x.test/plain");
        // What was changed or left out is said.
        assert_eq!(got.warnings.len(), 2, "{:?}", got.warnings);
        assert!(got.warnings.iter().any(|w| w.contains("multipart")));
        assert!(got.warnings.iter().any(|w| w.contains("oauth2")));
    }

    #[test]
    fn what_is_not_a_collection_is_said() {
        assert!(import_collection("nope")
            .unwrap_err()
            .contains("Not a JSON"));
        assert!(import_collection("{}")
            .unwrap_err()
            .contains("not a Postman collection"));
        let old = r#"{"info":{"name":"Old","schema":"https://schema.getpostman.com/json/collection/v1.0.0/collection.json"},"item":[]}"#;
        assert!(import_collection(old).unwrap().warnings[0].contains("older format"));
        assert!(import_environment("{}")
            .unwrap_err()
            .contains("not a Postman environment"));
    }

    #[test]
    fn a_collection_is_written_and_read_back_the_same() {
        let mut first = import_collection(COLLECTION).unwrap().collection;
        let mut next = 0;
        first.number(&mut next);
        let text = export_collection(&first);
        assert!(text.contains(SCHEMA));
        let mut back = import_collection(&text).unwrap();
        // The multipart form and the unsupported sign-in were changed on the
        // way in, so nothing is left to warn about on the way back.
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        let mut again = 0;
        back.collection.number(&mut again);
        assert_eq!(back.collection, first);
    }

    #[test]
    fn an_environment_is_read_and_a_secret_is_not_shared() {
        let json = r#"{"name": "dev", "values": [
          {"key": "host", "value": "localhost", "enabled": true},
          {"key": "token", "value": "abc", "type": "secret", "enabled": true},
          {"key": "off", "value": "x", "enabled": false},
          {"key": "", "value": "no name"}]}"#;
        assert!(is_environment(json));
        assert!(!is_environment(COLLECTION));
        let env = import_environment(json).unwrap();
        assert_eq!(env.name, "dev");
        assert_eq!(env.vars.len(), 3);
        assert!(env.vars[1].secret && !env.vars[2].enabled);
        let shared = export_environment(&env, false);
        assert!(!shared.contains("abc") && shared.contains("localhost"));
        let own = export_environment(&env, true);
        assert!(own.contains("abc"));
        assert_eq!(import_environment(&own).unwrap().vars, env.vars);
    }

    #[test]
    fn an_address_given_in_parts_is_put_together() {
        let json = r#"{"info":{"name":"p"},"item":[{"name":"r","request":{"method":"GET",
          "url":{"protocol":"http","host":["api","test"],"path":["v1","items"],"query":[{"key":"a","value":"1"}]}}}]}"#;
        let got = import_collection(json).unwrap();
        let Item::Request(r) = &got.collection.items[0] else {
            panic!()
        };
        assert_eq!(r.url, "http://api.test/v1/items");
        assert_eq!(r.params, vec![Pair::new("a", "1")]);
        // A GraphQL body is a JSON one.
        let gql = r#"{"info":{"name":"g"},"item":[{"name":"q","request":{"method":"POST","url":"https://x",
          "body":{"mode":"graphql","graphql":{"query":"{ me { id } }","variables":"{\"a\":1}"}}}}]}"#;
        let got = import_collection(gql).unwrap();
        let Item::Request(r) = &got.collection.items[0] else {
            panic!()
        };
        assert_eq!(r.body.kind, BodyKind::Json);
        assert!(r.body.text.contains("me { id }") && r.body.text.contains("\"a\": 1"));
    }
}
