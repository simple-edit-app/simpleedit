//! Search everywhere (Shift twice), as IntelliJ IDEA has it: one field that
//! looks for classes, files, symbols, actions, settings and text, by tab or
//! all at once. This module holds what does not depend on the window: the
//! tabs, how a query is read (`file:line`, a calculation, a qualified name)
//! and how actions match.

/// A tab of the search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tab {
    All,
    Classes,
    Files,
    Symbols,
    Actions,
    Text,
    Git,
}

impl Tab {
    /// The tabs shown, in order (Git only when turned on).
    pub fn shown(git: bool) -> Vec<Tab> {
        let mut tabs = vec![
            Tab::All,
            Tab::Classes,
            Tab::Files,
            Tab::Symbols,
            Tab::Actions,
            Tab::Text,
        ];
        if git {
            tabs.push(Tab::Git);
        }
        tabs
    }

    /// The tab `delta` places away, going round.
    pub fn step(self, delta: isize, git: bool) -> Tab {
        let tabs = Tab::shown(git);
        let at = tabs.iter().position(|t| *t == self).unwrap_or(0) as isize;
        tabs[(at + delta).rem_euclid(tabs.len() as isize) as usize]
    }
}

/// A query for a file that ends with a place in it: `main.rs:12` or
/// `main.rs:12:5` (lines and columns from 1). Gives the name and the place
/// (from 0).
pub fn location(query: &str) -> (&str, Option<(usize, Option<usize>)>) {
    let number = |s: &str| s.parse::<usize>().ok().filter(|n| *n > 0);
    let mut parts = query.rsplitn(3, ':');
    let last = parts.next().unwrap_or("");
    let middle = parts.next();
    let first = parts.next();
    match (first, middle, number(last)) {
        (Some(name), Some(line), Some(col)) if number(line).is_some() && !name.is_empty() => {
            (name, Some((number(line).unwrap() - 1, Some(col - 1))))
        }
        (_, Some(_), Some(line)) => {
            let name = &query[..query.len() - last.len() - 1];
            if name.is_empty() {
                (query, None)
            } else {
                (name, Some((line - 1, None)))
            }
        }
        _ => (query, None),
    }
}

/// A qualified name (`Foo.bar`, `module::function`): what it is in, and its
/// own name.
pub fn qualified(query: &str) -> (Option<&str>, &str) {
    for separator in ["::", ".", "#"] {
        if let Some((container, name)) = query.rsplit_once(separator) {
            if !container.is_empty() {
                return (Some(container), name);
            }
        }
    }
    (None, query)
}

// ─── Calculations ──────────────────────────────────────────────────────────

struct Calc<'a> {
    s: &'a [u8],
    i: usize,
}

