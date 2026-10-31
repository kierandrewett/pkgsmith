//! Text expansion for manifest fields: `${{ expression }}` (see `expr`) and the shell forms `${NAME}` and
//! `${NAME:-default}` for variables.

use crate::expr::{self, Lookup, Value};
use anyhow::{Result, bail};
use std::collections::BTreeMap;

/// What a manifest can see while one job (one matrix combination) is built.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// The environment, then `--set NAME=value`, then the manifest's own `env:` for names still unset.
    pub env: BTreeMap<String, String>,
    pub matrix: BTreeMap<String, String>,
    pub format: String,
    pub distro: String,
    pub arch: String,
}

impl Lookup for Scope {
    fn lookup(&self, name: &str) -> Value {
        let text =
            |value: Option<&String>| value.map(|v| Value::Str(v.clone())).unwrap_or(Value::Null);
        match name.split_once('.') {
            Some(("matrix", key)) => text(self.matrix.get(key)),
            Some(("env", key)) => text(self.env.get(key)),
            None => match name {
                "format" => Value::Str(self.format.clone()),
                "distro" => Value::Str(self.distro.clone()),
                "arch" => Value::Str(self.arch.clone()),
                _ => Value::Null,
            },
            _ => Value::Null,
        }
    }
}

impl Scope {
    pub fn from_environment() -> Scope {
        Scope {
            env: std::env::vars().collect(),
            ..Scope::default()
        }
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.env.insert(name.into(), value.into());
    }

    /// Replace every `${{ expression }}`, `${NAME}` and `${NAME:-default}`. A `${NAME}` that is unset and has no
    /// default is an error, so a typo cannot produce an empty path or version.
    pub fn expand(&self, text: &str) -> Result<String> {
        self.expand_with(text, true)
    }

    /// Like `expand`, but leave `${NAME}` alone. Scripts use it: `${HOME}` in a script belongs to the shell.
    pub fn expand_expressions(&self, text: &str) -> Result<String> {
        self.expand_with(text, false)
    }

    fn expand_with(&self, text: &str, variables: bool) -> Result<String> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            if let Some(expression) = after.strip_prefix('{') {
                let Some(end) = expression.find("}}") else {
                    bail!("unterminated ${{{{ in {text:?}")
                };
                out.push_str(&expr::eval(expression[..end].trim(), self)?.text());
                rest = &expression[end + 2..];
                continue;
            }
            if !variables {
                out.push_str("${");
                rest = after;
                continue;
            }
            let Some(end) = after.find('}') else {
                bail!("unterminated ${{ in {text:?}")
            };
            let body = &after[..end];
            let (name, default) = match body.split_once(":-") {
                Some((name, default)) => (name, Some(default)),
                None => (body, None),
            };
            match (self.env.get(name), default) {
                (Some(value), _) if !value.is_empty() => out.push_str(value),
                (_, Some(default)) => out.push_str(default),
                _ => bail!(
                    "${{{name}}} is not set (in {text:?}); set it in the environment or with --set {name}=VALUE"
                ),
            }
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Evaluate an `if:` expression. A `${{ }}` wrapper is allowed and ignored.
    pub fn test(&self, expression: &str) -> Result<bool> {
        let trimmed = expression.trim();
        let inner = trimmed
            .strip_prefix("${{")
            .and_then(|t| t.strip_suffix("}}"))
            .unwrap_or(trimmed);
        Ok(expr::eval(inner.trim(), self)?.truthy())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Scope {
        let mut context = Scope::default();
        context.set("STAGE", "/tmp/stage");
        context.matrix.insert("arch".into(), "arm64".into());
        context.format = "rpm".into();
        context
    }

    #[test]
    fn expands_variables_defaults_and_expressions() {
        let context = context();
        assert_eq!(
            context.expand("${STAGE}/usr/bin").unwrap(),
            "/tmp/stage/usr/bin"
        );
        assert_eq!(context.expand("${ARCH:-amd64}").unwrap(), "amd64");
        assert_eq!(
            context.expand("${{ matrix.arch }}/lib").unwrap(),
            "arm64/lib"
        );
        assert_eq!(
            context
                .expand("${{ matrix.arch == 'arm64' && 'aarch64' || 'x86_64' }}")
                .unwrap(),
            "aarch64"
        );
        assert_eq!(context.expand("plain").unwrap(), "plain");
    }

    #[test]
    fn an_unset_variable_is_an_error_that_names_it() {
        let error = Scope::default()
            .expand("${VERSION}")
            .unwrap_err()
            .to_string();
        assert!(error.contains("VERSION"), "{error}");
    }

    #[test]
    fn if_accepts_a_bare_expression_or_the_wrapped_form() {
        let context = context();
        assert!(context.test("format == 'rpm'").unwrap());
        assert!(context.test("${{ matrix.arch == 'arm64' }}").unwrap());
        assert!(!context.test("format == 'deb'").unwrap());
    }
}
