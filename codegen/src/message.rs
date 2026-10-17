//! Turn an `error`'s `message` template into a Rust format string.
//!
//! The IDL requires a `message` on every `error` and specifies what it means:
//! `{dotted.path}` substitutes a value, `{{` / `}}` are escaped literal
//! braces. Nothing had ever rendered it — the generator read the fields and
//! dropped the sentence the author wrote — so a generated error had no
//! `Display` and printed as `NotFound { key: "k" }` at best.
//!
//! `{self.<field>}` is the only path that can resolve: the error's own fields
//! are all the data a raised error carries, and binding anything else in at
//! the raise site is not in the language yet.
//!
//! ## Why an unknown path becomes literal text rather than an error
//!
//! The frozen template cannot be trusted to distinguish a placeholder from an
//! escaped brace. `InterpolatedString::reconstruct` in core re-inserts a
//! placeholder as `{a.b.c}` but collapses an escaped `{{` / `}}` down to a
//! *bare* `{` / `}`, so
//!
//! ```text
//! message = "literal {{brace}} here"   ->   literal {brace} here
//! ```
//!
//! arrives here indistinguishable from a real placeholder. Treating a path
//! that is not one of this error's fields as ordinary text recovers the
//! author's intent in exactly that case, and is the forgiving choice in every
//! other: a message is diagnostic text, and refusing to generate over one is
//! worse than printing it as written. Fixing `reconstruct` moves the frozen
//! IR and therefore the schema hash, so it is its own change.
//!
//! The upshot is that a template arriving here holds no escape sequences of
//! its own: `reconstruct` has already resolved them. There are only
//! `{path}` placeholders and literal text, and that literal text may contain
//! bare braces — which this module re-escapes on the way into a Rust format
//! string.

/// A rendered template: a Rust format string and the argument expressions it
/// interpolates, in order.
#[derive(Debug, PartialEq, Eq)]
pub struct Rendered {
    pub format: String,
    pub args: Vec<String>,
}

impl Rendered {
    /// The body of a `Display::fmt`, given the formatter's name.
    ///
    /// With no arguments this is a `write_str`, which avoids running the
    /// format machinery (and clippy's `write_literal`) over a constant.
    pub fn write_call(&self, formatter: &str) -> String {
        if self.args.is_empty() {
            format!("{formatter}.write_str({:?})", self.format)
        } else {
            format!(
                "write!({formatter}, {:?}, {})",
                self.format,
                self.args.join(", ")
            )
        }
    }
}

/// Render `template` against the names of the error's own fields.
pub fn render(template: &str, fields: &[String]) -> Rendered {
    let mut format = String::new();
    let mut args: Vec<String> = Vec::new();
    let mut rest = template;

    while let Some(open) = rest.find('{') {
        push_literal(&mut format, &rest[..open]);
        rest = &rest[open..];

        let Some(close) = rest.find('}') else {
            // Unterminated: the rest is literal text.
            push_literal(&mut format, rest);
            return Rendered { format, args };
        };

        let path = &rest[1..close];
        match path.strip_prefix("self.") {
            Some(field) if fields.iter().any(|f| f == field) => {
                format.push_str("{}");
                args.push(format!("self.{field}"));
            }
            // Not one of this error's fields — see the module docs.
            _ => push_literal(&mut format, &rest[..=close]),
        }
        rest = &rest[close + 1..];
    }

    push_literal(&mut format, rest);
    Rendered { format, args }
}

/// Append literal text, escaping the braces Rust's formatter would otherwise
/// read as placeholders of its own.
fn push_literal(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            other => out.push(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_field_placeholder_becomes_an_argument() {
        let r = render("no entry for {self.key}", &fields(&["key"]));
        assert_eq!(r.format, "no entry for {}");
        assert_eq!(r.args, vec!["self.key"]);
        assert_eq!(
            r.write_call("f"),
            r#"write!(f, "no entry for {}", self.key)"#
        );
    }

    #[test]
    fn several_placeholders_keep_their_order() {
        let r = render("{self.a} then {self.b}", &fields(&["a", "b"]));
        assert_eq!(r.format, "{} then {}");
        assert_eq!(r.args, vec!["self.a", "self.b"]);
    }

    #[test]
    fn a_message_with_no_placeholders_is_a_write_str() {
        let r = render("flatly refused", &fields(&[]));
        assert!(r.args.is_empty());
        assert_eq!(r.write_call("f"), r#"f.write_str("flatly refused")"#);
    }

    /// The `reconstruct` collapse: `{{brace}}` reaches us as `{brace}`, and
    /// `brace` is not a field, so it prints as the author wrote it.
    #[test]
    fn a_path_that_is_not_a_field_stays_literal_text() {
        let r = render("literal {brace} here", &fields(&["key"]));
        assert!(r.args.is_empty());
        assert_eq!(r.format, "literal {{brace}} here");
        assert_eq!(
            r.write_call("f"),
            r#"f.write_str("literal {{brace}} here")"#
        );
    }

    /// A field the error does not declare is not an argument either — the
    /// generated code would not compile if it were.
    #[test]
    fn an_unknown_self_field_stays_literal_text() {
        let r = render("{self.missing}", &fields(&["key"]));
        assert!(r.args.is_empty());
        assert_eq!(r.format, "{{self.missing}}");
    }

    /// Core resolves the IDL's `{{` / `}}` escapes before freezing, so this
    /// module never sees one. Every brace that reaches it is either a
    /// placeholder's or literal text, and literal text is re-escaped for
    /// Rust's formatter — so the rendered output matches the message the
    /// author wrote, braces and all.
    #[test]
    fn a_literal_brace_round_trips_through_the_format_string() {
        let r = render("a { and a }", &fields(&[]));
        assert!(r.args.is_empty());
        assert_eq!(r.format, "a {{ and a }}");
        // What `write_str` actually prints is the original text.
        assert_eq!(r.write_call("f"), r#"f.write_str("a {{ and a }}")"#);
    }

    #[test]
    fn an_unterminated_brace_is_literal() {
        let r = render("open {self.key", &fields(&["key"]));
        assert!(r.args.is_empty());
        assert_eq!(r.format, "open {{self.key");
    }

    #[test]
    fn a_quote_in_a_message_is_escaped_by_the_debug_formatting() {
        let r = render("she said \"no\"", &fields(&[]));
        assert_eq!(r.write_call("f"), r#"f.write_str("she said \"no\"")"#);
    }
}
