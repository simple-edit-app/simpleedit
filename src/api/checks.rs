//! The tests of an answer and the values kept from it: what a request
//! asks to check (status, a header, the body, a place of the JSON, the
//! time) and to remember (a token, an id) for the requests that follow.

use std::collections::HashMap;

use serde_json::Value;

use super::http::Response;
use super::model::{substitute, Check, CheckOp, CheckSource, Extract, ExtractSource};

/// How a test went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub passed: bool,
    /// What was tested, in words.
    pub label: String,
    /// What the answer had (empty when nothing).
    pub actual: String,
}

/// The value at a place of a JSON text: `data.items[0].id`, `$.a.b`.
/// `length` gives the size of a list, a text or an object that has no such
/// name.
pub fn json_path(root: &Value, path: &str) -> Option<Value> {
    let path = path.trim().trim_start_matches('$').trim_start_matches('.');
    let mut current = root.clone();
    if path.is_empty() {
        return Some(current);
    }
    for part in path.split('.') {
        let (name, mut rest) = match part.find('[') {
            Some(at) => (&part[..at], &part[at..]),
            None => (part, ""),
        };
        if !name.is_empty() {
            current = match (&current, name) {
                (Value::Object(map), _) if map.contains_key(name) => map[name].clone(),
                (Value::Array(list), "length") => Value::from(list.len()),
                (Value::String(text), "length") => Value::from(text.chars().count()),
                (Value::Object(map), "length") => Value::from(map.len()),
                _ => return None,
            };
        }
        while let Some(open) = rest.strip_prefix('[') {
            let close = open.find(']')?;
            let index: usize = open[..close].trim().parse().ok()?;
            current = current.get(index)?.clone();
            rest = &open[close + 1..];
        }
    }
    Some(current)
}

/// A value as a text: a string as it is, the rest as JSON.
fn shown(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok()
}

fn holds(op: CheckOp, actual: Option<&str>, expected: &str) -> bool {
    let same = |a: &str| match (number(a), number(expected)) {
        (Some(x), Some(y)) => x == y,
        _ => a == expected,
    };
    match op {
        CheckOp::Exists => actual.is_some(),
        CheckOp::Equals => actual.is_some_and(same),
        CheckOp::NotEquals => !actual.is_some_and(same),
        CheckOp::Contains => actual.is_some_and(|a| a.contains(expected)),
        CheckOp::NotContains => !actual.is_some_and(|a| a.contains(expected)),
        CheckOp::Less => match (actual.and_then(number), number(expected)) {
            (Some(a), Some(e)) => a < e,
            _ => false,
        },
        CheckOp::Greater => match (actual.and_then(number), number(expected)) {
            (Some(a), Some(e)) => a > e,
            _ => false,
        },
        CheckOp::Matches => match (actual, regex::Regex::new(expected)) {
            (Some(a), Ok(re)) => re.is_match(a),
            _ => false,
        },
    }
}

/// What the answer has for a check, if it has anything.
fn actual_of(check: &Check, response: &Response, json: &Option<Value>) -> Option<String> {
    match check.source {
        CheckSource::Status => Some(response.status.to_string()),
        CheckSource::Time => Some(response.ms.to_string()),
        CheckSource::Header => response.header(check.name.trim()).map(str::to_string),
        CheckSource::Body => Some(response.text()),
        CheckSource::Json => json
            .as_ref()
            .and_then(|root| json_path(root, &check.name))
            .map(|v| shown(&v)),
    }
}

/// Runs the checks that are on against the answer.
pub fn evaluate(
    checks: &[Check],
    response: &Response,
    vars: &HashMap<String, String>,
) -> Vec<Outcome> {
    let json: Option<Value> = serde_json::from_slice(&response.body).ok();
    checks
        .iter()
        .filter(|c| c.enabled)
        .map(|check| {
            let expected = substitute(&check.expected, vars, &mut Vec::new());
            let actual = actual_of(check, response, &json);
            let passed = holds(check.op, actual.as_deref(), &expected);
            let subject = match check.source {
                CheckSource::Header | CheckSource::Json if !check.name.trim().is_empty() => {
                    format!("{} {}", check.source, check.name.trim())
                }
                other => other.to_string(),
            };
            let label = if check.op == CheckOp::Exists {
                format!("{subject} {}", check.op)
            } else {
                format!("{subject} {} {expected}", check.op)
            };
            let mut actual = actual.unwrap_or_default();
            if actual.chars().count() > 80 {
                actual = actual.chars().take(80).collect::<String>() + "…";
            }
            Outcome {
                passed,
                label,
                actual,
            }
        })
        .collect()
}

