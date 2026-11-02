//! Names generated code uses that a schema can also declare.
//!
//! A module uses names it doesn't declare: the prelude's (`Option`, `Vec`,
//! `Result`, `Ok`), serde's derives, and what it imports from
//! `comline_runtime` (`Call`, `Connection`, `Peer`). A schema can declare a
//! type of the same name, and keeps it (comline-rust#96). In a module where
//! one does, the import is left out and every use is written as the full
//! path instead (`::comline_runtime::contract::Call`); elsewhere the module
//! is generated as it always was.
//!
//! The templates mark each such use as `$Name`, which nothing else in Rust
//! output contains; [`resolve`] writes each mark out once a module's own
//! names are known.

use std::collections::HashSet;

/// Every name a template marks, and the path that always reaches it.
const PATHS: &[(&str, &str)] = &[
    ("Option", "::core::option::Option"),
    ("Some", "::core::option::Option::Some"),
    ("None", "::core::option::Option::None"),
    ("Result", "::core::result::Result"),
    ("Ok", "::core::result::Result::Ok"),
    ("Err", "::core::result::Result::Err"),
    ("Vec", "::std::vec::Vec"),
    ("String", "::std::string::String"),
    ("Box", "::std::boxed::Box"),
    ("Serialize", "::serde::Serialize"),
    ("Deserialize", "::serde::Deserialize"),
    ("Client", "::comline_runtime::client::Client"),
    ("Answer", "::comline_runtime::async_connection::Answer"),
    ("AsyncConnection", "::comline_runtime::async_connection::AsyncConnection"),
    ("AsyncDispatch", "::comline_runtime::async_connection::AsyncDispatch"),
    ("AsyncPeer", "::comline_runtime::async_connection::AsyncPeer"),
    ("AsyncTransport", "::comline_runtime::async_connection::AsyncTransport"),
    ("Connection", "::comline_runtime::connection::Connection"),
    ("Call", "::comline_runtime::contract::Call"),
    ("CallError", "::comline_runtime::contract::CallError"),
    ("ConnectionFraming", "::comline_runtime::contract::ConnectionFraming"),
    ("Dispatch", "::comline_runtime::contract::Dispatch"),
    ("Envelope", "::comline_runtime::contract::Envelope"),
    ("Fault", "::comline_runtime::contract::Fault"),
    ("Kind", "::comline_runtime::contract::Kind"),
    ("Peer", "::comline_runtime::contract::Peer"),
    ("ProtocolNumber", "::comline_runtime::contract::ProtocolNumber"),
    ("Reply", "::comline_runtime::contract::Reply"),
    ("RuntimeError", "::comline_runtime::contract::RuntimeError"),
    ("WireFormat", "::comline_runtime::contract::WireFormat"),
    ("Server", "::comline_runtime::serve::Server"),
    ("Transport", "::comline_runtime::transport::Transport"),
];

/// `text` with every `$Name` mark written out: as the bare name, unless
/// `taken` (the names the module declares) holds it, then as its full path.
pub fn resolve(text: &str, taken: &HashSet<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let len = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
        let name = &after[..len];
        match PATHS.iter().find(|(marked, _)| *marked == name) {
            Some((_, path)) if taken.contains(name) => out.push_str(path),
            Some(_) => out.push_str(name),
            None => out.push('$'),
        }
        rest = if PATHS.iter().any(|(marked, _)| *marked == name) { &after[len..] } else { after };
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_is_the_bare_name_unless_the_module_takes_it() {
        let taken: HashSet<String> = ["Call".to_string()].into();
        assert_eq!(
            resolve("fn f(c: $Call, k: $Kind) -> $Result<(), $RuntimeError>", &taken),
            "fn f(c: ::comline_runtime::contract::Call, k: Kind) -> Result<(), RuntimeError>"
        );
        // Not a mark: left as written.
        assert_eq!(resolve("$x and $", &taken), "$x and $");
    }
}
