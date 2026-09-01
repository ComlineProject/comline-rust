//! Generate a `lib` crate for a protocol and actually `cargo build` it against
//! `comline-runtime`. Proves the emitted params structs / error enums /
//! provider trait / `Dispatch` impl / `Client` stub all compile — string
//! assertions can't.
//!
//! Needs network (the generated `Cargo.toml` git-deps `comline-runtime`) and a
//! full runtime compile, so it is a touch slow.

use std::fs;
use std::process::Command;

use comline_codegen::{GenRequest, Mode, PackageMeta};
use comline_codegen_rust::generate_rust;
use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
use comline_core::schema::ir::frozen::unit::{FrozenArgument, FrozenUnit};

fn field(name: &str, ty: &str) -> FrozenUnit {
    FrozenUnit::Field {
        docstring: None,
        parameters: vec![],
        optional: false,
        name: name.into(),
        kind_value: KindValue::Namespaced(ty.into(), None),
        span: (0, 0),
    }
}

fn arg(name: &str, ty: KindValue) -> FrozenArgument {
    FrozenArgument {
        name: name.into(),
        kind: ty,
        span: (0, 0),
    }
}

fn function(
    name: &str,
    args: Vec<FrozenArgument>,
    ret: Option<KindValue>,
    throws: Vec<u16>,
) -> FrozenUnit {
    FrozenUnit::Function {
        docstring: String::new(),
        parameters: vec![],
        name: name.into(),
        arguments: args,
        _return: ret,
        throws,
        span: (0, 0),
    }
}

/// A schema with a struct, an `error`, and a protocol exercising: a throwing
/// call, a non-throwing call returning a list, a zero-arg call, and a
/// `KindValue::Unit` return.
fn chat_schema() -> Vec<FrozenUnit> {
    vec![
        FrozenUnit::Struct {
            docstring: None,
            parameters: vec![],
            name: "Message".into(),
            fields: vec![field("body", "string"), field("seq", "u64")],
            span: (0, 0),
        },
        FrozenUnit::Error {
            docstring: None,
            parameters: vec![],
            ordinal: 0,
            imported_from: None,
            name: "Rejected".into(),
            message: "rejected: {self.why}".into(),
            fields: vec![field("why", "string")],
        },
        FrozenUnit::Protocol {
            docstring: "Chat".into(),
            parameters: vec![],
            name: "Chat".into(),
            functions: vec![
                function(
                    "send",
                    vec![arg("msg", KindValue::Namespaced("Message".into(), None))],
                    Some(KindValue::Namespaced("Message".into(), None)),
                    vec![0],
                ),
                // a `str` arg + a primitive: exercises the borrowed params
                // struct (`&'de str`) alongside an owned field
                function(
                    "search",
                    vec![
                        arg("query", KindValue::Namespaced("string".into(), None)),
                        arg("limit", KindValue::Primitive(Primitive::U32(None))),
                    ],
                    Some(KindValue::Namespaced("Message[]".into(), None)),
                    vec![],
                ),
                function(
                    "history",
                    vec![arg("limit", KindValue::Primitive(Primitive::U32(None)))],
                    Some(KindValue::Namespaced("Message[]".into(), None)),
                    vec![],
                ),
                function("wipe", vec![], Some(KindValue::Unit), vec![]),
                function("poke", vec![], None, vec![]),
                // @timeout_ms = 3000 → the client emits `call_with_timeout`
                FrozenUnit::Function {
                    docstring: String::new(),
                    parameters: vec![FrozenUnit::Property {
                        name: "timeout_ms".into(),
                        expression: Some("3000".into()),
                    }],
                    name: "await_ack".into(),
                    arguments: vec![arg("token", KindValue::Namespaced("string".into(), None))],
                    _return: Some(KindValue::Unit),
                    throws: vec![],
                    span: (0, 0),
                },
            ],
            span: (0, 0),
        },
    ]
}

#[test]
fn a_generated_protocol_crate_builds() {
    let schemas = vec![("chat".to_string(), chat_schema())];
    let req = GenRequest {
        mode: Mode::Lib,
        schemas: &schemas,
        package: PackageMeta {
            name: "comline-codegen-rust-compiletest".into(),
            version: "0.0.0".into(),
        },
    };
    let files = generate_rust(&req).expect("generation");

    let dir = std::env::temp_dir().join(format!(
        "comline-rust-compiletest-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    for f in &files {
        let path = dir.join(&f.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &f.contents).unwrap();
    }

    // Isolated target dir so it doesn't fight this crate's build lock.
    let status = Command::new(env!("CARGO"))
        .args(["build", "--quiet"])
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .status()
        .expect("run cargo build");

    let ok = status.success();
    if ok {
        let _ = fs::remove_dir_all(&dir);
    }
    assert!(
        ok,
        "generated crate failed to compile; left at {} for inspection",
        dir.display()
    );
}
