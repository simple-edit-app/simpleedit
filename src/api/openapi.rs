//! Reads an OpenAPI 3 or Swagger 2 description (JSON or YAML) as a
//! collection of requests: one per operation, in a folder per tag, with the
//! address of the server and the parameters of the paths as variables.

use serde_json::Value;

use super::model::{
    Auth, AuthKind, Body, BodyKind, Collection, Folder, Item, Method, Pair, Request, Variable,
};
use super::postman::Imported;

/// The description as data, whichever way it is written.
fn parse(text: &str) -> Option<Value> {
    serde_json::from_str::<Value>(text)
        .ok()
        .or_else(|| serde_yaml::from_str::<Value>(text).ok())
}

/// Whether the text is an OpenAPI or a Swagger description.
pub fn is_openapi(text: &str) -> bool {
    parse(text).is_some_and(|v| {
        (v.get("openapi").is_some() || v.get("swagger").is_some()) && v["paths"].is_object()
    })
}

/// A `$ref` (`#/components/schemas/Name`) followed, up to a few steps.
fn resolve<'a>(root: &'a Value, mut value: &'a Value) -> &'a Value {
    for _ in 0..8 {
        let Some(target) = value.get("$ref").and_then(Value::as_str) else {
            break;
        };
        let Some(pointer) = target.strip_prefix('#') else {
            break;
        };
        match root.pointer(pointer) {
            Some(found) => value = found,
            None => break,
        }
    }
    value
}

/// An example value for a schema: its own example, its default, the first
/// of its enumeration, or something of the right type.
fn example(root: &Value, schema: &Value, depth: usize) -> Value {
    let schema = resolve(root, schema);
    if depth > 6 {
        return Value::Null;
    }
    for key in ["example", "default"] {
        if let Some(v) = schema.get(key) {
            return v.clone();
        }
    }
    if let Some(first) = schema["enum"].as_array().and_then(|e| e.first()) {
        return first.clone();
    }
    for key in ["allOf", "oneOf", "anyOf"] {
        if let Some(parts) = schema[key].as_array() {
            if key == "allOf" {
                let mut merged = serde_json::Map::new();
                for part in parts {
                    if let Value::Object(map) = example(root, part, depth + 1) {
                        merged.extend(map);
                    }
                }
                return Value::Object(merged);
            }
            return parts
                .first()
                .map_or(Value::Null, |p| example(root, p, depth + 1));
        }
    }
    let kind = schema["type"]
        .as_str()
        .unwrap_or(if schema["properties"].is_object() {
            "object"
        } else {
            ""
        });
    match kind {
        "object" => {
            let mut map = serde_json::Map::new();
            if let Some(props) = schema["properties"].as_object() {
                for (name, prop) in props {
                    map.insert(name.clone(), example(root, prop, depth + 1));
                }
            }
            Value::Object(map)
        }
        "array" => Value::Array(vec![example(root, &schema["items"], depth + 1)]),
        "integer" => Value::from(0),
        "number" => Value::from(0.0),
        "boolean" => Value::Bool(false),
        "string" => Value::String(
            match schema["format"].as_str() {
                Some("date") => "2024-01-01",
                Some("date-time") => "2024-01-01T00:00:00Z",
                Some("email") => "user@example.com",
                Some("uuid") => "00000000-0000-0000-0000-000000000000",
                Some("uri") | Some("url") => "https://example.com",
                _ => "string",
            }
            .to_string(),
        ),
        _ => Value::Null,
    }
}

/// An example as the text of a parameter or a field.
fn as_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `{id}` becomes `{{id}}` so that a variable fills it in.
fn variables_in(path: &str) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        let name = &rest[open + 1..open + close];
        out.push_str(&rest[..open]);
        out.push_str(&format!("{{{{{name}}}}}"));
        names.push(name.to_string());
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    (out, names)
}

