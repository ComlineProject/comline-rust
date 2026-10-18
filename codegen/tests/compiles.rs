//! Generate a `lib` crate for a protocol and actually `cargo build` it against
//! `comline-runtime`. Proves the emitted params structs / error enums /
//! provider trait / `Dispatch` impl / `Client` stub all compile — string
//! assertions can't.
//!
//! Needs network (the generated `Cargo.toml` git-deps `comline-runtime`) and a
//! full runtime compile, so it is a touch slow.

use std::fs;
use std::process::Command;

use comline_codegen::validation::ValidateOnDecode;
use comline_codegen::{GenRequest, Mode, PackageMeta};
use comline_codegen_rust::generate_rust;
use comline_core::schema::ir::frozen::types::{Literal, PrimitiveType, TypeRef};
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenArgument, FrozenUnit};

/// The type a schema spells `written` - a primitive name, a named type, or
/// either followed by `[]` / `[N]`.
fn type_of(written: &str) -> TypeRef {
    if let Some(rest) = written.strip_suffix(']') {
        let (element, length) = rest.rsplit_once('[').expect("an array type");
        return TypeRef::Array {
            element: Box::new(type_of(element)),
            length: (!length.is_empty()).then(|| length.parse().expect("an array length")),
        };
    }
    match PrimitiveType::from_name(written) {
        Some(p) => TypeRef::Primitive(p),
        None => TypeRef::Named(written.to_string()),
    }
}

fn field(name: &str, ty: &str) -> FrozenUnit {
    FrozenUnit::Field {
        docstring: None,
        parameters: vec![],
        optional: false,
        name: name.into(),
        ty: type_of(ty),
        default: None,
        span: (0, 0),
    }
}

fn arg(name: &str, ty: TypeRef) -> FrozenArgument {
    FrozenArgument {
        name: name.into(),
        ty,
        span: (0, 0),
    }
}

fn function(
    name: &str,
    args: Vec<FrozenArgument>,
    ret: Option<TypeRef>,
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
/// `TypeRef::Unit` return. `@framing = "datagram"` keeps it on the datagram
/// stack even though the compile test sets a `jsonrpc` package default — so the
/// datagram path (with all this machinery) still gets built.
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
            span: Some((0, 0)),
            message_span: None,
            uninterpolated: vec![],
        },
        FrozenUnit::Protocol {
            docstring: "Chat".into(),
            parameters: vec![FrozenUnit::Property {
                name: "framing".into(),
                value: ConfigValue::Str("datagram".into()),
            }],
            name: "Chat".into(),
            functions: vec![
                function(
                    "send",
                    vec![arg("msg", TypeRef::Named("Message".to_string()))],
                    Some(TypeRef::Named("Message".to_string())),
                    vec![0],
                ),
                // a `str` arg + a primitive: exercises the borrowed params
                // struct (`&'de str`) alongside an owned field
                function(
                    "search",
                    vec![
                        arg("query", TypeRef::Primitive(PrimitiveType::String)),
                        arg("limit", TypeRef::Primitive(PrimitiveType::U32)),
                    ],
                    Some(TypeRef::Array { element: Box::new(TypeRef::Named("Message".to_string())), length: None }),
                    vec![],
                ),
                function(
                    "history",
                    vec![arg("limit", TypeRef::Primitive(PrimitiveType::U32))],
                    Some(TypeRef::Array { element: Box::new(TypeRef::Named("Message".to_string())), length: None }),
                    vec![],
                ),
                function("wipe", vec![], Some(TypeRef::Unit), vec![]),
                function("poke", vec![], None, vec![]),
                // @timeout_ms = 3000 → the client emits `call_with_timeout`
                FrozenUnit::Function {
                    docstring: String::new(),
                    parameters: vec![FrozenUnit::Property {
                        name: "timeout_ms".into(),
                        value: ConfigValue::Integer(3000),
                    }],
                    name: "await_ack".into(),
                    arguments: vec![arg("token", TypeRef::Primitive(PrimitiveType::String))],
                    _return: Some(TypeRef::Unit),
                    throws: vec![],
                    span: (0, 0),
                },
            ],
            span: (0, 0),
        },
    ]
}

