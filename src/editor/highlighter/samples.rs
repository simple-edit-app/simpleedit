//! One tricky-but-valid program per language, to catch a grammar that loses
//! its way: after the last line every string, comment and regular expression
//! must be closed, and the final line (plain code) must not be colored as one.
//!
//! Each sample packs the constructs that usually trip a highlighter (quotes
//! inside quotes, comment markers inside strings, nesting, heredocs, raw
//! strings, regular expressions next to divisions) and ends on a plain line.

use super::*;
use text::Highlighter as _;

const GO: &str = r####"package main

import "fmt"

type T struct {
	Name string `json:"name"`
}

/* block comment with "quote" and // inside */
func main() {
	raw := `raw "string" with \ and // not a comment`
	r := '\''
	q := '"'
	s := "http://example.com/*not a comment*/"
	fmt.Println(raw, r, q, s) // trailing comment with 'quote'
	m := map[string][]int{"a": {1, 2}}
	_ = m
}

var done = 1
"####;

const JAVA: &str = r####"package demo;

import java.util.List;

/** Javadoc with "quotes" and 'apostrophes' and /* nested-looking */
@SuppressWarnings("unchecked")
public class Demo<T extends Comparable<T>> {
    private static final String URL = "http://example.com//path";
    private static final char QUOTE = '"';
    private static final char TICK = '\'';
    String block = """
        text block with "quotes" and \"""
        and // not a comment
        """;

    public int run(List<String> xs) {
        xs.forEach(x -> System.out.println(x + "\\")); // comment
        return xs.size() < 3 ? 1 : 2;
    }
}

class Done { int value = 1; }
"####;

const PHP: &str = r####"<?php
namespace App;

# hash comment with 'quote'
// line comment with "quote"
/* block 'comment' */
$name = 'single \' quoted';
$text = "double $name {$name} \" quoted";
$heredoc = <<<EOT
Heredoc with "quotes" and $name and // not a comment
EOT;
$nowdoc = <<<'EOT'
Nowdoc with $not and 'quotes'
EOT;
function f(array $a): int { return count($a) < 3 ? 1 : 2; }
?>
<p>html after php</p>
<?php $done = 1; ?>
"####;

const KOTLIN: &str = r####"package demo

/* outer /* nested */ still comment */
fun main() {
    val raw = """raw "string" with ${1 + 2} and \ and // not a comment"""
    val s = "template $raw and ${raw.length} and \" quote"
    val c = '\''
    val d = '"'
    println("$s $c $d") // trailing comment
}

val done = 1
"####;

const SWIFT: &str = r####"import Foundation

/* outer /* nested */ still comment */
let multi = """
    multi "line" with \(1 + 2) and // not a comment
    """
let raw = #"raw "string" with \ and \#(1)"#
let s = "interp \(multi.count) and \" quote and \\"
// line comment with 'quote'
func f<T: Comparable>(_ a: T, _ b: T) -> Bool { return a < b }

let done = 1
"####;

const LUA: &str = r####"-- line comment with 'quote' and "quote"
--[[ block comment with "quotes" ]]
local long = [[long string with "quotes" and -- not a comment]]
local level = [==[level two ]] still string ]==]
--[==[ block
comment ]==]
local s = 'it\'s' .. "say \"hi\""
local function f(a, b) return a < b end

local done = 1
"####;

const HASKELL: &str = r####"module Demo where

{- outer {- nested -} still comment -}
-- line comment with 'quote'
f' :: Int -> Int
f' x = x + 1

c :: Char
c = '\''

s :: String
s = "say \"hi\" -- not a comment {- nor this -}"

(-->) :: Int -> Int -> Int
a --> b = a + b

done :: Int
done = 1
"####;

const SCALA: &str = r####"package demo

/* outer /* nested */ still comment */
object Demo {
  val raw = """raw "string" with \ and // not a comment"""
  val name = "x"
  val s = s"interp $name and ${name.length} and \" quote"
  val sym = 'sym
  val c = '\''
  def f[T <: Comparable[T]](a: T, b: T): Boolean = a.compareTo(b) < 0
}

object Done { val value = 1 }
"####;

const CS: &str = r####"using System;

namespace Demo
{
    /// <summary>Doc with "quotes" and 'apostrophes'</summary>
    class Demo
    {
        const string Verbatim = @"C:\path\with ""doubled"" quotes and // not a comment";
        const string Interp = $"value {1 + 2} and {{braces}} and \" quote";
        const string Raw = """raw "string" with \ and // not a comment""";
        const char Quote = '"';
        const char Tick = '\'';
        // trailing comment
        bool F(int a, int b) => a < b;
    }