impl Calc<'_> {
    fn space(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.space();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    /// sum := product (('+' | '-') product)*
    fn sum(&mut self) -> Option<f64> {
        let mut value = self.product()?;
        loop {
            if self.eat(b'+') {
                value += self.product()?;
            } else if self.eat(b'-') {
                value -= self.product()?;
            } else {
                return Some(value);
            }
        }
    }

    /// product := power (('*' | '/' | '%') power)*
    fn product(&mut self) -> Option<f64> {
        let mut value = self.power()?;
        loop {
            if self.eat(b'*') {
                value *= self.power()?;
            } else if self.eat(b'/') {
                value /= self.power()?;
            } else if self.eat(b'%') {
                value %= self.power()?;
            } else {
                return Some(value);
            }
        }
    }

    /// power := unary ('^' power)?   (right associative)
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.eat(b'^') {
            return Some(base.powf(self.power()?));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat(b'-') {
            return Some(-self.unary()?);
        }
        if self.eat(b'+') {
            return self.unary();
        }
        self.atom()
    }

    fn atom(&mut self) -> Option<f64> {
        self.space();
        if self.eat(b'(') {
            let value = self.sum()?;
            return self.eat(b')').then_some(value);
        }
        let start = self.i;
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        if self.i > start {
            let name = std::str::from_utf8(&self.s[start..self.i]).ok()?;
            let f: fn(f64) -> f64 = match name.to_ascii_lowercase().as_str() {
                "sqrt" => f64::sqrt,
                "sin" => f64::sin,
                "cos" => f64::cos,
                "tan" => f64::tan,
                _ => return None,
            };
            if !self.eat(b'(') {
                return None;
            }
            let value = self.sum()?;
            return self.eat(b')').then(|| f(value));
        }
        self.number()
    }

    fn number(&mut self) -> Option<f64> {
        let rest = &self.s[self.i..];
        // 0x…, 0b…, 0o…
        if rest.len() > 2 && rest[0] == b'0' {
            let radix = match rest[1].to_ascii_lowercase() {
                b'x' => Some(16),
                b'b' => Some(2),
                b'o' => Some(8),
                _ => None,
            };
            if let Some(radix) = radix {
                let digits = rest[2..]
                    .iter()
                    .take_while(|c| (**c as char).is_digit(radix) || **c == b'_')
                    .count();
                let text: String = std::str::from_utf8(&rest[2..2 + digits])
                    .ok()?
                    .replace('_', "");
                self.i += 2 + digits;
                return i64::from_str_radix(&text, radix).ok().map(|n| n as f64);
            }
        }
        let len = rest
            .iter()
            .take_while(|c| c.is_ascii_digit() || **c == b'.' || **c == b'_')
            .count();
        if len == 0 {
            return None;
        }
        let text = std::str::from_utf8(&rest[..len]).ok()?.replace('_', "");
        self.i += len;
        text.parse().ok()
    }
}

/// The value of an arithmetic expression: `+ - * / %`, `^` (power),
/// parentheses, `sqrt sin cos tan`, numbers in decimal, `0x` hexadecimal,
/// `0b` binary and `0o` octal.
pub fn evaluate(expression: &str) -> Option<f64> {
    let mut calc = Calc {
        s: expression.as_bytes(),
        i: 0,
    };
    let value = calc.sum()?;
    calc.space();
    (calc.i == calc.s.len() && value.is_finite()).then_some(value)
}