/// A protocol annotated `@framing = "jsonrpc"` — its generated `connect` /
/// `serve` helpers must compile against `comline_runtime::framing::JsonRpcFraming`
/// and the `Client<T, W, JsonRpcFraming>` alias.
fn rpc_schema() -> Vec<FrozenUnit> {
    vec![FrozenUnit::Protocol {
        docstring: "Rpc".into(),
        parameters: vec![FrozenUnit::Property {
            name: "framing".into(),
            value: ConfigValue::Str("jsonrpc".into()),
        }],
        name: "Rpc".into(),
        functions: vec![
            function(
                "echo",
                vec![arg("line", TypeRef::Primitive(PrimitiveType::String))],
                Some(TypeRef::Primitive(PrimitiveType::String)),
                vec![],
            ),
            function(
                "tick",
                vec![],
                Some(TypeRef::Primitive(PrimitiveType::U32)),
                vec![],
            ),
        ],
        span: (0, 0),
    }]
}

/// A plain protocol with no `@framing` — it rides the `jsonrpc` package default
/// the test sets, so the default-driven JSON-RPC output also gets compiled.
fn rpc_default_schema() -> Vec<FrozenUnit> {
    vec![FrozenUnit::Protocol {
        docstring: "Clock".into(),
        parameters: vec![],
        name: "Clock".into(),
        functions: vec![function(
            "now",
            vec![arg("tz", TypeRef::Primitive(PrimitiveType::String))],
            Some(TypeRef::Primitive(PrimitiveType::U64)),
            vec![],
        )],
        span: (0, 0),
    }]
}

#[test]
fn a_generated_protocol_crate_builds() {
    let schemas = vec![
        ("chat".to_string(), chat_schema()),
        ("rpc".to_string(), rpc_schema()),
        ("clock".to_string(), rpc_default_schema()),
    ];
    // `chat` opts back to datagram with `@framing = "datagram"`; `clock`
    // (unannotated) takes this default; `rpc` names `jsonrpc` itself.
    build_generated_crate("protocol", &schemas, Some("jsonrpc".into()), false);
}

/// `lib` mode links `comline_std` rather than inlining std, which only
/// works if the crate it emits actually resolves against the real pinned
/// `comline_std` - the import rewriting, the dependency line, and the
/// absence of std's own files all have to agree. Emitting the right text is
/// not the same claim as building, so this builds it.
#[test]
fn a_generated_crate_linking_comline_std_builds() {
    let schemas = vec![
        ("chat".to_string(), uses_std_http()),
        // Std's own schema, present the way a real package sees it. With
        // `external_std` on it is filtered out of the output and turned
        // into the dependency instead.
        ("std/http".to_string(), vec![]),
    ];
    build_generated_crate("externalstd", &schemas, None, true);
}

/// An aliased cross-schema import has to resolve to the name the *other*
/// schema exports, not to this schema's name for it: `use shared::Shared as
/// Aliased` binds `Aliased` here while `shared` still exports only `Shared`.
/// Emitting `crate::shared::Aliased` is well-formed Rust naming nothing, so
/// only a build catches it - and that is how it survived: the unit test
/// asserted the path *prefix*, which was right, while the path it produced
/// was wrong.
///
/// Found by the multi-schema conformance corpus, which until then could not
/// express a reference that crosses schemas at all.
#[test]
fn a_generated_crate_with_an_aliased_import_builds() {
    let schemas = vec![
        (
            "shared".to_string(),
            vec![FrozenUnit::Struct {
                docstring: None,
                parameters: vec![],
                name: "Shared".to_string(),
                fields: vec![field("value", "s32")],
                span: (0, 0),
            }],
        ),
        (
            "consumer".to_string(),
            vec![
                FrozenUnit::Import(
                    "shared::Shared".to_string(),
                    Some("Aliased".to_string()),
                    (0, 0),
                ),
                FrozenUnit::Struct {
                    docstring: None,
                    parameters: vec![],
                    name: "Holder".to_string(),
                    fields: vec![field("one", "Aliased")],
                    span: (0, 0),
                },
            ],
        ),
    ];
    build_generated_crate("aliasimport", &schemas, None, false);
}