    class Done { int value = 1; }
}
"####;

const C: &str = r####"#include <stdio.h>
#define MULTI(a, b) \
    do { printf("%d", (a) + (b)); } while (0)

/* comment with "quote" and 'apostrophe' */
static const char quote = '"';
static const char tick = '\'';
static const char *url = "http://example.com//path";
// comment continued \
   on the next line

int f(int a, int b) { return a < b ? 1 : 2; }

int done = 1;
"####;

const CPP: &str = r####"#include <string>
#include <vector>

/* comment with "quote" */
template <typename T>
bool less(const T &a, const T &b) { return a < b; }

const char *raw = R"delim(raw "string" with ) and // not a comment)delim";
const char *wide = u8"utf8 \" quote";
const long big = 1'000'000;
const char tick = '\'';
std::vector<std::vector<int>> v;

int done = 1;
"####;

const RUBY: &str = r####"# comment with 'quote'
=begin
block comment with "quote"
=end
name = "world"
s = "hello #{name} and #{"nested #{name}"} and \" quote"
t = 'single \' quoted'
h = <<~EOS
  heredoc with "quotes" and #{name} and # not a comment
EOS
words = %w[a b c]
q = %q(parens (nested) and 'quotes')
re = /ab+c/i
x = 10 / 2 / 5
match = "abc" =~ /b/
sym = :"quoted symbol"
ch = ?a
def f(a, b) = a < b

done = 1
"####;

const PERL: &str = r####"use strict;
# comment with 'quote'
my $name = "world";
my $s = "hello $name and @{[ 1 + 2 ]} and \" quote";
my $t = 'single \' quoted';
my @words = qw(a b c);
my $re = qr{ab+c}ix;
(my $copy = $s) =~ s/world/there/g;
my $heredoc = <<"EOT";
heredoc with "quotes" and $name and # not a comment
EOT
my $half = 10 / 2;
print "x" if $s =~ m/hello/;

=pod

pod block with "quotes"

=cut

my $done = 1;
"####;

const R: &str = r####"# comment with 'quote'
x <- c(1, 2, 3)
s <- "say \"hi\" # not a comment"
t <- 'single \' quoted'
`weird name` <- 1
y <- x %in% c(1, 2)
f <- function(a, b) a < b
r <- r"(raw "string" with \ )"

done <- 1
"####;

const SQL: &str = r####"-- comment with 'quote'
/* block comment with "quote" */
SELECT 'it''s' AS a, "Quoted Ident" AS b, 1 < 2 AS c
FROM t -- trailing comment
WHERE name = 'x -- not a comment' AND id > 3;

INSERT INTO t (a) VALUES ('/* not a comment */');

SELECT 1 AS done;
"####;

const SH: &str = r####"#!/bin/bash
# comment with 'quote'
name="world"
echo "hello ${name:-default} and $(echo "nested $name") and \" quote"
echo 'single quoted with $not and "quotes"'
echo $'ansi \' quote'
arr=(a b c)
echo "${#arr[@]}" ${#name}
cat <<'EOF'
heredoc with "quotes" and $not and # not a comment
EOF
cat <<EOF
heredoc with $name
EOF
case "$name" in
  a) echo "a" ;;
  *) echo "other" ;;
esac
if [ "$name" = "x" ]; then echo "$(date)"; fi

done=1
"####;

const MAKE: &str = r####"# comment with 'quote'
CC := gcc
FLAGS := -Wall \
    -O2
define RULE
	@echo "building $(1)"
endef

all: main.o
	$(CC) $(FLAGS) -o app main.o # trailing comment
	@echo 'single quoted $$HOME'

done := 1
"####;

const TEX: &str = r####"% comment with 'quote'
\documentclass{article}
\begin{document}
Text with 100\% escaped percent and $a < b$ inline math and \textbf{bold}.
\begin{verbatim}
verbatim with % not a comment and \ commands
\end{verbatim}
\[ x = \frac{1}{2} \]
\end{document}
\newcommand{\done}{1}
"####;

const TOML: &str = r####"# comment with 'quote'
title = "TOML \"example\""
literal = 'C:\path\literal'
multi = """
multi-line "basic" string with \
continuation and # not a comment
"""
multi_lit = '''
multi-line 'literal' string with # not a comment
'''
date = 1979-05-27T07:32:00Z
inline = { a = 1, b = "two" } # trailing comment

[section]
done = 1
"####;

