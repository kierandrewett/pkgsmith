//! A small writer for the Nix language: values built as a tree and printed with one layout, so no expression is made by
//! pasting strings together.
//!
//! Specifications used:
//! - String literals and escapes: https://nix.dev/manual/nix/stable/language/string-literals.html
//! - Attribute names: https://nix.dev/manual/nix/stable/language/syntax.html

use std::fmt::Write;

/// A Nix value.
pub enum Expr {
    /// A string, written with `"` and the escapes of the language.
    Str(String),
    /// A multi-line string in `''`, for a shell script. `$out` and the other shell variables stay as they are.
    Script(String),
    /// Nix code that the caller wrote and trusts, such as `lib.platforms.all` or `./files`.
    Raw(String),
    Bool(bool),
    List(Vec<Expr>),
    Attrs(Vec<(String, Expr)>),
}

/// A string literal. `${` would start an interpolation, so its `$` is escaped.
pub fn string(text: &str) -> String {
    let mut out = String::from("\"");
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Whether `name` can be written as an attribute name or argument without quotes.
pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '\''))
        && !matches!(
            name,
            "if" | "then" | "else" | "assert" | "with" | "let" | "in" | "rec" | "inherit" | "or"
        )
}

/// An attribute name, quoted when it must be.
pub fn attribute_name(name: &str) -> String {
    if is_identifier(name) {
        name.to_owned()
    } else {
        string(name)
    }
}

impl Expr {
    pub fn strings<I, S>(items: I) -> Expr
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Expr::List(
            items
                .into_iter()
                .map(|s| Expr::Str(s.as_ref().to_owned()))
                .collect(),
        )
    }

    pub fn raws<I, S>(items: I) -> Expr
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Expr::List(
            items
                .into_iter()
                .map(|s| Expr::Raw(s.as_ref().to_owned()))
                .collect(),
        )
    }

    /// The expression at `depth` levels of indentation, two spaces each. The first line has no indentation.
    pub fn render(&self, depth: usize) -> String {
        let pad = "  ".repeat(depth);
        match self {
            Expr::Str(text) => string(text),
            Expr::Raw(code) => code.clone(),
            Expr::Bool(value) => value.to_string(),
            Expr::List(items) if items.is_empty() => "[ ]".into(),
            Expr::List(items) => {
                let short = items
                    .iter()
                    .all(|i| !matches!(i, Expr::Attrs(_) | Expr::Script(_)));
                let one_line = format!(
                    "[ {} ]",
                    items
                        .iter()
                        .map(|i| i.render(depth))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                if short && one_line.len() + pad.len() <= 100 {
                    return one_line;
                }
                let mut out = String::from("[\n");
                for item in items {
                    let _ = writeln!(out, "{pad}  {}", item.render(depth + 1));
                }
                let _ = write!(out, "{pad}]");
                out
            }
            Expr::Attrs(pairs) if pairs.is_empty() => "{ }".into(),
            Expr::Attrs(pairs) => {
                let mut out = String::from("{\n");
                for (key, value) in pairs {
                    let _ = writeln!(
                        out,
                        "{pad}  {} = {};",
                        attribute_name(key),
                        value.render(depth + 1)
                    );
                }
                let _ = write!(out, "{pad}}}");
                out
            }
            Expr::Script(text) => {
                let mut out = String::from("''\n");
                for line in text.lines() {
                    if line.is_empty() {
                        out.push('\n');
                    } else {
                        // `''` ends the string and `${` interpolates, so both get the escape of an indented string.
                        let line = line.replace("''", "'''").replace("${", "''${");
                        let _ = writeln!(out, "{pad}  {line}");
                    }
                }
                let _ = write!(out, "{pad}''");
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_literals_escape_what_nix_would_expand() {
        assert_eq!(
            string("a \"b\" ${c} \\ $d"),
            "\"a \\\"b\\\" \\${c} \\\\ $d\""
        );
    }

    #[test]
    fn attribute_names_are_quoted_when_they_must_be() {
        assert_eq!(attribute_name("hi-there"), "hi-there");
        assert_eq!(attribute_name("1pass"), "\"1pass\"");
        assert_eq!(attribute_name("a.b"), "\"a.b\"");
        assert_eq!(attribute_name("rec"), "\"rec\"");
    }

    #[test]
    fn values_print_with_one_layout() {
        let value = Expr::Attrs(vec![
            ("pname".into(), Expr::Str("hi".into())),
            ("buildInputs".into(), Expr::raws(["openssl", "zlib"])),
            ("dontStrip".into(), Expr::Bool(true)),
            (
                "meta".into(),
                Expr::Attrs(vec![("platforms".into(), Expr::strings(["x86_64-linux"]))]),
            ),
            (
                "installPhase".into(),
                Expr::Script("echo '' ${x}\n\nls".into()),
            ),
        ]);
        let expected = "{\n  pname = \"hi\";\n  buildInputs = [ openssl zlib ];\n  dontStrip = true;\n  meta = {\n    platforms = [ \"x86_64-linux\" ];\n  };\n  installPhase = ''\n    echo ''' ''${x}\n\n    ls\n  '';\n}";
        assert_eq!(value.render(0), expected);
    }
}
