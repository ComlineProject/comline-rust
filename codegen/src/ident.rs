//! Schema names as Rust identifiers, valid in every edition.
//!
//! Core accepts any name the IDL can spell, and a Rust keyword is one: a
//! field named `match`, a function named `loop`, an enum variant `Self`.
//! Code-mode output compiles inside the consumer's crate, under whatever
//! edition it uses, so a keyword of *any* edition is escaped: the strict
//! ones, `async` / `await` / `dyn` / `try` (2018) and `gen` (2024), and the
//! reserved ones. Output then compiles in every edition, and no edition
//! needs to be configured.
//!
//! - Most become raw identifiers, `r#match`. Serde strips the `r#`, so the
//!   wire name doesn't change.
//! - `self`, `Self`, `super` and `crate` can't be raw. They take a trailing
//!   underscore (`self_`), and a field or variant keeps its wire name with
//!   `#[serde(rename = "self")]` ([`serde_rename`]).

/// Every keyword, strict or reserved, of every edition.
const KEYWORDS: &[&str] = &[
    // strict, 2015
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
    "trait", "true", "type", "unsafe", "use", "where", "while",
    // strict from 2018
    "async", "await", "dyn",
    // reserved
    "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield",
    // reserved from 2018 and 2024
    "try", "gen",
];

/// The keywords a raw identifier can't spell.
const NOT_RAW: &[&str] = &["self", "Self", "super", "crate"];

/// `name` as a Rust identifier.
pub fn ident(name: &str) -> String {
    if NOT_RAW.contains(&name) {
        format!("{name}_")
    } else if KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// `#[serde(rename = "…")]` for a field or variant whose identifier
/// ([`ident`]) isn't its wire name to serde, indented by `indent`; empty
/// otherwise.
pub fn serde_rename(name: &str, indent: &str) -> String {
    if NOT_RAW.contains(&name) {
        format!("{indent}#[serde(rename = \"{name}\")]\n")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_of_every_edition_are_escaped() {
        assert_eq!(ident("match"), "r#match");
        assert_eq!(ident("async"), "r#async");
        assert_eq!(ident("gen"), "r#gen");
        assert_eq!(ident("self"), "self_");
        assert_eq!(ident("Self"), "Self_");
        assert_eq!(ident("name"), "name");
        assert_eq!(ident("union"), "union", "a weak keyword is a valid identifier");
    }

    #[test]
    fn only_the_underscored_ones_need_a_rename() {
        assert_eq!(serde_rename("match", "    "), "");
        assert_eq!(serde_rename("self", "    "), "    #[serde(rename = \"self\")]\n");
    }
}