/// The values to keep: the name of the variable and the value, or why
/// there is none.
pub fn extract(extracts: &[Extract], response: &Response) -> Vec<(String, Result<String, String>)> {
    let json: Option<Value> = serde_json::from_slice(&response.body).ok();
    extracts
        .iter()
        .filter(|e| e.enabled && !e.variable.trim().is_empty())
        .map(|e| {
            let found: Result<String, String> = match e.source {
                ExtractSource::Json => json
                    .as_ref()
                    .and_then(|root| json_path(root, &e.path))
                    .map(|v| shown(&v))
                    .ok_or_else(|| t!("api.err_not_in_answer", path = e.path.trim()).to_string()),
                ExtractSource::Header => response
                    .header(e.path.trim())
                    .map(str::to_string)
                    .ok_or_else(|| t!("api.err_no_header", path = e.path.trim()).to_string()),
                ExtractSource::Body => regex::Regex::new(e.path.trim())
                    .map_err(|err| err.to_string())
                    .and_then(|re| {
                        let text = response.text();
                        re.captures(&text)
                            .map(|c| {
                                c.get(1)
                                    .or_else(|| c.get(0))
                                    .map_or("", |m| m.as_str())
                                    .to_string()
                            })
                            .ok_or_else(|| t!("api.err_no_match").to_string())
                    }),
            };
            (e.variable.trim().to_string(), found)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, body: &str) -> Response {
        Response {
            status,
            reason: String::new(),
            version: String::new(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            cookies: Vec::new(),
            body: body.as_bytes().to_vec(),
            truncated: false,
            ms: 120,
            url: String::new(),
        }
    }

    fn check(source: CheckSource, name: &str, op: CheckOp, expected: &str) -> Check {
        Check {
            enabled: true,
            source,
            name: name.into(),
            op,
            expected: expected.into(),
        }
    }

    #[test]
    fn places_of_a_json_are_found() {
        let v: Value =
            serde_json::from_str(r#"{"a": {"b": [10, {"c": "x"}]}, "s": "héé"}"#).unwrap();
        assert_eq!(json_path(&v, "a.b[0]"), Some(Value::from(10)));
        assert_eq!(json_path(&v, "$.a.b[1].c"), Some(Value::from("x")));
        assert_eq!(json_path(&v, "a.b.length"), Some(Value::from(2)));
        assert_eq!(json_path(&v, "s.length"), Some(Value::from(3)));
        assert_eq!(json_path(&v, "a.nope"), None);
        assert_eq!(json_path(&v, "a.b[5]"), None);
        assert!(json_path(&v, "").is_some());
    }

    #[test]
    fn checks_pass_and_fail_with_what_the_answer_had() {
        let r = response(201, r#"{"id": 7, "name": "Ann", "tags": ["a", "b"]}"#);
        let vars: HashMap<String, String> = [("who".to_string(), "Ann".to_string())].into();
        let checks = vec![
            check(CheckSource::Status, "", CheckOp::Equals, "201"),
            check(CheckSource::Status, "", CheckOp::Less, "300"),
            check(
                CheckSource::Header,
                "content-type",
                CheckOp::Contains,
                "json",
            ),
            check(CheckSource::Json, "id", CheckOp::Equals, "7.0"),
            check(CheckSource::Json, "name", CheckOp::Equals, "{{who}}"),
            check(CheckSource::Json, "tags.length", CheckOp::Greater, "5"),
            check(CheckSource::Json, "missing", CheckOp::Exists, ""),
            check(CheckSource::Body, "", CheckOp::Matches, r#""id":\s*\d+"#),
            check(CheckSource::Time, "", CheckOp::Less, "100"),
            Check {
                enabled: false,
                ..check(CheckSource::Status, "", CheckOp::Equals, "500")
            },
        ];
        let out = evaluate(&checks, &r, &vars);
        let passed: Vec<bool> = out.iter().map(|o| o.passed).collect();
        assert_eq!(
            passed,
            vec![true, true, true, true, true, false, false, true, false],
            "{out:#?}"
        );
        assert_eq!(out.len(), 9, "a check that is off is not run");
        assert_eq!(out[5].actual, "2");
        assert!(out[4].label.ends_with("Ann"), "{}", out[4].label);
        assert_eq!(out[6].actual, "");
    }

    #[test]
    fn values_are_kept_from_the_json_the_headers_and_the_body() {
        let r = response(200, r#"{"data": {"token": "abc", "n": 3}}"#);
        let extracts = vec![
            Extract {
                variable: "token".into(),
                path: "data.token".into(),
                ..Extract::default()
            },
            Extract {
                variable: "n".into(),
                path: "data.n".into(),
                ..Extract::default()
            },
            Extract {
                variable: "type".into(),
                source: ExtractSource::Header,
                path: "Content-Type".into(),
                ..Extract::default()
            },
            Extract {
                variable: "digit".into(),
                source: ExtractSource::Body,
                path: r#""n":\s*(\d)"#.into(),
                ..Extract::default()
            },
            Extract {
                variable: "none".into(),
                path: "data.zzz".into(),
                ..Extract::default()
            },
            Extract {
                enabled: false,
                variable: "off".into(),
                path: "data".into(),
                ..Extract::default()
            },
        ];
        let got = extract(&extracts, &r);
        assert_eq!(got.len(), 5);
        assert_eq!(got[0], ("token".into(), Ok("abc".into())));
        assert_eq!(got[1], ("n".into(), Ok("3".into())));
        assert_eq!(got[2].1, Ok("application/json".into()));
        assert_eq!(got[3].1, Ok("3".into()));
        assert!(got[4].1.is_err());
    }

    #[test]
    fn a_body_that_is_not_json_fails_a_json_check_without_a_panic() {
        let r = response(200, "plain text");
        let out = evaluate(
            &[check(CheckSource::Json, "a", CheckOp::Exists, "")],
            &r,
            &HashMap::new(),
        );
        assert!(!out[0].passed);
    }
}

/// How one request of a run went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub name: String,
    pub status: Option<u16>,
    pub ms: u64,
    pub error: Option<String>,
    /// The tests that passed, and the labels of those that failed.
    pub passed: usize,
    pub failed: Vec<String>,
}

/// Sends the requests one after the other, each with the variables the
/// ones before kept: the results, and the values kept (to put in the
/// environment).
pub async fn run_all(
    requests: Vec<super::model::Request>,
    mut vars: HashMap<String, String>,
) -> (Vec<RunResult>, Vec<(String, String)>) {
    let mut results = Vec::new();
    let mut kept: Vec<(String, String)> = Vec::new();
    for request in requests {
        let name = request.title();
        match super::http::send(request.resolve(&vars)).await {
            Ok(response) => {
                let outcomes = evaluate(&request.checks, &response, &vars);
                for (variable, value) in extract(&request.extracts, &response) {
                    if let Ok(value) = value {
                        vars.insert(variable.clone(), value.clone());
                        kept.retain(|(k, _)| *k != variable);
                        kept.push((variable, value));
                    }
                }
                results.push(RunResult {
                    name,
                    status: Some(response.status),
                    ms: response.ms,
                    error: None,
                    passed: outcomes.iter().filter(|o| o.passed).count(),
                    failed: outcomes
                        .iter()
                        .filter(|o| !o.passed)
                        .map(|o| format!("{} ({})", o.label, o.actual))
                        .collect(),
                });
            }
            Err(error) => results.push(RunResult {
                name,
                status: None,
                ms: 0,
                error: Some(error),
                passed: 0,
                failed: Vec::new(),
            }),
        }
    }
    (results, kept)
}

#[cfg(test)]
mod run_tests {
    use super::*;
    use crate::api::model::{Extract, Request};

    #[test]
    fn a_run_chains_the_requests_with_what_each_one_kept() {
        let _guard = crate::api::store::test_lock();
        // The first answer gives an id, the second one is asked for it.
        let (port, seen) = crate::api::testing::serve(|_| {
            let answer = |body: &str| {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .into_bytes()
            };
            vec![answer(r#"{"id": 42}"#), answer(r#"{"ok": true}"#)]
        });
        let mut first = Request::new("create");
        first.url = format!("http://127.0.0.1:{port}/create");
        first.extracts = vec![Extract {
            variable: "id".into(),
            path: "id".into(),
            ..Extract::default()
        }];
        let mut second = Request::new("read");
        second.url = format!("http://127.0.0.1:{port}/items/{{{{id}}}}");
        second.checks = vec![
            super::super::model::Check::default(),
            super::super::model::Check {
                source: CheckSource::Json,
                name: "ok".into(),
                expected: "false".into(),
                ..super::super::model::Check::default()
            },
        ];
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (results, kept) = runtime.block_on(run_all(vec![first, second], HashMap::new()));
        assert_eq!(kept, vec![("id".to_string(), "42".to_string())]);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].status, Some(200));
        assert_eq!((results[1].passed, results[1].failed.len()), (1, 1));
        let asked = seen.lock().unwrap();
        assert!(
            asked[1].request_line.contains("/items/42"),
            "{}",
            asked[1].request_line
        );
    }

    #[test]
    fn a_request_that_cannot_be_sent_is_a_result_and_the_run_goes_on() {
        let mut bad = Request::new("down");
        bad.url = "http://127.0.0.1:1/".into();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (results, kept) = runtime.block_on(run_all(vec![bad], HashMap::new()));
        assert!(results[0].error.is_some() && kept.is_empty());
    }
}