/// A query that is a calculation (it has an operator or a function, not
/// just a number), and its result as shown.
pub fn calculation(query: &str) -> Option<String> {
    let q = query.trim();
    let calculates =
        q.chars().any(|c| "+-*/^%(".contains(c)) && q.chars().any(|c| c.is_ascii_digit());
    if !calculates {
        return None;
    }
    let value = evaluate(q)?;
    Some(if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        let text = format!("{value:.10}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    })
}

// ─── Actions ───────────────────────────────────────────────────────────────

/// The words of a text, lowercase, accents removed.
fn words(text: &str) -> Vec<String> {
    crate::palette::folded(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// How well a query matches an action (its label, its synonyms, its
/// abbreviations): higher is better, `None` if it doesn't. An abbreviation
/// typed exactly comes first; then the label starting with the query; then
/// every typed word starting a word of the label, in any order; then the
/// same with the synonyms.
pub fn action_score(
    query: &str,
    label: &str,
    synonyms: &[&str],
    abbreviations: &[String],
) -> Option<i64> {
    let q = crate::palette::folded(query.trim());
    if q.is_empty() {
        return Some(0);
    }
    if abbreviations.iter().any(|a| crate::palette::folded(a) == q) {
        return Some(1000);
    }
    let lower = crate::palette::folded(label);
    if lower.starts_with(&q) {
        return Some(500 - lower.len() as i64);
    }
    let typed = words(&q);
    let all_found = |of: &[String]| {
        typed.iter().all(|t| {
            of.iter()
                .any(|w| w.starts_with(t.as_str()) || crate::completion::fuzzy(t, w).is_some())
        })
    };
    if all_found(&words(label)) {
        return Some(300 - lower.len() as i64);
    }
    let mut known = words(label);
    for synonym in synonyms {
        known.extend(words(synonym));
    }
    if all_found(&known) {
        return Some(100 - lower.len() as i64);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_go_round_with_git_only_when_on() {
        assert_eq!(Tab::All.step(1, false), Tab::Classes);
        assert_eq!(Tab::All.step(-1, false), Tab::Text);
        assert_eq!(Tab::All.step(-1, true), Tab::Git);
        assert_eq!(Tab::Text.step(1, false), Tab::All);
    }

    #[test]
    fn a_file_name_with_a_line_and_a_column() {
        assert_eq!(location("main.rs"), ("main.rs", None));
        assert_eq!(location("main.rs:12"), ("main.rs", Some((11, None))));
        assert_eq!(location("main.rs:12:5"), ("main.rs", Some((11, Some(4)))));
        assert_eq!(location("src/app:3"), ("src/app", Some((2, None))));
        // Not a place: kept as typed.
        assert_eq!(location("a:b"), ("a:b", None));
        assert_eq!(location(":12"), (":12", None));
        assert_eq!(location("main.rs:0"), ("main.rs:0", None));
    }

    #[test]
    fn qualified_names() {
        assert_eq!(qualified("User.name"), (Some("User"), "name"));
        assert_eq!(qualified("app::view"), (Some("app"), "view"));
        assert_eq!(qualified("view"), (None, "view"));
        assert_eq!(qualified(".x"), (None, ".x"));
    }

    #[test]
    fn calculations() {
        assert_eq!(calculation("2+3*4"), Some("14".into()));
        assert_eq!(calculation("(2+3)*4"), Some("20".into()));
        assert_eq!(
            calculation("2^3^2"),
            Some("512".into()),
            "right associative"
        );
        assert_eq!(calculation("sqrt(16) + 1"), Some("5".into()));
        assert_eq!(calculation("0xff + 1"), Some("256".into()));
        assert_eq!(calculation("0b101 * 2"), Some("10".into()));
        assert_eq!(calculation("0o17 - 0"), Some("15".into()));
        assert_eq!(calculation("10 / 4"), Some("2.5".into()));
        assert_eq!(calculation("-3 + 1"), Some("-2".into()));
        assert_eq!(calculation("cos(0)*2"), Some("2".into()));
        assert_eq!(calculation("1/3"), Some("0.3333333333".into()));
        // Not calculations.
        assert_eq!(calculation("42"), None);
        assert_eq!(calculation("main.rs"), None);
        assert_eq!(calculation("foo-bar"), None);
        assert_eq!(calculation("1/0"), None);
        assert_eq!(calculation("2+"), None);
        assert_eq!(calculation("nope(2)"), None);
    }

    #[test]
    fn actions_match_by_words_synonyms_and_abbreviations() {
        let label = "Show the minimap";
        assert!(action_score("show", label, &[], &[]).unwrap() > 400);
        // Words in any order, each starting a word.
        assert!(action_score("minimap show", label, &[], &[]).is_some());
        assert!(action_score("mini sh", label, &[], &[]).is_some());
        assert!(
            action_score("map", label, &[], &[]).is_none(),
            "not inside a word"
        );
        // A synonym.
        assert!(action_score("toggle minimap", label, &[], &[]).is_none());
        let with = action_score("toggle minimap", label, &["toggle"], &[]).unwrap();
        assert!(with < action_score("minimap", label, &[], &[]).unwrap());
        // An abbreviation, typed exactly, comes first.
        let abbr = vec!["sm".to_string()];
        assert_eq!(action_score("SM", label, &[], &abbr), Some(1000));
        assert!(action_score("s", label, &[], &abbr).unwrap() < 1000);
        // Accents don't matter, either way.
        assert!(action_score("preferences", "Préférences…", &[], &[]).is_some());
        assert!(action_score("préf", "Preferences", &[], &[]).is_some());
        assert!(action_score("recent", "Ouvrir un fichier récent", &[], &[]).is_some());
    }
}