const YAML: &str = r####"# comment with 'quote'
---
name: "double \"quoted\" # not a comment"
other: 'single ''quoted'' # not a comment'
anchor: &a { x: 1 }
ref: *a
literal: |
  block with "quotes" and # not a comment
  second line
folded: >
  folded block
  continues here
list:
  - one # trailing comment
  - two
done: 1
"####;

const CSS: &str = r####"/* comment with "quote" and 'apostrophe' */
@import url("a.css");
@media (max-width: 600px) { a { color: red; } }
a[href^="http://"]::after { content: "\201C quote \" inside"; }
.bg { background: url(data:image/png;base64,AAAA//BBBB==) no-repeat; }
.sel > .child + .sib ~ .gen { margin: 0 auto !important; }

.done { color: blue; }
"####;

const SCSS: &str = r####"// line comment with 'quote'
/* block comment with "quote" */
$map: (a: 1, b: 2);
@mixin m($x) { width: $x; }
.a { @include m(10px); &:hover { color: red; } }
.b-#{$x} { content: "interp #{$x} and \" quote"; }

.done { color: blue; }
"####;

const LESS: &str = r####"// line comment with 'quote'
/* block comment with "quote" */
@color: #fff;
.mixin(@a) { color: @a; }
.a { .mixin(@color); width: ~"calc(100% - 10px)"; }
.b { content: "quote \" inside"; }

.done { color: blue; }
"####;

const HTML: &str = r####"<!DOCTYPE html>
<html>
<head>
<!-- comment with "quote" and <b>tags</b> -->
<style>
a[href^="http://"] { color: red; } /* css comment */
</style>
<script>
const s = "</div> in a string";
const re = /a\/b/g; // js comment
</script>
</head>
<body class="x" data-a='single "inner" quotes' title="a > b">
<p>text with &amp; entity<br/>and <a href="x">link</a></p>
</body>
</html>
<span id="done"></span>
"####;

const XML: &str = r####"<?xml version="1.0" encoding="UTF-8"?>
<!-- comment with "quote" and <tags> -->
<root attr="a > b" other='single "inner"'>
  <![CDATA[ raw <not> a tag & "quotes" ]]>
  <item id="1">text</item>
</root>
<done/>
"####;

const JSON: &str = r####"{
  "a": "with \"escaped\" quotes and \\ backslash and // not a comment",
  "b": [1, 2.5e3, true, null],
  "c": { "d": "/* not a comment */", "e": "\u00e9" }
}
"####;

const MD: &str = r####"# Title

Text with *emphasis*, **strong**, `inline code with * and _`, and a [link](http://x.y/a_b_c).

```rust
fn main() { let s = "unclosed? no"; }
```

* list item with `code`
* another

> quote with "quotes"

<!-- html comment -->

## Done
"####;

const DIFF: &str = r####"diff --git a/f b/f
index 1..2 100644
--- a/f
+++ b/f
@@ -1,3 +1,3 @@
 context "quote"
-removed 'line'
+added "line"
@@ -10,2 +10,2 @@
 done
"####;

const RS: &str = r####"//! crate doc with "quote"
/* outer /* nested */ still comment */
#[derive(Debug)]
struct S<'a> { name: &'a str }

fn f<'a>(x: &'a str, c: char) -> &'a str {
    let raw = r#"raw "string" with \ and // not a comment"#;
    let bytes = b"bytes \" quote";
    let q = '"';
    let t = '\'';
    let fmt = format!("{} {{}} {:?}", x, c);
    println!("{raw} {bytes:?} {q} {t} {fmt}"); // trailing comment
    let cl = |a: i32| a < 3;
    let _ = cl(1);
    x
}

const DONE: i32 = 1;
"####;

const PY: &str = r####"# comment with 'quote'
import re

def f(a, b):
    """Docstring with "quotes" and 'apostrophes' and \""" escaped"""
    s = '''triple single with "double" and # not a comment'''
    t = f"fstring {a['key']} and {{braces}} and {'nested'} and \" quote"
    r = r"raw \d+ string"
    by = b'bytes \' quote'
    cont = 1 + \
        2
    return a < b # trailing comment

class C:
    @staticmethod
    def g(x=lambda y: y * 2): return x

done = 1
"####;

const JS: &str = r####"// comment with 'quote'
/* block with "quote" and // inside */
const s = "http://example.com//path /* not a comment */";
const t = 'single \' quoted';
const tpl = `template ${1 + 2} and ${`nested ${s}`} and \` backtick`;
const re = /[/]\/a+/gi.test(s);
const half = 10 / 2 / 5;
const obj = { "}": "{", f() { return a < b; } };
if (half) { /* regex after paren */ }
const arrow = (a, b) => a / b;
label: for (;;) { break label; }

