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
//! ## The template's own escapes
//!
//! The frozen template is `{path}` placeholders and literal text, with a
//! literal brace written `{{` / `}}`. An f-string's escapes arrive that
//! way, and so does every brace of a plain `"..."` message, which never
//! interpolates. So a single `{` always opens a placeholder.
//!
//! ## Why an unknown path still becomes literal text rather than an error
//!
//! A message is diagnostic text, and refusing to generate over one is worse
//! than printing it as written. A path that names nothing this error (or
//! validator) can bind prints as the author wrote it.

/// A rendered template: a Rust format string and the argument expressions it
/// interpolates, in order.
#[derive(Debug, PartialEq, Eq)]
pub struct Rendered {
    pub format: String,
    pub args: Vec<String>,
}

impl Rendered {
    /// The message as it prints, for a template with no arguments: the
    /// format string with its doubled braces single again. A string used
    /// as is (`write_str`, `.into()`) would print them doubled.
    pub fn literal(&self) -> String {
        self.format.replace("{{", "{").replace("}}", "}")
    }

    /// The body of a `Display::fmt`, given the formatter's name.
    ///
    /// With no arguments this is a `write_str`, which avoids running the
    /// format machinery (and clippy's `write_literal`) over a constant.
    pub fn write_call(&self, formatter: &str) -> String {
        if self.args.is_empty() {
            format!("{formatter}.write_str({:?})", self.literal())
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
    render_with(template, |path| match path.strip_prefix("self.") {
        Some(field) if fields.iter().any(|f| f == field) => Some(format!("self.{}", crate::ident::ident(field))),
        _ => None,
    })
}

/// Render `template`, turning each `{path}` into the argument expression
/// `resolve` gives for it. A path `resolve` doesn't know stays literal text,
/// for the reason in the module docs.
pub fn render_with(template: &str, resolve: impl Fn(&str) -> Option<String>) -> Rendered {
    let mut format = String::new();
    let mut args: Vec<String> = Vec::new();
    let mut rest = template;

    while let Some(at) = rest.find(['{', '}']) {
        push_literal(&mut format, &rest[..at]);
        rest = &rest[at..];

        // `{{` / `}}`: a literal brace.
        if let Some(after) = rest.strip_prefix("{{").or_else(|| rest.strip_prefix("}}")) {
            push_literal(&mut format, &rest[..1]);
            rest = after;
            continue;
        }
        // A lone `}` is text too.
        if rest.starts_with('}') {
            push_literal(&mut format, "}");
            rest = &rest[1..];
            continue;
        }

        let Some(close) = rest.find('}') else {
            // Unterminated: the rest is literal text.
            push_literal(&mut format, rest);
            return Rendered { format, args };
        };

        match resolve(&rest[1..close]) {
            Some(arg) => {
                format.push_str("{}");
                args.push(arg);
            }
            // Nothing it names - see the module docs.
            None => push_literal(&mut format, &rest[..=close]),
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

    /// A path naming nothing it can bind prints as the author wrote it.
    #[test]
    fn a_path_that_is_not_a_field_stays_literal_text() {
        let r = render("literal {brace} here", &fields(&["key"]));
        assert!(r.args.is_empty());
        assert_eq!(r.format, "literal {{brace}} here");
        assert_eq!(r.write_call("f"), r#"f.write_str("literal {brace} here")"#);
    }

    /// A field the error does not declare is not an argument either — the
    /// generated code would not compile if it were.
    #[test]
    fn an_unknown_self_field_stays_literal_text() {
        let r = render("{self.missing}", &fields(&["key"]));
        assert!(r.args.is_empty());
        assert_eq!(r.format, "{{self.missing}}");
    }

    /// `{{` / `}}` in the template are literal braces, never a placeholder,
    /// even around a field's own path - and `write_str` prints them single.
    #[test]
    fn an_escaped_brace_is_literal_even_around_a_field() {
        let r = render("{{self.key}} is {self.key}", &fields(&["key"]));
        assert_eq!(r.format, "{{self.key}} is {}");
        assert_eq!(r.args, vec!["self.key"]);

        // A plain message's braces all arrive escaped.
        let r = render("a {{ and a }}", &fields(&[]));
        assert!(r.args.is_empty());
        assert_eq!(r.format, "a {{ and a }}");
        assert_eq!(r.write_call("f"), r#"f.write_str("a { and a }")"#);
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