/// The address of the server: the first of `servers` (its variables at their
/// defaults), or Swagger's `scheme://host/basePath`.
fn base_url(root: &Value) -> String {
    if let Some(server) = root["servers"].as_array().and_then(|s| s.first()) {
        let mut url = server["url"].as_str().unwrap_or("").to_string();
        if let Some(vars) = server["variables"].as_object() {
            for (name, var) in vars {
                let default = var["default"].as_str().unwrap_or("");
                url = url.replace(&format!("{{{name}}}"), default);
            }
        }
        // A relative address (`/v1`) is relative to where the file came from.
        if url.starts_with('/') {
            url = format!("http://localhost{url}");
        }
        return url.trim_end_matches('/').to_string();
    }
    let host = root["host"].as_str().unwrap_or("");
    if host.is_empty() {
        return String::new();
    }
    let scheme = root["schemes"]
        .as_array()
        .and_then(|s| s.first())
        .and_then(Value::as_str)
        .unwrap_or("https");
    let base = root["basePath"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches('/');
    format!("{scheme}://{host}{base}")
}

/// The sign-in the description asks for everywhere: a bearer token, basic,
/// or an API key, read from variables.
fn auth_of(root: &Value, variables: &mut Vec<Variable>) -> Auth {
    let Some(name) = root["security"]
        .as_array()
        .and_then(|s| s.first())
        .and_then(|s| s.as_object())
        .and_then(|s| s.keys().next())
    else {
        return Auth::default();
    };
    let scheme = root
        .pointer(&format!("/components/securitySchemes/{name}"))
        .or_else(|| root.pointer(&format!("/securityDefinitions/{name}")));
    let Some(scheme) = scheme else {
        return Auth::default();
    };
    let mut add = |key: &str| {
        if !variables.iter().any(|v| v.key == key) {
            variables.push(Variable {
                key: key.to_string(),
                value: String::new(),
                ..Variable::default()
            });
        }
    };
    match (scheme["type"].as_str(), scheme["scheme"].as_str()) {
        (Some("http"), Some(s)) if s.eq_ignore_ascii_case("bearer") => {
            add("token");
            Auth {
                kind: AuthKind::Bearer,
                token: "{{token}}".into(),
                ..Auth::default()
            }
        }
        (Some("http"), Some(s)) if s.eq_ignore_ascii_case("basic") => {
            add("username");
            add("password");
            Auth {
                kind: AuthKind::Basic,
                username: "{{username}}".into(),
                password: "{{password}}".into(),
                ..Auth::default()
            }
        }
        (Some("basic"), _) => {
            add("username");
            add("password");
            Auth {
                kind: AuthKind::Basic,
                username: "{{username}}".into(),
                password: "{{password}}".into(),
                ..Auth::default()
            }
        }
        (Some("apiKey"), _) => {
            add("apiKey");
            Auth {
                kind: AuthKind::ApiKey,
                key_name: scheme["name"].as_str().unwrap_or("X-API-Key").to_string(),
                key_value: "{{apiKey}}".into(),
                key_in_header: scheme["in"].as_str() != Some("query"),
                ..Auth::default()
            }
        }
        (Some("oauth2"), _) | (Some("openIdConnect"), _) => {
            add("token");
            Auth {
                kind: AuthKind::Bearer,
                token: "{{token}}".into(),
                ..Auth::default()
            }
        }
        _ => Auth::default(),
    }
}

pub fn import(text: &str) -> Result<Imported, String> {
    let root = parse(text).ok_or_else(|| "Not a JSON or YAML file.".to_string())?;
    let paths = root["paths"]
        .as_object()
        .ok_or_else(|| "This is not an OpenAPI description (it has no \"paths\").".to_string())?;
    let mut warnings = Vec::new();
    let mut variables = vec![Variable {
        key: "baseUrl".into(),
        value: base_url(&root),
        ..Variable::default()
    }];
    let auth = auth_of(&root, &mut variables);
    let mut top: Vec<Item> = Vec::new();
    let mut folders: Vec<Folder> = Vec::new();
    for (path, item) in paths {
        let item = resolve(&root, item);
        for (verb, operation) in item.as_object().into_iter().flatten() {
            let Some(method) = Method::parse(verb) else {
                continue;
            };
            let operation = resolve(&root, operation);
            let (url_path, path_names) = variables_in(path);
            let mut request = Request::new(
                operation["summary"]
                    .as_str()
                    .or_else(|| operation["operationId"].as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("{} {path}", method.as_str()))
                    .as_str(),
            );
            request.method = method;
            request.url = format!("{{{{baseUrl}}}}{url_path}");
            request.auth = auth.clone();
            // The parameters of the path and of the operation.
            let listed = item["parameters"]
                .as_array()
                .into_iter()
                .chain(operation["parameters"].as_array())
                .flatten();
            for parameter in listed {
                let parameter = resolve(&root, parameter);
                let name = parameter["name"].as_str().unwrap_or("").to_string();
                if name.is_empty() {
                    continue;
                }
                let schema = if parameter["schema"].is_object() {
                    &parameter["schema"]
                } else {
                    parameter
                };
                let value = as_text(&example(&root, schema, 0));
                let required = parameter["required"].as_bool().unwrap_or(false);
                match parameter["in"].as_str() {
                    Some("query") => request.params.push(Pair {
                        enabled: required,
                        key: name,
                        value,
                    }),
                    Some("header") => request.headers.push(Pair {
                        enabled: required,
                        key: name,
                        value,
                    }),
                    Some("path") => {
                        if !variables.iter().any(|v| v.key == name) {
                            variables.push(Variable {
                                key: name,
                                value,
                                ..Variable::default()
                            });
                        }
                    }
                    Some("body") => {
                        request.body = Body {
                            kind: BodyKind::Json,
                            text: serde_json::to_string_pretty(&example(&root, schema, 0))
                                .unwrap_or_default(),
                            ..Body::default()
                        };
                    }
                    Some("formData") => {
                        request.body.kind = BodyKind::Form;
                        request.body.form.push(Pair::new(&name, &value));
                    }
                    _ => {}
                }
            }
            for name in path_names {
                if !variables.iter().any(|v| v.key == name) {
                    variables.push(Variable {
                        key: name,
                        ..Variable::default()
                    });
                }
            }
            // The body of OpenAPI 3.
            let content = &resolve(&root, &operation["requestBody"])["content"];
            if let Some(media) = content.as_object() {
                if let Some(json) = media
                    .iter()
                    .find(|(k, _)| k.contains("json"))
                    .map(|(_, v)| v)
                {
                    let sample = if json.get("example").is_some() {
                        json["example"].clone()
                    } else {
                        example(&root, &json["schema"], 0)
                    };
                    request.body = Body {
                        kind: BodyKind::Json,
                        text: serde_json::to_string_pretty(&sample).unwrap_or_default(),
                        ..Body::default()
                    };
                    request
                        .headers
                        .push(Pair::new("Content-Type", "application/json"));
                } else if let Some(form) = media
                    .iter()
                    .find(|(k, _)| k.contains("x-www-form-urlencoded"))
                    .map(|(_, v)| v)
                {
                    request.body.kind = BodyKind::Form;
                    if let Value::Object(fields) = example(&root, &form["schema"], 0) {
                        for (k, v) in fields {
                            request.body.form.push(Pair::new(&k, &as_text(&v)));
                        }
                    }
                } else if let Some((kind, _)) = media.iter().next() {
                    warnings.push(format!(
                        "{} {path}: the body type {kind} is not kept.",
                        method.as_str()
                    ));
                }
            }
            let tag = operation["tags"]
                .as_array()
                .and_then(|t| t.first())
                .and_then(Value::as_str);
            match tag {
                Some(tag) => match folders.iter_mut().find(|f| f.name == tag) {
                    Some(folder) => folder.items.push(Item::Request(request)),
                    None => folders.push(Folder {
                        id: 0,
                        name: tag.to_string(),
                        items: vec![Item::Request(request)],
                    }),
                },
                None => top.push(Item::Request(request)),
            }
        }
    }
    let mut items: Vec<Item> = folders.into_iter().map(Item::Folder).collect();
    items.extend(top);
    let title = root["info"]["title"].as_str().unwrap_or("OpenAPI");
    Ok(Imported {
        collection: Collection {
            id: 0,
            name: title.to_string(),
            items,
        },
        variables,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = r##"{
      "openapi": "3.0.0",
      "info": {"title": "Pets"},
      "servers": [{"url": "https://{env}.example.com/v1/", "variables": {"env": {"default": "api"}}}],
      "security": [{"bearer": []}],
      "components": {
        "securitySchemes": {"bearer": {"type": "http", "scheme": "bearer"}},
        "schemas": {"Pet": {"type": "object", "properties": {
            "id": {"type": "integer"}, "name": {"type": "string", "example": "Rex"},
            "tags": {"type": "array", "items": {"type": "string"}},
            "kind": {"type": "string", "enum": ["dog", "cat"]}}}}
      },
      "paths": {
        "/pets": {
          "get": {"tags": ["pets"], "summary": "List pets", "parameters": [
            {"name": "limit", "in": "query", "required": true, "schema": {"type": "integer", "default": 10}},
            {"name": "q", "in": "query", "schema": {"type": "string"}}]},
          "post": {"tags": ["pets"], "operationId": "addPet", "requestBody": {"content": {
            "application/json": {"schema": {"$ref": "#/components/schemas/Pet"}}}}}
        },
        "/pets/{petId}": {
          "parameters": [{"name": "petId", "in": "path", "required": true, "schema": {"type": "integer", "example": 5}}],
          "delete": {"summary": "Remove"}
        }
      }
    }"##;

    #[test]
    fn an_openapi_description_is_recognised_in_json_and_yaml() {
        assert!(is_openapi(SPEC));
        assert!(is_openapi("openapi: 3.0.0\ninfo: {title: x}\npaths: {}\n"));
        assert!(!is_openapi(r#"{"info": {}, "item": []}"#));
        assert!(!is_openapi("not a description"));
    }

    #[test]
    fn operations_become_requests_in_folders_with_variables() {
        let imported = import(SPEC).unwrap();
        let c = &imported.collection;
        assert_eq!(c.name, "Pets");
        // One folder (the tag) and the operation without a tag at the top.
        let Item::Folder(pets) = &c.items[0] else {
            panic!("the folder of the tag");
        };
        assert_eq!(pets.name, "pets");
        let Item::Request(list) = &pets.items[0] else {
            panic!("a request");
        };
        assert_eq!(
            (list.name.as_str(), list.method),
            ("List pets", Method::Get)
        );
        assert_eq!(list.url, "{{baseUrl}}/pets");
        assert_eq!(
            list.params[0],
            Pair {
                enabled: true,
                key: "limit".into(),
                value: "10".into()
            }
        );
        assert!(!list.params[1].enabled, "an optional parameter starts off");
        assert_eq!(list.auth.kind, AuthKind::Bearer);
        let Item::Request(add) = &pets.items[1] else {
            panic!("a request");
        };
        assert_eq!(add.name, "addPet");
        assert_eq!(add.body.kind, BodyKind::Json);
        let body: Value = serde_json::from_str(&add.body.text).unwrap();
        assert_eq!(body["name"], "Rex");
        assert_eq!(body["kind"], "dog");
        assert_eq!(body["tags"], serde_json::json!(["string"]));
        let Item::Request(remove) = &c.items[1] else {
            panic!("a request");
        };
        assert_eq!(remove.url, "{{baseUrl}}/pets/{{petId}}");
        let value = |key: &str| {
            imported
                .variables
                .iter()
                .find(|v| v.key == key)
                .map(|v| v.value.clone())
        };
        assert_eq!(
            value("baseUrl").as_deref(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(value("petId").as_deref(), Some("5"));
        assert!(value("token").is_some(), "the token of the sign-in");
    }

    #[test]
    fn a_swagger_2_description_gives_its_server_and_body() {
        let spec = r#"
swagger: "2.0"
info: {title: Old}
host: example.org
basePath: /api
schemes: [http]
paths:
  /items:
    post:
      parameters:
        - {name: item, in: body, schema: {type: object, properties: {n: {type: integer}}}}
"#;
        let imported = import(spec).unwrap();
        let Item::Request(r) = &imported.collection.items[0] else {
            panic!("a request");
        };
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.body.kind, BodyKind::Json);
        assert!(r.body.text.contains("\"n\": 0"));
        assert_eq!(imported.variables[0].value, "http://example.org/api");
    }

    #[test]
    fn a_relative_server_url_gets_a_host() {
        let spec = r#"{"openapi": "3.0.0", "servers": [{"url": "/v1"}], "paths": {}}"#;
        assert_eq!(base_url(&parse(spec).unwrap()), "http://localhost/v1");
    }

    #[test]
    fn a_reference_that_loops_does_not_hang() {
        let spec = r##"{"openapi": "3.0.0", "paths": {"/a": {"post": {"requestBody": {"content": {
            "application/json": {"schema": {"$ref": "#/components/schemas/Node"}}}}}}},
            "components": {"schemas": {"Node": {"type": "object", "properties": {
                "next": {"$ref": "#/components/schemas/Node"}}}}}}"##;
        let imported = import(spec).unwrap();
        let Item::Request(r) = &imported.collection.items[0] else {
            panic!("a request");
        };
        assert!(r.body.text.contains("next"));
    }
}
