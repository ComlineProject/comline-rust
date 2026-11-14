//! A protocol in a schema under a folder (`chat/admin`) is shown to its
//! reader as `chat::admin::Admin`: the request spells the namespace with `/`,
//! and the generated text used to put `::` after it, `chat/admin::Admin`.
//! Text only: what is numbered, ordered and hashed doesn't change.

use comline_codegen::{connections, facts, GenerateRequest, Mode, PackageMeta, TargetLanguage};
use comline_codegen_rust::generate_rust;
use comline_conformance::nested_protocol;
use comline_core::schema::ir::frozen::unit::FrozenUnit;

/// The request `comline generation` sends for the nested-protocol fixture in
/// `mode`, with the facts its protocol's wire numbers come from.
fn request(mode: Mode) -> GenerateRequest {
    let fx = nested_protocol();
    let mut req = GenerateRequest {
        mode,
        schemas: fx.schemas.clone(),
        package: PackageMeta {
            name: "conformance".into(),
            version: "0.0.0".into(),
        },
        target: TargetLanguage {
            language: "rust".into(),
            version: comline_codegen_rust::LANG_VERSION.into(),
        },
        wire_format: "packed@1".into(),
        external_std: false,
        validate_on_decode: Default::default(),
        async_api: false,
        encodings: comline_codegen::encodings::official_for("rust"),
        facts: None,
        dependencies: vec![],
    };
    assert!(facts::needed_in(&req), "the fixture's protocol needs facts");
    req.facts = Some(comline_conformance::facts_for(&req.schemas, "packed@1"));
    req
}

#[test]
fn a_nested_protocols_display_path_is_spelled_with_colons_throughout() {
    for mode in [Mode::Code, Mode::Lib] {
        for async_api in [false, true] {
            let mut req = request(mode);
            req.async_api = async_api;
            let files = generate_rust(&req).unwrap_or_else(|e| panic!("{mode:?}: {e:#}"));
            let all: String = files
                .iter()
                .map(|f| format!("// {}\n{}\n", f.path.display(), f.contents))
                .collect();
            let at = format!("{mode:?}, async {async_api}");

            // The Hello's comment and `PROTOCOLS`, which `Unavailable` and
            // `warnings()` print from, in the package module.
            assert!(
                all.contains("`chat::admin::Admin`, line"),
                "{at}: the Hello's comment\n{all}"
            );
            assert!(
                all.contains("\"chat::admin::Admin\")]"),
                "{at}: PROTOCOLS\n{all}"
            );
            // And what a sender checks each call against, in the schema's own file.
            let helper = if async_api {
                "check_call_async"
            } else {
                "check_call"
            };
            assert!(
                all.contains(&format!("{helper}("))
                    && all.contains("\"chat::admin::Admin\", \"greet\")?;"),
                "{at}: {helper}\n{all}"
            );
            // Nowhere the two spellings are mixed.
            assert!(!all.contains("chat/admin::"), "{at}\n{all}");
        }
    }
}

/// What the path is spelled with is all that changes: the protocol's number,
/// its calls' numbers, and the connection's hash come from the schemas and the
/// facts, which the request spells the way it always has.
#[test]
fn the_spelling_leaves_numbers_and_hashes_alone() {
    let req = request(Mode::Code);
    let schemas = req.schemas.clone();
    assert_eq!(schemas[0].0, "chat/admin");
    let facts = req.facts.as_ref().unwrap();
    let admin = facts::protocol(Some(facts), "chat/admin", "Admin").expect("facts for Admin");
    let Some(FrozenUnit::Protocol {
        name,
        parameters,
        functions,
        ..
    }) = schemas[0]
        .1
        .iter()
        .find(|u| matches!(u, FrozenUnit::Protocol { .. }))
    else {
        panic!("the fixture has a protocol")
    };
    let at = facts::addressing_in(&req, "chat/admin", name, parameters, functions).unwrap();
    assert_eq!(at.protocol, admin.number);

    // The same bytes either way the display text is read.
    let files = generate_rust(&req).unwrap();
    let hash = format!("{:#018x}", connections::connection_ir_hash(&schemas));
    let connection = files
        .iter()
        .find(|f| f.path.ends_with("connection.rs"))
        .expect("the package connection");
    assert!(
        connection.contents.contains(&hash),
        "{}",
        connection.contents
    );
    assert!(
        connection
            .contents
            .contains(&format!("({}, \"chat::admin::Admin\")", admin.number)),
        "{}",
        connection.contents
    );
}
