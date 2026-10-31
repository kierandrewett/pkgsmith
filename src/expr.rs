//! The expression language of `${{ ... }}` and `if:`, modelled on GitHub Actions: contexts (`matrix.arch`,
//! `env.NAME`, `format`, `distro`, `arch`), string, number and boolean literals, `== != && || !`, parentheses, and the
//! functions `contains`, `startsWith`, `endsWith`, `format`, `lower` and `upper`. String comparison ignores case.

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
}

impl Value {
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
        }
    }

    pub fn text(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n) if n.fract() == 0.0 => format!("{}", *n as i64),
            Value::Num(n) => n.to_string(),
            Value::Str(s) => s.clone(),
        }
    }

    fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Null, other) | (other, Value::Null) => !other.truthy(),
            (Value::Num(a), Value::Num(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (a, b) => a.text().eq_ignore_ascii_case(&b.text()),
        }
    }
}

/// Where context names (`matrix.arch`) get their values.
pub trait Lookup {
    fn lookup(&self, name: &str) -> Value;
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Name(String),
    Str(String),
    Num(f64),
    Open,
    Close,
    Comma,
    Not,
    Eq,
    Ne,
    And,
    Or,
}

fn tokens(text: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' => i += 1,
            '(' => (out.push(Token::Open), i += 1).1,
            ')' => (out.push(Token::Close), i += 1).1,
            ',' => (out.push(Token::Comma), i += 1).1,
            '\'' => {
                let mut value = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => bail!("unterminated string in expression {text:?}"),
                        Some('\'') if chars.get(i + 1) == Some(&'\'') => {
                            (value.push('\''), i += 2).1
                        }
                        Some('\'') => break,
                        Some(&ch) => (value.push(ch), i += 1).1,
                    }
                }
                i += 1;
                out.push(Token::Str(value));
            }
            '!' if chars.get(i + 1) == Some(&'=') => (out.push(Token::Ne), i += 2).1,
            '!' => (out.push(Token::Not), i += 1).1,
            '=' if chars.get(i + 1) == Some(&'=') => (out.push(Token::Eq), i += 2).1,
            '&' if chars.get(i + 1) == Some(&'&') => (out.push(Token::And), i += 2).1,
            '|' if chars.get(i + 1) == Some(&'|') => (out.push(Token::Or), i += 2).1,
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                out.push(Token::Num(
                    chars[start..i].iter().collect::<String>().parse()?,
                ));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || "_.-".contains(chars[i]))
                {
                    i += 1;
                }
                out.push(Token::Name(chars[start..i].iter().collect()));
            }
            other => bail!("unexpected {other:?} in expression {text:?}"),
        }
    }
    Ok(out)
}

struct Parser<'a, L: Lookup> {
    tokens: Vec<Token>,
    at: usize,
    lookup: &'a L,
}