const done = 1;
"####;

const JSX: &str = r####"import React from "react";

// comment with 'quote'
const tpl = `template ${1 + 2} and ${`nested`}`;
export default function App({ items }) {
  const half = 10 / 2 / 5;
  return (
    <div className="app" data-x={`x ${half}`}>
      {/* jsx comment */}
      <img src="a.png" alt="it's" />
      {items.map((i) => <span key={i}>{i / 2}</span>)}
      <p>
        text with 'apostrophe' and "quotes" and a / slash
      </p>
    </div>
  );
}

const done = 1;
"####;

const TS: &str = r####"// comment with 'quote'
/* block with "quote" and // inside */
enum E { A = 1, B }
interface I<T> { readonly x: T; f<U>(u: U): Array<U>; }
type Tpl = `prefix-${string}-suffix`;
@Component({ selector: "x" })
class C<T extends Record<string, number>> implements I<T> {
  private s = "http://example.com//path";
  f<U>(u: U): U { return u as U; }
}
const re = /[/]\/a+/gi;
const half = 10 / 2 / 5;
const t = <T,>(a: T) => a;
const v = x satisfies number as const;

const done = 1;
"####;

const TSX: &str = r####"import React from "react";

type Props<T> = { items: T[] };
// comment with 'quote'
export function List<T>({ items }: Props<T>) {
  const tpl = `item ${items.length} and ${`nested`}`;
  return (
    <ul className={`list ${tpl}`}>
      {items.map((i, n) => (
        <li key={n} title="it's">{String(i)}</li>
      ))}
      <input disabled />
    </ul>
  );
}

const done = 1;
"####;

const SAMPLES: &[(&str, &str)] = &[
    ("go", GO),
    ("java", JAVA),
    ("php", PHP),
    ("kt", KOTLIN),
    ("swift", SWIFT),
    ("lua", LUA),
    ("hs", HASKELL),
    ("scala", SCALA),
    ("cs", CS),
    ("c", C),
    ("cpp", CPP),
    ("rb", RUBY),
    ("pl", PERL),
    ("r", R),
    ("sql", SQL),
    ("sh", SH),
    ("make", MAKE),
    ("tex", TEX),
    ("toml", TOML),
    ("yaml", YAML),
    ("css", CSS),
    ("scss", SCSS),
    ("less", LESS),
    ("html", HTML),
    ("xml", XML),
    ("json", JSON),
    ("md", MD),
    ("diff", DIFF),
    ("rs", RS),
    ("py", PY),
    ("js", JS),
    ("jsx", JSX),
    ("ts", TS),
    ("tsx", TSX),
];

/// A scope that swallows what follows when it never ends.
fn swallowing(scope: &parsing::Scope) -> bool {
    let name = scope.build_string();
    ["string.", "comment.", "regexp"]
        .iter()
        .any(|kind| name.contains(kind))
}

#[test]
fn every_listed_language_has_a_sample() {
    for (token, name) in crate::editor::SUPPORTED_LANGUAGES {
        assert!(
            SAMPLES.iter().any(|(t, _)| t == token),
            "no sample for {name} ({token})"
        );
    }
}

#[test]
fn a_tricky_program_in_each_language_ends_outside_any_string_or_comment() {
    let colors = crate::theme::syntax(true);
    let mut failures = Vec::new();
    for (token, text) in SAMPLES {
        let mut h = Highlighter::new(&Settings {
            dark: true,
            extension: token.to_string(),
        });
        let mut last_line: Vec<(String, Option<Color>)> = Vec::new();
        for line in text.lines() {
            last_line = h
                .highlight_line(line)
                .map(|(r, hl)| (line[r].to_string(), hl.0))
                .collect();
        }
        let open: Vec<String> = h
            .caches
            .last()
            .expect("a cache")
            .1
            .scopes
            .iter()
            .filter(|s| swallowing(s))
            .map(|s| s.build_string())
            .collect();
        if !open.is_empty() {
            failures.push(format!("{token}: ends inside {open:?}"));
        }
        let swallowed = last_line.iter().any(|(t, c)| {
            !t.trim().is_empty() && (*c == Some(colors.string) || *c == Some(colors.comment))
        });
        // The last line is plain code or data, except where a bare value or
        // an attribute is a string by the grammar's own account.
        if swallowed && !["sh", "make", "html"].contains(token) {
            failures.push(format!(
                "{token}: the last line is colored as a string or comment: {last_line:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