/// A struct whose field is typed by `std`'s `Request`, with the leaf import
/// core expands `use std::http::Request;` into before the generator sees it.
fn uses_std_http() -> Vec<FrozenUnit> {
    vec![
        FrozenUnit::Import("std::http::Request".to_string(), None, (0, 0)),
        FrozenUnit::Struct {
            docstring: None,
            parameters: vec![],
            name: "Call".to_string(),
            fields: vec![field("request", "Request")],
            span: (0, 0),
        },
    ]
}

/// Generate into a temp dir and `cargo build` it. Left in place on failure,
/// so the generated source can be read rather than guessed at.
fn build_generated_crate(
    label: &str,
    schemas: &[(String, Vec<FrozenUnit>)],
    default_framing: Option<String>,
    external_std: bool,
) {
    let req = GenRequest {
        mode: Mode::Lib,
        schemas,
        package: PackageMeta {
            name: "comline-codegen-rust-compiletest".into(),
            version: "0.0.0".into(),
        },
        lang_version: comline_codegen_rust::LANG_VERSION.into(),
        default_framing,
        external_std,
        validate_on_decode: Default::default(),
    };
    check_generated_crate(label, &req, None);
}

/// Generate `req` into a temp dir and `cargo build` it - or, given a test
/// file, add it as `tests/behaviour.rs` (with `serde_json` to decode with)
/// and `cargo test` it, so what the generated code *does* is checked too.
fn check_generated_crate(label: &str, req: &GenRequest, behaviour: Option<&str>) {
    let files = generate_rust(req).expect("generation");

    let dir = std::env::temp_dir().join(format!(
        "comline-rust-compiletest-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    for f in &files {
        let path = dir.join(&f.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &f.contents).unwrap();
    }

    // Local iteration: build against a sibling `runtime` checkout instead of
    // the `RUNTIME_REV` the generator pins. Without this, a runtime change
    // and the generator change that needs it cannot both be proved until the
    // runtime is merged and a rev exists to point at - which is the wrong way
    // round. Set `COMLINE_RUNTIME_PATH=../runtime/runtime/core`.
    // An empty value counts as unset, so `COMLINE_RUNTIME_PATH=` in a shell
    // falls back to the pinned rev rather than panicking on an empty path.
    if let Some(runtime) = std::env::var("COMLINE_RUNTIME_PATH")
        .ok()
        .filter(|p| !p.is_empty())
    {
        let runtime = std::fs::canonicalize(&runtime)
            .unwrap_or_else(|e| panic!("COMLINE_RUNTIME_PATH {runtime}: {e}"));
        let manifest = dir.join("Cargo.toml");
        let mut contents = fs::read_to_string(&manifest).unwrap();
        contents.push_str(&format!(
            "\n[patch.\"https://github.com/ComlineProject/runtime\"]\n\
             comline-runtime = {{ path = {:?} }}\n",
            runtime
        ));
        fs::write(&manifest, contents).unwrap();
    }

    if let Some(test) = behaviour {
        fs::create_dir_all(dir.join("tests")).unwrap();
        fs::write(dir.join("tests/behaviour.rs"), test).unwrap();
        let manifest = dir.join("Cargo.toml");
        let mut contents = fs::read_to_string(&manifest).unwrap();
        contents.push_str("\n[dev-dependencies]\nserde_json = \"1\"\n");
        fs::write(&manifest, contents).unwrap();
    }

    // Isolated target dir so it doesn't fight this crate's build lock.
    let status = Command::new(env!("CARGO"))
        .args([if behaviour.is_some() { "test" } else { "build" }, "--quiet"])
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

/// Constants of every shape the IDL allows, actually compiled. String
/// assertions cannot catch `&'static str` tripping clippy, a `u8` holding a
/// value only a `u64` could, or a signed constant keeping the IDL's `s`
/// spelling instead of Rust's `i`.
#[test]
fn a_generated_crate_with_constants_builds() {
    let constant = |name: &str, ty: PrimitiveType, value: Literal| FrozenUnit::Constant {
        docstring: None,
        name: name.to_string(),
        ty: TypeRef::Primitive(ty),
        value: Some(value),
        span: (0, 0),
    };
    use Literal::{Bool, Integer};
    use PrimitiveType as P;
    let schemas = vec![(
        "limits".to_string(),
        vec![
            constant("MAX_RETRIES", P::U8, Integer(5)),
            constant("WINDOW", P::U16, Integer(1024)),
            constant("CAPACITY", P::U32, Integer(70000)),
            constant("TOTAL", P::U64, Integer(5_000_000_000)),
            constant("HUGE", P::U128, Integer(1)),
            constant("OFFSET", P::S8, Integer(-3)),
            constant("DELTA", P::S16, Integer(-300)),
            constant("DRIFT", P::S32, Integer(-70000)),
            constant("SKEW", P::S64, Integer(-5_000_000_000)),
            constant("DEEP", P::S128, Integer(-1)),
            constant("DEBUG", P::Bool, Bool(true)),
            constant("QUIET", P::Bool, Bool(false)),
            constant("SERVICE_NAME", P::String, Literal::String("store".to_string())),
        ],
    )];
    build_generated_crate("constants", &schemas, None, false);
}

/// Exact float widths and fixed arrays, compiled: `[u8; 4]` through serde's
/// own derive, `[u8; 64]` through `serde_big_array` (whose dependency the
/// generated `Cargo.toml` has to declare), and an integer-written float
/// constant that needs its decimal point.
#[test]
fn a_generated_crate_with_floats_and_fixed_arrays_builds() {
    let schemas = vec![(
        "shapes".to_string(),
        vec![
            FrozenUnit::Struct {
                docstring: None,
                parameters: vec![],
                name: "Sample".to_string(),
                fields: vec![
                    field("ratio", "f32"),
                    field("precise", "f64"),
                    field("rgba", "u8[4]"),
                    field("digest", "u8[64]"),
                    field("rows", "u16[3][]"),
                ],
                span: (0, 0),
            },
            FrozenUnit::Constant {
                docstring: None,
                name: "SCALE".to_string(),
                ty: TypeRef::Primitive(PrimitiveType::F32),
                value: Some(Literal::Integer(2)),
                span: (0, 0),
            },
        ],
    )];
    build_generated_crate("fixed_arrays", &schemas, None, false);
}

/// The IDL the validation tests generate from: std's `StringBounds` (bound
/// through a constant), a local validator over a number, an alias carrying
/// checks, an optional field and an array of the alias, a struct nested in
/// another, and a function argument checked through the alias.
const VALIDATED: &str = r#"
use std::validators::StringBounds

const MIN_HANDLE: u32 = 2

validator Range for u32 {
    min: u32 = 0
    max: u32 = 100

    validate {
        assert(value >= params.min and value <= params.max,
               f"{value.name} must be between {params.min} and {params.max}, got {value}")
    }
}

@validators = [StringBounds(min_chars = MIN_HANDLE, max_chars = 8)]
type Handle = string

struct Profile {
    handle: Handle
    @validators = [Range(max = 120)]
    age: u32
    optional nickname: Handle
    friends: Handle[]
}

struct Post {
    author: Profile
    @validators = [StringBounds(min_chars = 1)]
    body: string
}

protocol Social {
    function rename(to: Handle) -> Handle;
}
"#;

fn validated_schemas() -> Vec<(String, Vec<FrozenUnit>)> {
    use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
    use comline_core::schema::ir::compiler::Compile;
    use comline_core::package::stdlib;
    // std's own `validators.ids`, as every build gets it.
    let (_, std_validators) = stdlib::schemas()
        .find(|(namespace, _)| namespace.join("/") == "std/validators")
        .expect("std ships validators");
    vec![
        (
            "std/validators".to_string(),
            IncrementalInterpreter::from_source(std_validators),
        ),
        ("app".to_string(), IncrementalInterpreter::from_source(VALIDATED)),
    ]
}

/// The generated `validate()` / `validate_first()`, run: every failure
/// collected with its path and message, nested values and array elements
/// included, an absent optional skipped - and decoding, with
/// `validate_on_decode = "collect"`, refusing a value that fails.
#[test]
fn generated_validators_run() {
    let schemas = validated_schemas();
    let req = GenRequest {
        mode: Mode::Lib,
        schemas: &schemas,
        package: PackageMeta {
            name: "comline-codegen-rust-compiletest".into(),
            version: "0.0.0".into(),
        },
        lang_version: comline_codegen_rust::LANG_VERSION.into(),
        default_framing: None,
        external_std: false,
        validate_on_decode: ValidateOnDecode::Collect,
    };
    check_generated_crate("validators", &req, Some(VALIDATED_BEHAVIOUR));
}

const VALIDATED_BEHAVIOUR: &str = r##"
use comline_codegen_rust_compiletest::app::{Post, Profile, SocialRenameParams};
use comline_runtime::validation::{Validate, ValidationErrors};

fn profile() -> Profile {
    Profile { handle: "ada".into(), age: 30, nickname: None, friends: vec!["bob".into()] }
}

fn failures(errors: ValidationErrors) -> Vec<(String, String)> {
    errors.0.into_iter().map(|e| (e.field, e.message)).collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected.iter().map(|(f, m)| (f.to_string(), m.to_string())).collect()
}

#[test]
fn a_valid_value_passes() {
    assert!(profile().validate().is_ok());
    assert!(profile().validate_first().is_ok());
}

#[test]
fn every_failure_is_collected_with_its_path() {
    let bad = Profile {
        handle: "a".into(),
        age: 200,
        nickname: Some("far_too_long".into()),
        friends: vec!["ok".into(), "x".into()],
    };
    assert_eq!(
        failures(bad.validate().unwrap_err()),
        pairs(&[
            ("handle", "handle must be at least 2 characters"),
            ("age", "age must be between 0 and 120, got 200"),
            ("nickname", "nickname must be at most 8 characters"),
            ("friends[1]", "friends[1] must be at least 2 characters"),
        ])
    );
    assert_eq!(bad.validate_first().unwrap_err().field, "handle");
}

#[test]
fn length_counts_characters_not_bytes() {
    // Eight characters, sixteen bytes.
    let p = Profile { handle: "éééééééé".into(), ..profile() };
    assert!(p.validate().is_ok());
}

#[test]
fn a_nested_value_is_validated_under_its_path() {
    let post = Post { author: Profile { handle: "a".into(), ..profile() }, body: String::new() };
    let fields: Vec<String> = post.validate().unwrap_err().0.into_iter().map(|e| e.field).collect();
    assert_eq!(fields, ["author.handle", "body"]);
}

#[test]
fn an_argument_is_validated_through_its_alias() {
    let params = SocialRenameParams { to: "x".into() };
    assert_eq!(params.validate_first().unwrap_err().field, "to");
}

#[test]
fn decoding_runs_the_checks() {
    let ok: Result<Profile, _> =
        serde_json::from_str(r#"{"handle":"ada","age":3,"nickname":null,"friends":[]}"#);
    assert!(ok.is_ok());
    let bad = serde_json::from_str::<Profile>(
        r#"{"handle":"a","age":3,"nickname":null,"friends":[]}"#,
    )
    .unwrap_err();
    assert!(bad.to_string().contains("handle must be at least 2 characters"), "{bad}");
}
"##;
