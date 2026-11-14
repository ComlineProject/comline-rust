//! Generated code names its entry points, hides its plumbing and spells its
//! types short (comline-rust#129): what a user of a package implements,
//! serves, dials and calls is said at the top of `connection.rs` (the crate
//! root, in `lib` mode); the wire plumbing stays `pub` for by-hand use but
//! drops out of rustdoc; the connection's type is written once.
//!
//! What stays the same is `wire_invariants.rs`'s business.

use comline_codegen::{GeneratedFile, Mode};
use comline_codegen_rust::generate_rust;
use comline_conformance::{corpus, Fixture};

mod common;
use common::{lockstep, request};

fn fixture(name: &str) -> Fixture {
    corpus()
        .into_iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no fixture `{name}`"))
}

fn files(fx: &Fixture, mode: Mode, async_api: bool) -> Vec<GeneratedFile> {
    generate_rust(&request(fx, mode, async_api)).unwrap_or_else(|e| panic!("{}: {e:#}", fx.name))
}

fn file<'a>(files: &'a [GeneratedFile], path: &str) -> &'a str {
    files
        .iter()
        .find(|f| f.path.to_str() == Some(path))
        .unwrap_or_else(|| {
            panic!(
                "no {path} in {:?}",
                files.iter().map(|f| &f.path).collect::<Vec<_>>()
            )
        })
        .contents
        .as_str()
}

/// The first line of `source` that isn't a plain `//` comment or blank.
fn first_item(source: &str) -> &str {
    source
        .lines()
        .find(|l| !l.trim().is_empty() && !(l.starts_with("//") && !l.starts_with("//!")))
        .unwrap()
}

/// Whether the item `source` has at `at` is `#[doc(hidden)]`: among the
/// attributes and docs written directly above it.
fn hidden_at(source: &str, at: usize) -> bool {
    source[..at]
        .trim_end_matches(' ')
        .lines()
        .rev()
        .take_while(|l| l.trim().starts_with("#[") || l.trim().starts_with("///"))
        .any(|l| l.trim() == "#[doc(hidden)]")
}