impl<L: Lookup> Parser<'_, L> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn take(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        self.at += 1;
        token
    }

    fn or(&mut self) -> Result<Value> {
        let mut left = self.and()?;
        while self.peek() == Some(&Token::Or) {
            self.take();
            let right = self.and()?;
            left = if left.truthy() { left } else { right };
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Value> {
        let mut left = self.comparison()?;
        while self.peek() == Some(&Token::And) {
            self.take();
            let right = self.comparison()?;
            left = if left.truthy() { right } else { left };
        }
        Ok(left)
    }

    fn comparison(&mut self) -> Result<Value> {
        let left = self.unary()?;
        match self.peek() {
            Some(Token::Eq) => {
                self.take();
                Ok(Value::Bool(left.equals(&self.unary()?)))
            }
            Some(Token::Ne) => {
                self.take();
                Ok(Value::Bool(!left.equals(&self.unary()?)))
            }
            _ => Ok(left),
        }
    }

    fn unary(&mut self) -> Result<Value> {
        if self.peek() == Some(&Token::Not) {
            self.take();
            return Ok(Value::Bool(!self.unary()?.truthy()));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Value> {
        match self.take() {
            Some(Token::Str(s)) => Ok(Value::Str(s)),
            Some(Token::Num(n)) => Ok(Value::Num(n)),
            Some(Token::Open) => {
                let value = self.or()?;
                match self.take() {
                    Some(Token::Close) => Ok(value),
                    _ => bail!("missing ) in expression"),
                }
            }
            Some(Token::Name(name)) => match name.as_str() {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                "null" => Ok(Value::Null),
                _ if self.peek() == Some(&Token::Open) => {
                    self.take();
                    let mut arguments = Vec::new();
                    if self.peek() != Some(&Token::Close) {
                        loop {
                            arguments.push(self.or()?);
                            match self.take() {
                                Some(Token::Comma) => continue,
                                Some(Token::Close) => break,
                                _ => bail!("missing ) after the arguments of {name}"),
                            }
                        }
                    } else {
                        self.take();
                    }
                    call(&name, &arguments)
                }
                _ => Ok(self.lookup.lookup(&name)),
            },
            other => bail!("unexpected {other:?} in expression"),
        }
    }
}

fn call(name: &str, arguments: &[Value]) -> Result<Value> {
    let text = |i: usize| arguments.get(i).map(Value::text).unwrap_or_default();
    let need = |n: usize| -> Result<()> {
        if arguments.len() < n {
            bail!("{name}() needs {n} arguments")
        } else {
            Ok(())
        }
    };
    Ok(match name.to_ascii_lowercase().as_str() {
        "contains" => {
            need(2)?;
            Value::Bool(
                text(0)
                    .to_ascii_lowercase()
                    .contains(&text(1).to_ascii_lowercase()),
            )
        }
        "startswith" => {
            need(2)?;
            Value::Bool(
                text(0)
                    .to_ascii_lowercase()
                    .starts_with(&text(1).to_ascii_lowercase()),
            )
        }
        "endswith" => {
            need(2)?;
            Value::Bool(
                text(0)
                    .to_ascii_lowercase()
                    .ends_with(&text(1).to_ascii_lowercase()),
            )
        }
        "lower" => {
            need(1)?;
            Value::Str(text(0).to_ascii_lowercase())
        }
        "upper" => {
            need(1)?;
            Value::Str(text(0).to_ascii_uppercase())
        }
        "format" => {
            need(1)?;
            let mut out = text(0);
            for (i, argument) in arguments.iter().enumerate().skip(1) {
                out = out.replace(&format!("{{{}}}", i - 1), &argument.text());
            }
            Value::Str(out)
        }
        other => bail!("unknown function {other}()"),
    })
}

pub fn eval(expression: &str, lookup: &impl Lookup) -> Result<Value> {
    let mut parser = Parser {
        tokens: tokens(expression)?,
        at: 0,
        lookup,
    };
    let value = parser.or()?;
    if parser.at < parser.tokens.len() {
        bail!("unexpected input after the expression {expression:?}");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Names(BTreeMap<&'static str, &'static str>);

    impl Lookup for Names {
        fn lookup(&self, name: &str) -> Value {
            self.0
                .get(name)
                .map(|v| Value::Str((*v).into()))
                .unwrap_or(Value::Null)
        }
    }

    fn names() -> Names {
        Names(BTreeMap::from([
            ("matrix.arch", "amd64"),
            ("format", "deb"),
            ("distro", "Fedora"),
        ]))
    }

    fn truth(expression: &str) -> bool {
        eval(expression, &names()).unwrap().truthy()
    }

    #[test]
    fn comparisons_ignore_case_and_combine_with_and_or_not() {
        assert!(truth("matrix.arch == 'AMD64'"));
        assert!(truth("distro == 'fedora' && format != 'rpm'"));
        assert!(truth("format == 'rpm' || matrix.arch == 'amd64'"));
        assert!(truth("!(format == 'rpm')"));
        assert!(!truth("matrix.arch == 'arm64' || matrix.missing"));
    }

    #[test]
    fn functions_work_on_text() {
        assert!(truth("contains(distro, 'FED')"));
        assert!(truth("startsWith(format, 'de') && endsWith(format, 'eb')"));
        assert_eq!(
            eval("format('{0}-{1}', matrix.arch, format)", &names())
                .unwrap()
                .text(),
            "amd64-deb"
        );
        assert_eq!(eval("upper(format)", &names()).unwrap().text(), "DEB");
    }

    #[test]
    fn and_or_return_a_value_like_actions_do() {
        let value = eval("matrix.arch == 'amd64' && 'x86_64' || 'aarch64'", &names()).unwrap();
        assert_eq!(value.text(), "x86_64");
        let value = eval("matrix.arch == 'arm64' && 'x86_64' || 'aarch64'", &names()).unwrap();
        assert_eq!(value.text(), "aarch64");
    }

    #[test]
    fn mistakes_are_errors_not_silent_false() {
        for bad in ["format ==", "(format", "nosuch(1)", "'open", "a & b"] {
            assert!(eval(bad, &names()).is_err(), "{bad}");
        }
    }
}