/// The `//!` header of `source`, as text.
fn header(source: &str) -> String {
    source
        .lines()
        .filter(|l| l.starts_with("//!"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_package_module_opens_with_what_to_use() {
    // A package that serves, and calls back: `datagram@2`, both sides.
    let both = files(&fixture("connections"), Mode::Code, false);
    let connection = file(&both, "connection.rs");
    assert!(first_item(connection).starts_with("//!"), "{connection}");
    let doc = header(connection);
    for expected in [
        "`conformance::Chat`",
        "`conformance::Events`",
        "[`serve`]`(transport, Services { chat: .. })`",
        "[`connect`]`(transport, ClientServices { events: .. })`",
        "`ChatSender::new(&mut conn).send(..)`",
        "`EventsSender::new(&mut conn).shown(..)`",
        "[`Connection<T>`](Connection)",
    ] {
        assert!(doc.contains(expected), "{expected}\n{doc}");
    }

    // One that only serves: nothing to pass `connect`.
    let served = files(&fixture("framing_datagram3"), Mode::Code, false);
    let doc = header(file(&served, "connection.rs"));
    assert!(doc.contains("[`connect`]`(transport)`"), "{doc}");
    assert!(doc.contains("`datagram@3` framing"), "{doc}");
    assert!(!doc.contains("ClientServices"), "{doc}");

    // Async: awaited, run, called through a handle.
    let asynchronous = files(&fixture("connections_async"), Mode::Code, true);
    let doc = header(file(&asynchronous, "connection.rs"));
    assert!(
        doc.contains("[`serve`]`(transport, Services { chat: .. }).await`"),
        "{doc}"
    );
    assert!(
        doc.contains("`ChatAsyncSender::new(&conn.handle()).send(..).await`"),
        "{doc}"
    );
    assert!(doc.contains("`run().await`"), "{doc}");

    // A nested protocol is named the way it's declared.
    let nested = files(&fixture("nested_protocol"), Mode::Code, false);
    let doc = header(file(&nested, "connection.rs"));
    assert!(doc.contains("`chat::admin::Admin`"), "{doc}");
    assert!(
        doc.contains("`AdminSender::new(&mut conn).greet(..)`"),
        "{doc}"
    );
}

#[test]
fn the_crate_root_of_a_lib_package_says_it_too_before_any_item() {
    for (name, async_api) in [
        ("connections", false),
        ("connections_async", true),
        ("nested_protocol", false),
    ] {
        let lib = files(&fixture(name), Mode::Lib, async_api);
        let root = file(&lib, "src/lib.rs");
        assert!(first_item(root).starts_with("//!"), "{name}\n{root}");
        assert!(header(root).contains("[`serve`]"), "{name}\n{root}");
    }
}

#[test]
fn a_package_without_a_connection_says_what_it_has_instead() {
    // Lockstep: the dispatcher serves, the client connects.
    let lockstep = files(&lockstep(fixture("protocol")), Mode::Lib, false);
    let root = file(&lockstep, "src/lib.rs");
    assert!(first_item(root).starts_with("//!"), "{root}");
    let doc = header(root);
    assert!(
        doc.contains("`conformance::ServiceDispatcher(provider).serve(&mut transport)`"),
        "{doc}"
    );
    assert!(
        doc.contains("`conformance::ServiceClient::connect(transport)?.lookup(..)`"),
        "{doc}"
    );
    assert!(!lockstep.iter().any(|f| f.path.ends_with("connection.rs")));

    // No protocol at all.
    let types = files(&fixture("primitives"), Mode::Lib, false);
    let root = file(&types, "src/lib.rs");
    assert!(first_item(root).starts_with("//!"), "{root}");
    assert!(header(root).contains("declares no protocols"), "{root}");
}

#[test]
fn the_wire_plumbing_is_hidden_and_the_entry_points_are_not() {
    let all = files(&fixture("nested_protocol"), Mode::Code, false);
    let admin = file(&all, "chat/admin.rs");
    let connection = file(&all, "connection.rs");
    let hidden = |source: &str, item: &str| {
        let at = source
            .find(item)
            .unwrap_or_else(|| panic!("no `{item}` in\n{source}"));
        hidden_at(source, at)
    };
    for item in [
        "pub struct AdminGreetParams",
        "pub const ADMIN_CALLS",
        "pub struct AdminDispatcher<S>",
    ] {
        assert!(hidden(admin, item), "{item} is plumbing\n{admin}");
    }
    // The sender's number, which `Services` and the sender use themselves.
    assert!(
        hidden(
            admin,
            "pub const PROTOCOL: ProtocolNumber = 0;\n\n    pub fn new"
        ),
        "{admin}"
    );
    for item in ["pub const HELLO", "pub fn handshake()", "pub fn check_call"] {
        assert!(hidden(connection, item), "{item} is plumbing\n{connection}");
    }
    // What a user touches stays in the docs.
    for (source, item) in [
        (admin, "pub trait Admin"),
        (admin, "pub struct AdminSender<"),
        (admin, "pub struct Notice"),
        (admin, "pub type AdminGreetError"),
        (connection, "pub struct Services<"),
        (connection, "pub fn serve<"),
        (connection, "pub fn accept<"),
        (connection, "pub fn connect<"),
        (connection, "pub type Connection<T>"),
        (connection, "pub type Format"),
        (connection, "pub fn negotiated<"),
    ] {
        assert!(!hidden(source, item), "{item} is an entry point\n{source}");
    }
}

#[test]
fn a_dispatcher_nobody_provides_for_you_stays_in_the_docs() {
    // On a connection with no package module to provide it (a dependency's
    // protocol is never on `Services`), and lockstep, where it's the entry.
    let lockstep = files(&lockstep(fixture("protocol")), Mode::Code, false);
    let source = file(&lockstep, "conformance.rs");
    let at = source.find("pub struct ServiceDispatcher<S>").unwrap();
    assert!(!hidden_at(source, at), "{source}");
    // Its params and call table are hidden all the same.
    assert!(source.contains("#[doc(hidden)]\n#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct ServiceLookupParams"));
    assert!(source.contains("#[doc(hidden)]\npub const SERVICE_CALLS"));

    let dependency = files(&fixture("dependency_and_own_protocols"), Mode::Code, false);
    let greeting = file(&dependency, "localdeps/greeting.rs");
    let at = greeting.find("pub struct FooDispatcher<S>").unwrap();
    assert!(!hidden_at(greeting, at), "{greeting}");
}

#[test]
fn the_runtime_is_imported_not_spelled_out_in_every_signature() {
    for (name, async_api) in [
        ("connections", false),
        ("connections_async", true),
        ("connections_jsonrpc", false),
        ("framing_datagram3", false),
        ("handshake_comline2", false),
        ("handshake_none", false),
        ("nested_protocol", false),
    ] {
        let all = files(&fixture(name), Mode::Code, async_api);
        let connection = file(&all, "connection.rs");
        // The signatures read short...
        for line in connection
            .lines()
            .filter(|l| l.starts_with("pub fn") || l.starts_with("pub async fn"))
        {
            assert!(!line.contains("comline_runtime::"), "{name}: {line}");
        }
        assert!(
            connection.contains("-> Result<Connection<T>, RuntimeError>"),
            "{name}\n{connection}"
        );
        assert!(
            connection.contains("use comline_runtime::contract::"),
            "{name}\n{connection}"
        );
        // ...and the connection's type is written in one place.
        let spelled = connection
            .lines()
            .filter(|l| l.contains("comline_runtime::connection::") && l.contains("Connection<T,"))
            .count();
        assert_eq!(spelled, 1, "{name}\n{connection}");
    }
}

#[test]
fn the_aliases_name_the_connection_the_package_runs_on() {
    let alias = |name: &str, async_api: bool| {
        let all = files(&fixture(name), Mode::Code, async_api);
        file(&all, "connection.rs")
            .lines()
            .filter(|l| l.starts_with("pub type"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        alias("framing_datagram3", false),
        "pub type Format = comline_runtime::format::Packed;\n\
         pub type Framing = comline_runtime::contract::Datagram3Framing;\n\
         pub type Connection<T> = comline_runtime::connection::Connection<T, Format, Framing>;"
    );
    assert_eq!(
        alias("connections_jsonrpc", false),
        "pub type Format = comline_runtime::format::Packed;\n\
         pub type Framing = comline_runtime::framing::JsonRpcFraming;\n\
         pub type Connection<T> = comline_runtime::connection::Connection<T, comline_runtime::format::Json, Framing>;"
    );
    // `datagram@2` is the runtime's default framing: nothing to name.
    assert_eq!(
        alias("connections", false),
        "pub type Format = comline_runtime::format::Packed;\n\
         pub type Connection<T> = comline_runtime::connection::Connection<T, Format>;"
    );
    assert_eq!(
        alias("connections_async", true),
        "pub type Format = comline_runtime::format::Packed;\n\
         pub type Connection<T> = comline_runtime::connection::AsyncConnection<T, Format>;"
    );
}

#[test]
fn format_and_the_digest_are_defined_once() {
    for mode in [Mode::Code, Mode::Lib] {
        let (package, protocol) = match mode {
            Mode::Code => ("connection.rs", "conformance.rs"),
            Mode::Lib => ("src/lib.rs", "src/conformance.rs"),
        };
        let all = files(&fixture("connections"), mode, false);
        let (package, protocol) = (file(&all, package), file(&all, protocol));
        assert_eq!(package.matches("pub type Format = ").count(), 1);
        assert_eq!(
            package
                .matches("pub const CONNECTION_IR_HASH: u64 = 0x")
                .count(),
            1
        );
        // The protocol's file refers to them, and still exports them.
        assert!(!protocol.contains("pub type Format = "), "{protocol}");
        assert!(
            !protocol.contains("pub const CONNECTION_IR_HASH"),
            "{protocol}"
        );
        let path = match mode {
            Mode::Code => "crate::connection",
            Mode::Lib => "crate",
        };
        assert!(
            protocol.contains(&format!(
                "#[doc(hidden)]\npub use {path}::{{CONNECTION_IR_HASH, Format}};\n"
            )),
            "{protocol}"
        );
    }

    // Where there is no package module to define them, the file does: the
    // lockstep client has its `Format`, and no digest of a connection.
    let all = files(&lockstep(fixture("protocol")), Mode::Code, false);
    let protocol = file(&all, "conformance.rs");
    assert!(
        protocol.contains("pub type Format = comline_runtime::format::Packed;"),
        "{protocol}"
    );
    assert!(!protocol.contains("CONNECTION_IR_HASH"), "{protocol}");
}

#[test]
fn connect_says_what_it_takes() {
    let with = files(&fixture("connections"), Mode::Code, false);
    let connection = file(&with, "connection.rs");
    assert!(
        connection.contains("/// `services`, which the server calls."),
        "{connection}"
    );
    let without = files(&fixture("framing_datagram3"), Mode::Code, false);
    let connection = file(&without, "connection.rs");
    // `connect` takes no `services` here, so it doesn't promise to provide any.
    assert!(
        connection.contains("pub fn connect<T: Transport>(transport: T)"),
        "{connection}"
    );
    assert!(
        !connection.contains("then provide every protocol in"),
        "{connection}"
    );
    assert!(
        !connection.contains("which the server calls"),
        "{connection}"
    );
    assert!(
        connection.contains("/// Dial over `transport`: run the handshake."),
        "{connection}"
    );
}

/// A schema at the crate root of a `lib` package can't be named like the
/// types the package's module declares there.
#[test]
fn a_schema_named_like_a_root_type_is_refused_in_lib_mode() {
    use comline_core::schema::ir::frozen::unit::FrozenUnit;
    for name in [
        "Connection",
        "Format",
        "Services",
        "Transport",
        "RuntimeError",
    ] {
        let mut fx = fixture("framing_datagram3");
        let units: Vec<FrozenUnit> = fx.schemas[0].1.clone();
        fx.schemas = vec![(name.to_string(), units)];
        let refused = generate_rust(&request(&fx, Mode::Lib, false))
            .expect_err(&format!("a schema `{name}` clashes"))
            .to_string();
        assert!(refused.contains(&format!("`{name}`")), "{refused}");
        // In `code` mode its file is its own, and nothing clashes.
        generate_rust(&request(&fx, Mode::Code, false)).unwrap_or_else(|e| panic!("{name}: {e:#}"));
    }
}
