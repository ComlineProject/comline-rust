use comline_codegen::{GenRequest, Mode, PackageMeta};
use comline_codegen_rust::generate_rust;
use comline_core::schema::ir::frozen::types::{Literal, PrimitiveType, TypeRef};
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenArgument, FrozenUnit};

fn code_req(schemas: &[(String, Vec<FrozenUnit>)]) -> GenRequest<'_> {
    GenRequest {
        mode: Mode::Code,
        schemas,
        package: PackageMeta {
            name: "test".into(),
            version: "0.1.0".into(),
        },
        lang_version: comline_codegen_rust::LANG_VERSION.into(),
        default_framing: None,
        external_std: false,
        validate_on_decode: Default::default(),
    }
}

fn lib_req(schemas: &[(String, Vec<FrozenUnit>)]) -> GenRequest<'_> {
    GenRequest {
        mode: Mode::Lib,
        schemas,
        package: PackageMeta {
            name: "chat".into(),
            version: "0.3.0".into(),
        },
        lang_version: comline_codegen_rust::LANG_VERSION.into(),
        default_framing: None,
        external_std: false,
        validate_on_decode: Default::default(),
    }
}

fn user_struct() -> FrozenUnit {
    FrozenUnit::Struct {
        docstring: None,
        parameters: vec![],
        name: "User".to_string(),
        fields: vec![
            FrozenUnit::Field {
                docstring: None,
                parameters: vec![],
                optional: false,
                name: "id".to_string(),
                ty: TypeRef::Primitive(PrimitiveType::S32), default: None,
                span: (0, 0),
            },
            FrozenUnit::Field {
                docstring: None,
                parameters: vec![],
                optional: false,
                name: "username".to_string(),
                ty: TypeRef::Primitive(PrimitiveType::String), default: None,
                span: (0, 0),
            },
            FrozenUnit::Field {
                docstring: None,
                parameters: vec![],
                optional: false,
                name: "tags".to_string(),
                ty: TypeRef::Array { element: Box::new(TypeRef::Primitive(PrimitiveType::String)), length: None }, default: None,
                span: (0, 0),
            },
        ],
        span: (0, 0),
    }
}

#[test]
fn code_mode_one_file_per_schema() {
    let schemas = vec![("account".to_string(), vec![user_struct()])];
    let files = generate_rust(&code_req(&schemas)).unwrap();

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path.to_str().unwrap(), "account.rs");

    let src = &files[0].contents;
    assert!(src.contains("pub struct User"));
    assert!(src.contains("pub id: i32"));
    assert!(src.contains("pub username: String"));
    assert!(src.contains("pub tags: Vec<String>"));
}

#[test]
fn code_mode_generates_enum_and_protocol() {
    let enum_unit = FrozenUnit::Enum {
        docstring: None,
        name: "Status".to_string(),
        variants: vec![
            FrozenUnit::EnumVariant("Active".to_string(), (0, 0)),
            FrozenUnit::EnumVariant("Inactive".to_string(), (0, 0)),
        ],
        span: (0, 0),
    };
    let proto = FrozenUnit::Protocol {
        docstring: "A user service".to_string(),
        name: "UserService".to_string(),
        parameters: vec![],
        functions: vec![FrozenUnit::Function {
            docstring: String::new(),
            name: "get_user".to_string(),
            parameters: vec![],
            arguments: vec![FrozenArgument {
                name: "id".to_string(),
                ty: TypeRef::Primitive(PrimitiveType::S32),
                span: (0, 0),
            }],
            _return: Some(TypeRef::Named("User".to_string())),
            throws: vec![],
            span: (0, 0),
        }],
        span: (0, 0),
    };

    let schemas = vec![("account".to_string(), vec![enum_unit, proto])];
    let src = generate_rust(&code_req(&schemas))
        .unwrap()
        .remove(0)
        .contents;

    assert!(src.contains("pub enum Status"));
    assert!(src.contains("Active,"));
    // provider trait: `&self`, a `Result`, a per-function error enum
    assert!(src.contains("pub trait UserService {"));
    assert!(src
        .contains("fn get_user(&self, id: i32) -> Result<User, Fault<UserServiceGetUserError>>;"));
    // params struct + the runtime imports + the call table
    assert!(src.contains("use comline_runtime::client::Client;"));
    assert!(src.contains("pub struct UserServiceGetUserParams {"));
    assert!(src.contains("pub id: i32,"));
    assert!(src.contains(r#"pub const USER_SERVICE_CALLS: &[&str] = &["get_user"];"#));
    // dispatcher + client stub
    assert!(src.contains("impl<S: UserService> Dispatch for UserServiceDispatcher<S>"));
    assert!(src.contains("impl<T: Transport, W: WireFormat> UserServiceClient<T, W>"));
    assert!(src.contains(
        "pub fn get_user(&mut self, id: i32) -> Result<User, CallError<UserServiceGetUserError>>"
    ));
    // handshake: an IR fingerprint + connect / serve helpers
    assert!(src.contains("pub const IR_HASH: u64 = 0x"));
    assert!(src.contains("pub fn connect(transport: T, format: W) -> Result<Self, RuntimeError>"));
    assert!(src.contains("let hs = Handshake::new(IR_HASH, format.name(), FRAMING_DATAGRAM, 0);"));
    assert!(src.contains("impl<S: UserService> UserServiceDispatcher<S> {"));
    assert!(src
        .contains("pub fn serve<T: Transport, W: WireFormat>(self, transport: &mut T, format: W)"));
    assert!(src.contains("Server::new(self, format).serve_handshaked(transport, hs)"));
}

#[test]
fn code_mode_emits_a_type_alias_and_fields_reference_it_by_name() {
    let alias = FrozenUnit::TypeAlias {
        docstring: None,
        parameters: vec![],
        name: "UserId".to_string(),
        target: TypeRef::Primitive(PrimitiveType::U64),
        span: (0, 0),
    };
    let user = FrozenUnit::Struct {
        docstring: None,
        parameters: vec![],
        name: "Greeting".to_string(),
        fields: vec![FrozenUnit::Field {
            docstring: None,
            parameters: vec![],
            optional: false,
            name: "sender".to_string(),
            ty: TypeRef::Named("UserId".to_string()), default: None,
            span: (0, 0),
        }],
        span: (0, 0),
    };

    let schemas = vec![("account".to_string(), vec![alias, user])];
    let src = generate_rust(&code_req(&schemas))
        .unwrap()
        .remove(0)
        .contents;

    assert!(src.contains("pub type UserId = u64;"), "got: {src}");
    assert!(src.contains("pub sender: UserId,"), "got: {src}");
}

#[test]
fn a_schema_without_a_protocol_has_no_ir_hash() {
    let schemas = vec![("plain".to_string(), vec![user_struct()])];
    let src = generate_rust(&code_req(&schemas))
        .unwrap()
        .remove(0)
        .contents;
    assert!(!src.contains("IR_HASH"));
    assert!(!src.contains("comline_runtime"));
}

#[test]
fn protocol_errors_map_to_ordinals_and_a_union() {
    // error Rejected { why: str }  (ordinal 0)
    let rejected = FrozenUnit::Error {
        docstring: None,
        parameters: vec![],
        ordinal: 0,
        imported_from: None,
        name: "Rejected".to_string(),
        message: "no".to_string(),
        fields: vec![FrozenUnit::Field {
            docstring: None,
            parameters: vec![],
            optional: false,
            name: "why".to_string(),
            ty: TypeRef::Primitive(PrimitiveType::String), default: None,
            span: (0, 0),
        }],
        span: Some((0, 0)),
        message_span: None,
        uninterpolated: vec![],
    };
    let proto = FrozenUnit::Protocol {
        docstring: "Chat".to_string(),
        parameters: vec![],
        name: "Chat".to_string(),
        functions: vec![
            // request/response with an empty ack (`-> ()`), and a `!`
            FrozenUnit::Function {
                docstring: String::new(),
                name: "send".to_string(),
                parameters: vec![],
                arguments: vec![FrozenArgument {
                    name: "body".to_string(),
                    ty: TypeRef::Primitive(PrimitiveType::String),
                    span: (0, 0),
                }],
                _return: Some(TypeRef::Unit),
                throws: vec![0],
                span: (0, 0),
            },
            FrozenUnit::Function {
                docstring: String::new(),
                name: "ping".to_string(),
                parameters: vec![],
                arguments: vec![],
                _return: Some(TypeRef::Unit),
                throws: vec![],
                span: (0, 0),
            },
            // one-way: no `->` at all
            FrozenUnit::Function {
                docstring: String::new(),
                name: "poke".to_string(),
                parameters: vec![],
                arguments: vec![FrozenArgument {
                    name: "note".to_string(),
                    ty: TypeRef::Primitive(PrimitiveType::String),
                    span: (0, 0),
                }],
                _return: None,
                throws: vec![], // a `!` on a one-way fn is dropped
                span: (0, 0),
            },
        ],
        span: (0, 0),
    };

    let schemas = vec![("chat".to_string(), vec![rejected, proto])];
    let src = generate_rust(&code_req(&schemas))
        .unwrap()
        .remove(0)
        .contents;

    // the error struct
    assert!(src.contains("pub struct Rejected {"));
    // per-function enum from the throw ordinal
    assert!(src.contains("pub enum ChatSendError {\n    Rejected(Rejected),\n}"));
    // A non-throwing request/response function names the never type instead
    // of minting an uninhabited enum of its own.
    assert!(src.contains("pub type ChatPingError = ::core::convert::Infallible;"));
    assert!(!src.contains("pub enum ChatPingError"));
    // per-protocol union + From impl
    assert!(src.contains("pub enum ChatError {\n    Rejected(Rejected),\n}"));
    assert!(src.contains("impl From<ChatSendError> for ChatError"));
    // dispatcher records the error at its ordinal on the framing-agnostic Reply
    assert!(src.contains("reply.err(0u16, &body);"));
    assert!(src.contains("reply.ok(&body);"));
    assert!(src.contains("fn calls(&self) -> &'static [&'static str] {"));
    // client maps that ordinal back
    assert!(src.contains("Envelope::Err { id: 0u16, body } =>"));
    // `-> ()` (Unit) is request/response with an empty ack
    assert!(src.contains("fn ping(&self) -> Result<(), Fault<ChatPingError>>;"));
    // both addresses on the wire; the framing picks
    assert!(src.contains(r#"self.0.call(Call::new(1, "ping"), &())"#));
    // a `str` arg is borrowed
    assert!(src.contains("fn send(&self, body: &str) -> Result<(), Fault<ChatSendError>>;"));
    assert!(src.contains(
        "pub struct ChatSendParams<'a> {\n    #[serde(borrow)]\n    pub body: &'a str,\n}"
    ));
    assert!(
        src.contains("pub fn send(&mut self, body: &str) -> Result<(), CallError<ChatSendError>>")
    );

    // Every request/response arm can answer with a fault, including the one
    // whose error type is uninhabited.
    assert!(src.contains("Err(Fault::Internal) => reply.fault(),"));
    assert!(src.contains("Err(Fault::App(never)) => match never {},"));
    assert!(src.contains("Err(Fault::App(ChatSendError::Rejected(e))) => {"));
    // and the client turns a peer's fault back into a runtime error
    assert!(src.contains("Envelope::Fault => Err(CallError::Runtime(RuntimeError::Provider)),"));

    // the `message` the schema requires is what `Display` renders
    assert!(src.contains("impl core::fmt::Display for Rejected {"));
    assert!(src.contains("impl core::fmt::Display for ChatSendError {"));
    // the union delegates to the error struct's own rendering
    assert!(src.contains("ChatError::Rejected(e) => e.fmt(f),"));

    // one-way `poke`: no error enum, a plain trait method, a `notify` client
    assert!(!src.contains("ChatPokeError"));
    assert!(src.contains("fn poke(&self, note: &str);"));
    assert!(src.contains("pub fn poke(&mut self, note: &str) -> Result<(), RuntimeError> {"));
    assert!(src.contains(r#"self.0.notify(Call::new(2, "poke"), &ChatPokeParams { note })"#));
}

#[test]
fn timeout_ms_annotation_emits_call_with_timeout() {
    let proto = FrozenUnit::Protocol {
        docstring: "Api".to_string(),
        parameters: vec![],
        name: "Api".to_string(),
        functions: vec![
            // @timeout_ms = 2500
            FrozenUnit::Function {
                docstring: String::new(),
                name: "slow".to_string(),
                parameters: vec![FrozenUnit::Property {
                    name: "timeout_ms".to_string(),
                    value: ConfigValue::Integer(2500),
                }],
                arguments: vec![],
                _return: Some(TypeRef::Primitive(PrimitiveType::U32)),
                throws: vec![],
                span: (0, 0),
            },
            // no annotation
            FrozenUnit::Function {
                docstring: String::new(),
                name: "fast".to_string(),
                parameters: vec![],
                arguments: vec![],
                _return: Some(TypeRef::Primitive(PrimitiveType::U32)),
                throws: vec![],
                span: (0, 0),
            },
        ],
        span: (0, 0),
    };
    let schemas = vec![("api".to_string(), vec![proto])];
    let src = generate_rust(&code_req(&schemas))
        .unwrap()
        .remove(0)
        .contents;

    assert!(src.contains(
        r#"self.0.call_with_timeout(Call::new(0, "slow"), &(), core::time::Duration::from_millis(2500))?;"#
    ));
    assert!(src.contains(r#"self.0.call(Call::new(1, "fast"), &())?;"#));
}

#[test]
fn framing_annotation_selects_jsonrpc_for_the_connect_and_serve_helpers() {
    // @framing = "jsonrpc" protocol Rpc { function now() -> u32; }
    let proto = FrozenUnit::Protocol {
        docstring: "Rpc".to_string(),
        parameters: vec![FrozenUnit::Property {
            name: "framing".to_string(),
            value: ConfigValue::Str("jsonrpc".into()),
        }],
        name: "Rpc".to_string(),
        functions: vec![FrozenUnit::Function {
            docstring: String::new(),
            name: "now".to_string(),
            parameters: vec![],
            arguments: vec![],
            _return: Some(TypeRef::Primitive(PrimitiveType::U32)),
            throws: vec![],
            span: (0, 0),
        }],
        span: (0, 0),
    };
    let schemas = vec![("rpc".to_string(), vec![proto])];
    let src = generate_rust(&code_req(&schemas))
        .unwrap()
        .remove(0)
        .contents;

    // the `Framing` trait is in scope (the helpers call `framing.name()`)
    assert!(src.contains("\n    Framing,\n"));
    // the client wraps a `Client` pinned to the JSON-RPC framing
    assert!(src.contains(
        "pub struct RpcClient<T, W>(pub Client<T, W, comline_runtime::framing::JsonRpcFraming>);"
    ));
    // connect / serve pass the framing explicitly and hash its name
    assert!(src.contains("let framing = comline_runtime::framing::JsonRpcFraming;"));
    assert!(src.contains("let hs = Handshake::new(IR_HASH, format.name(), framing.name(), 0);"));
    assert!(src.contains("Ok(Self(Client::connect_with_framing(transport, format, framing, hs)?))"));
    assert!(
        src.contains("Server::with_framing(self, format, framing).serve_handshaked(transport, hs)")
    );
    // the datagram default never appears for an all-JSON-RPC schema
    assert!(!src.contains("FRAMING_DATAGRAM"));
    assert!(!src.contains("Client::connect(transport, format, hs)"));
}

/// A protocol with no `@framing` of its own, plus a plain (annotated) one for
/// contrast.
fn one_plain_protocol(name: &str) -> Vec<FrozenUnit> {
    vec![FrozenUnit::Protocol {
        docstring: name.to_string(),
        parameters: vec![],
        name: name.to_string(),
        functions: vec![FrozenUnit::Function {
            docstring: String::new(),
            name: "now".to_string(),
            parameters: vec![],
            arguments: vec![],
            _return: Some(TypeRef::Primitive(PrimitiveType::U32)),
            throws: vec![],
            span: (0, 0),
        }],
        span: (0, 0),
    }]
}

#[test]
fn package_default_framing_applies_when_a_protocol_has_no_annotation() {
    let schemas = vec![("rpc".to_string(), one_plain_protocol("Rpc"))];
    let req = GenRequest {
        default_framing: Some("jsonrpc".to_string()),
        ..code_req(&schemas)
    };
    let src = generate_rust(&req).unwrap().remove(0).contents;

    assert!(src.contains(
        "pub struct RpcClient<T, W>(pub Client<T, W, comline_runtime::framing::JsonRpcFraming>);"
    ));
    assert!(src.contains("Server::with_framing(self, format, framing).serve_handshaked"));
    assert!(!src.contains("FRAMING_DATAGRAM"));
}

#[test]
fn an_explicit_datagram_annotation_opts_out_of_the_package_default() {
    // @framing = "datagram" on the protocol, package default is jsonrpc.
    let mut units = one_plain_protocol("Rpc");
    if let FrozenUnit::Protocol { parameters, .. } = &mut units[0] {
        parameters.push(FrozenUnit::Property {
            name: "framing".to_string(),
            value: ConfigValue::Str("datagram".into()),
        });
    }
    let schemas = vec![("rpc".to_string(), units)];
    let with_default = GenRequest {
        default_framing: Some("jsonrpc".to_string()),
        ..code_req(&schemas)
    };
    let src = generate_rust(&with_default).unwrap().remove(0).contents;

    // the datagram stack — the `@framing = "datagram"` opt-out beats the
    // `jsonrpc` package default (the `IR_HASH` still moves: the annotation is
    // part of the frozen IR)
    assert!(!src.contains("JsonRpcFraming"));
    assert!(!src.contains("Framing,")); // trait not imported
    assert!(src.contains("pub struct RpcClient<T, W>(pub Client<T, W>);"));
    assert!(src.contains("let hs = Handshake::new(IR_HASH, format.name(), FRAMING_DATAGRAM, 0);"));
    assert!(src.contains("Server::new(self, format).serve_handshaked(transport, hs)"));
    assert!(src.contains("Ok(Self(Client::connect(transport, format, hs)?))"));
}

#[test]
fn an_all_one_way_protocol_leaves_the_dispatch_reply_param_unbound() {
    let proto = FrozenUnit::Protocol {
        docstring: "Bus".to_string(),
        parameters: vec![],
        name: "Bus".to_string(),
        functions: vec![FrozenUnit::Function {
            docstring: String::new(),
            name: "emit".to_string(),
            parameters: vec![],
            arguments: vec![FrozenArgument {
                name: "topic".to_string(),
                ty: TypeRef::Primitive(PrimitiveType::String),
                span: (0, 0),
            }],
            _return: None,
            throws: vec![],
            span: (0, 0),
        }],
        span: (0, 0),
    };
    let schemas = vec![("bus".to_string(), vec![proto])];
    let src = generate_rust(&code_req(&schemas))
        .unwrap()
        .remove(0)
        .contents;
    assert!(src.contains("_reply: &mut Reply,"));
    assert!(!src.contains("\n        reply: &mut Reply,"));
}

#[test]
fn lib_mode_emits_a_crate() {
    let schemas = vec![
        ("account".to_string(), vec![user_struct()]),
        ("billing".to_string(), vec![]),
    ];
    let files = generate_rust(&lib_req(&schemas)).unwrap();

    let by_path = |p: &str| files.iter().find(|f| f.path.to_str().unwrap() == p);

    let cargo = &by_path("Cargo.toml").expect("Cargo.toml").contents;
    assert!(cargo.contains("name = \"chat\""));
    assert!(cargo.contains("version = \"0.3.0\""));
    assert!(cargo.contains("edition = \"2021\""));
    assert!(cargo.contains("autobins = false"));
    assert!(cargo.contains("serde = { version = \"1\", features = [\"derive\"] }"));

    let lib = &by_path("src/lib.rs").expect("src/lib.rs").contents;
    assert!(lib.contains("pub mod account;"));
    assert!(lib.contains("pub mod billing;"));

    assert!(by_path("src/account.rs")
        .expect("src/account.rs")
        .contents
        .contains("pub struct User"));
    assert!(by_path("src/billing.rs").is_some());
}

#[test]
fn lib_mode_nests_a_single_level() {
    let schemas = vec![("account/user".to_string(), vec![user_struct()])];
    let files = generate_rust(&lib_req(&schemas)).unwrap();
    let by_path = |p: &str| files.iter().find(|f| f.path.to_str().unwrap() == p);

    let lib = &by_path("src/lib.rs").expect("src/lib.rs").contents;
    assert!(lib.contains("pub mod account;"));

    let account = &by_path("src/account.rs").expect("src/account.rs").contents;
    assert!(account.contains("pub mod user;"));
    // `account` is purely a grouping namespace here - its own file must
    // not also carry `user`'s generated code.
    assert!(!account.contains("pub struct User"));

    let user = &by_path("src/account/user.rs")
        .expect("src/account/user.rs")
        .contents;
    assert!(user.contains("pub struct User"));
}

#[test]
fn lib_mode_nests_multiple_schemas_under_one_namespace() {
    // The actual case that motivated this: comline-core's embedded std
    // merges as `std/validators`, `std/http`, ... under one dependency.
    let schemas = vec![
        ("std/validators".to_string(), vec![user_struct()]),
        ("std/http".to_string(), vec![]),
    ];
    let files = generate_rust(&lib_req(&schemas)).unwrap();
    let by_path = |p: &str| files.iter().find(|f| f.path.to_str().unwrap() == p);

    assert!(by_path("src/lib.rs")
        .unwrap()
        .contents
        .contains("pub mod std;"));

    let std_mod = &by_path("src/std.rs").expect("src/std.rs").contents;
    assert!(std_mod.contains("pub mod http;"));
    assert!(std_mod.contains("pub mod validators;"));

    assert!(by_path("src/std/validators.rs")
        .expect("src/std/validators.rs")
        .contents
        .contains("pub struct User"));
    assert!(by_path("src/std/http.rs").is_some());
}

#[test]
fn lib_mode_nests_multiple_levels() {
    let schemas = vec![("a/b/c".to_string(), vec![user_struct()])];
    let files = generate_rust(&lib_req(&schemas)).unwrap();
    let by_path = |p: &str| files.iter().find(|f| f.path.to_str().unwrap() == p);

    assert!(by_path("src/lib.rs")
        .unwrap()
        .contents
        .contains("pub mod a;"));
    assert!(by_path("src/a.rs").unwrap().contents.contains("pub mod b;"));
    assert!(by_path("src/a/b.rs")
        .unwrap()
        .contents
        .contains("pub mod c;"));
    assert!(by_path("src/a/b/c.rs")
        .unwrap()
        .contents
        .contains("pub struct User"));
}

#[test]
fn lib_mode_rejects_a_namespace_thats_both_a_schema_and_a_parent() {
    // `account` would need its own file to be *both* its generated code
    // and `pub mod user;` - nowhere for one of them to go.
    let schemas = vec![
        ("account".to_string(), vec![user_struct()]),
        ("account/user".to_string(), vec![]),
    ];
    let err = generate_rust(&lib_req(&schemas)).unwrap_err().to_string();
    assert!(err.contains("account"), "{err}");
}

/// `src/lib.rs` is the crate root this mode writes first; a schema of the
/// same name was emitted straight over it, taking the whole `pub mod` tree
/// with it and leaving a crate whose modules had all vanished. Core refuses
/// the name when a package is compiled, but `GenRequest` is public, so a
/// caller building one by hand gets told rather than silently wrecked.
#[test]
fn lib_mode_rejects_a_schema_named_lib() {
    let schemas = vec![("lib".to_string(), vec![user_struct()])];
    let err = generate_rust(&lib_req(&schemas)).unwrap_err().to_string();
    assert!(err.contains("crate root"), "{err}");
}

/// The same clobbering, one level down and previously unguarded: `lib/foo`
/// makes `lib` a *grouping* node, whose own module file is `src/lib.rs` -
/// so the `pub mod foo;` tree is written over the crate root just as a
/// schema named `lib` was.
///
/// Core refuses this too - `reserved_in_namespace` checks *every* segment,
/// so `lib/foo.ids` never reaches a generator from a real project. This is
/// the same defence-in-depth as the `lib` case above: `GenRequest` is a
/// public contract, and a caller building one by hand gets told rather than
/// silently wrecked. Checking a namespace's first segment rather than the
/// whole of it is what closes it, for both targets at once.
#[test]
fn lib_mode_rejects_a_schema_nested_under_lib() {
    let schemas = vec![("lib/foo".to_string(), vec![user_struct()])];
    let err = generate_rust(&lib_req(&schemas)).unwrap_err().to_string();
    assert!(err.contains("crate root"), "{err}");
}

/// The target declares `rust#1.70.0`; the manifest should say so rather
/// than leaving a consumer to discover the toolchain by failing to build.
#[test]
fn lib_mode_records_the_declared_toolchain() {
    let schemas = vec![("chat".to_string(), vec![user_struct()])];
    let req = GenRequest {
        lang_version: "1.74.0".into(),
        ..lib_req(&schemas)
    };
    let files = generate_rust(&req).unwrap();
    let manifest = &files
        .iter()
        .find(|f| f.path.to_str() == Some("Cargo.toml"))
        .expect("a lib build has a manifest")
        .contents;
    assert!(manifest.contains("rust-version = \"1.74.0\""), "{manifest}");
}

/// `main` imports and uses `Shared`, declared over in the `shared` schema -
/// core already expands `use shared::Shared;` into this exact leaf
/// `FrozenUnit::Import` before it ever reaches the generator.
fn cross_namespace_schemas() -> Vec<(String, Vec<FrozenUnit>)> {
    let main_schema = vec![
        FrozenUnit::Import("shared::Shared".to_string(), None, (0, 0)),
        FrozenUnit::Struct {
            docstring: None,
            parameters: vec![],
            name: "Outer".to_string(),
            fields: vec![FrozenUnit::Field {
                docstring: None,
                parameters: vec![],
                optional: false,
                name: "value".to_string(),
                ty: TypeRef::Named("Shared".to_string()), default: None,
                span: (0, 0),
            }],
            span: (0, 0),
        },
    ];
    let shared_schema = vec![FrozenUnit::Struct {
        docstring: None,
        parameters: vec![],
        name: "Shared".to_string(),
        fields: vec![FrozenUnit::Field {
            docstring: None,
            parameters: vec![],
            optional: false,
            name: "n".to_string(),
            ty: TypeRef::Primitive(PrimitiveType::U32), default: None,
            span: (0, 0),
        }],
        span: (0, 0),
    }];
    vec![
        ("main".to_string(), main_schema),
        ("shared".to_string(), shared_schema),
    ]
}

#[test]
fn code_mode_qualifies_a_cross_namespace_field_type() {
    let schemas = cross_namespace_schemas();
    let files = generate_rust(&code_req(&schemas)).unwrap();
    let main_rs = &files
        .iter()
        .find(|f| f.path.to_str().unwrap() == "main.rs")
        .unwrap()
        .contents;
    assert!(
        main_rs.contains("pub value: crate::shared::Shared,"),
        "got: {main_rs}"
    );
}

#[test]
fn lib_mode_qualifies_a_cross_namespace_field_type_consistently_with_module_tree() {
    let schemas = cross_namespace_schemas();
    let files = generate_rust(&lib_req(&schemas)).unwrap();
    let by_path = |p: &str| files.iter().find(|f| f.path.to_str().unwrap() == p);

    let main_rs = &by_path("src/main.rs").unwrap().contents;
    assert!(
        main_rs.contains("pub value: crate::shared::Shared,"),
        "got: {main_rs}"
    );
    // The qualified path's namespace segment must be the same one
    // `ModuleTree` declared a `pub mod` for.
    assert!(by_path("src/lib.rs")
        .unwrap()
        .contents
        .contains("pub mod shared;"));
}

#[test]
fn a_same_schema_local_reference_stays_bare() {
    let schemas = vec![("account".to_string(), vec![user_struct()])];
    let src = generate_rust(&code_req(&schemas))
        .unwrap()
        .remove(0)
        .contents;
    assert!(
        !src.contains("crate::"),
        "a local reference must not be qualified: {src}"
    );
}

/// `main` imports and uses `Request`, declared over in `std/http` - the
/// real namespace shape `package::stdlib::used_by` prefixes std's own
/// schemas with when merging them into a consumer's schema list.
fn std_reference_schemas() -> Vec<(String, Vec<FrozenUnit>)> {
    let main_schema = vec![
        FrozenUnit::Import("std::http::Request".to_string(), None, (0, 0)),
        FrozenUnit::Struct {
            docstring: None,
            parameters: vec![],
            name: "Outer".to_string(),
            fields: vec![FrozenUnit::Field {
                docstring: None,
                parameters: vec![],
                optional: false,
                name: "request".to_string(),
                ty: TypeRef::Named("Request".to_string()), default: None,
                span: (0, 0),
            }],
            span: (0, 0),
        },
    ];
    let std_http_schema = vec![FrozenUnit::Struct {
        docstring: None,
        parameters: vec![],
        name: "Request".to_string(),
        fields: vec![FrozenUnit::Field {
            docstring: None,
            parameters: vec![],
            optional: false,
            name: "uri".to_string(),
            ty: TypeRef::Primitive(PrimitiveType::String), default: None,
            span: (0, 0),
        }],
        span: (0, 0),
    }];
    vec![
        ("main".to_string(), main_schema),
        ("std/http".to_string(), std_http_schema),
    ]
}

fn external_std_code_req(schemas: &[(String, Vec<FrozenUnit>)]) -> GenRequest<'_> {
    GenRequest {
        external_std: true,
        ..code_req(schemas)
    }
}

fn external_std_lib_req(schemas: &[(String, Vec<FrozenUnit>)]) -> GenRequest<'_> {
    GenRequest {
        external_std: true,
        ..lib_req(schemas)
    }
}

#[test]
fn external_std_off_is_unchanged_from_today() {
    let schemas = std_reference_schemas();
    let files = generate_rust(&code_req(&schemas)).unwrap();
    assert!(files
        .iter()
        .any(|f| f.path.to_str().unwrap() == "std/http.rs"));
    let main_rs = &files
        .iter()
        .find(|f| f.path.to_str().unwrap() == "main.rs")
        .unwrap()
        .contents;
    assert!(
        main_rs.contains("pub request: crate::std::http::Request,"),
        "got: {main_rs}"
    );
    assert!(!main_rs.contains("comline_std"));
}

#[test]
fn external_std_filters_stds_own_file_and_qualifies_the_reference_in_code_mode() {
    let schemas = std_reference_schemas();
    let files = generate_rust(&external_std_code_req(&schemas)).unwrap();
    assert!(
        !files
            .iter()
            .any(|f| f.path.to_str().unwrap() == "std/http.rs"),
        "std's own file should not be emitted when external_std is on"
    );
    let main_rs = &files
        .iter()
        .find(|f| f.path.to_str().unwrap() == "main.rs")
        .unwrap()
        .contents;
    assert!(
        main_rs.contains("pub request: comline_std::http::Request,"),
        "got: {main_rs}"
    );
    assert!(
        main_rs.contains("// comline_std = { git ="),
        "code mode has no manifest to add the dependency to - it should say so: {main_rs}"
    );
}

#[test]
fn external_std_adds_the_comline_std_dependency_in_lib_mode() {
    let schemas = std_reference_schemas();
    let files = generate_rust(&external_std_lib_req(&schemas)).unwrap();
    let by_path = |p: &str| files.iter().find(|f| f.path.to_str().unwrap() == p);

    assert!(
        by_path("src/std/http.rs").is_none(),
        "std's own file should not be emitted when external_std is on"
    );
    let cargo_toml = &by_path("Cargo.toml").unwrap().contents;
    assert!(
        cargo_toml.contains("comline_std = { git ="),
        "got: {cargo_toml}"
    );
    let main_rs = &by_path("src/main.rs").unwrap().contents;
    assert!(
        main_rs.contains("pub request: comline_std::http::Request,"),
        "got: {main_rs}"
    );
}

#[test]
fn external_std_without_any_std_reference_adds_no_dependency() {
    let schemas = vec![("account".to_string(), vec![user_struct()])];
    let files = generate_rust(&external_std_lib_req(&schemas)).unwrap();
    let cargo_toml = &files
        .iter()
        .find(|f| f.path.to_str().unwrap() == "Cargo.toml")
        .unwrap()
        .contents;
    assert!(!cargo_toml.contains("comline_std"), "got: {cargo_toml}");
}

/// A schema of nothing but constants generated an empty file - the
/// declarations were dropped on the floor, in both targets.
#[test]
fn constants_are_emitted_with_the_type_they_declared() {
    let schemas = vec![(
        "limits".to_string(),
        vec![
            FrozenUnit::Constant {
                docstring: Some("How many times a client retries.".to_string()),
                name: "MAX_RETRIES".to_string(),
                ty: TypeRef::Primitive(PrimitiveType::U8), value: Some(Literal::Integer(5)),
                span: (0, 0),
            },
            FrozenUnit::Constant {
                docstring: None,
                name: "SERVICE_NAME".to_string(),
                ty: TypeRef::Primitive(PrimitiveType::String), value: Some(Literal::String("store".to_string())),
                span: (0, 0),
            },
            FrozenUnit::Constant {
                docstring: None,
                name: "DEBUG".to_string(),
                ty: TypeRef::Primitive(PrimitiveType::Bool), value: Some(Literal::Bool(true)),
                span: (0, 0),
            },
            FrozenUnit::Constant {
                docstring: None,
                name: "OFFSET".to_string(),
                ty: TypeRef::Primitive(PrimitiveType::S16), value: Some(Literal::Integer(-3)),
                span: (0, 0),
            },
        ],
    )];
    let files = generate_rust(&code_req(&schemas)).unwrap();
    let out = &files[0].contents;

    assert!(
        out.contains("/// How many times a client retries.\npub const MAX_RETRIES: u8 = 5;"),
        "got: {out}"
    );
    // `u8`, not `u64` - the width survives because core stopped inferring
    // the primitive from the literal.
    assert!(!out.contains("MAX_RETRIES: u64"), "got: {out}");
    // `String` cannot be constructed in a `const`; `&str` rather than
    // `&'static str` because clippy fires on the explicit lifetime.
    assert!(
        out.contains("pub const SERVICE_NAME: &str = \"store\";"),
        "got: {out}"
    );
    assert!(out.contains("pub const DEBUG: bool = true;"), "got: {out}");
    // The IDL spells signed integers `s16`; Rust's own primitive is `i16`.
    assert!(out.contains("pub const OFFSET: i16 = -3;"), "got: {out}");
}

/// Nothing in this file derives anything, so the serde import was pure
/// noise - and a warning in generated code is one a consumer cannot fix.
/// `std/validators` is the real case: it declares only validators, so its
/// generated file *was* the unused import.
#[test]
fn a_file_with_nothing_to_derive_imports_no_serde() {
    let schemas = vec![(
        "limits".to_string(),
        vec![FrozenUnit::Constant {
            docstring: None,
            name: "MAX".to_string(),
            ty: TypeRef::Primitive(PrimitiveType::U8), value: Some(Literal::Integer(1)),
            span: (0, 0),
        }],
    )];
    let out = generate_rust(&code_req(&schemas)).unwrap()[0]
        .contents
        .clone();
    assert!(!out.contains("use serde"), "got: {out}");
    assert!(out.contains("pub const MAX: u8 = 1;"), "got: {out}");
}

/// ...and a file that *does* declare a derived type still imports it.
#[test]
fn a_file_with_a_struct_still_imports_serde() {
    let schemas = vec![("api".to_string(), vec![user_struct()])];
    let out = generate_rust(&code_req(&schemas)).unwrap()[0]
        .contents
        .clone();
    assert!(
        out.contains("use serde::{Deserialize, Serialize};"),
        "got: {out}"
    );
}

/// `! A, B`. The IR has always modelled `throws` as a list, so the generator
/// needed nothing for this — but nothing proved it either, because the
/// grammar could only ever produce one.
#[test]
fn a_function_with_several_throws_gets_a_variant_for_each() {
    let error = |ordinal: u16, name: &str| FrozenUnit::Error {
        docstring: None,
        parameters: vec![],
        ordinal,
        imported_from: None,
        name: name.to_string(),
        message: format!("{name} happened"),
        fields: vec![],
        span: None,
        message_span: None,
        uninterpolated: vec![],
    };
    let proto = FrozenUnit::Protocol {
        docstring: String::new(),
        parameters: vec![],
        name: "Vault".to_string(),
        functions: vec![FrozenUnit::Function {
            docstring: String::new(),
            name: "open".to_string(),
            parameters: vec![],
            arguments: vec![],
            _return: Some(TypeRef::Unit),
            // The `!` clause's own order, which need not be ordinal order.
            throws: vec![1, 0],
            span: (0, 0),
        }],
        span: (0, 0),
    };
    let schemas = vec![(
        "vault".to_string(),
        vec![error(0, "NotFound"), error(1, "Denied"), proto],
    )];
    let src = generate_rust(&code_req(&schemas))
        .unwrap()
        .remove(0)
        .contents;

    // Variants follow the clause, so the author sees what they wrote.
    assert!(
        src.contains("pub enum VaultOpenError {\n    Denied(Denied),\n    NotFound(NotFound),\n}"),
        "{src}"
    );
    // The union is ordinal-sorted and deduplicated, as it always was.
    assert!(
        src.contains("pub enum VaultError {\n    NotFound(NotFound),\n    Denied(Denied),\n}"),
        "{src}"
    );
    // Both raise paths reach the wire at their own ordinal.
    assert!(src.contains("reply.err(1u16, &body);"), "{src}");
    assert!(src.contains("reply.err(0u16, &body);"), "{src}");
    // And both come back typed on the client.
    assert!(src.contains("Envelope::Err { id: 0u16, body } =>"), "{src}");
    assert!(src.contains("Envelope::Err { id: 1u16, body } =>"), "{src}");
    // One Display arm per variant.
    assert!(
        src.contains("VaultOpenError::Denied(e) => e.fmt(f),"),
        "{src}"
    );
    assert!(
        src.contains("VaultOpenError::NotFound(e) => e.fmt(f),"),
        "{src}"
    );
}

/// `protocol Gateway` + `error GatewayError` used to emit two types of that
/// name, and the author met the failure as a redefinition error inside
/// generated code they never wrote.
#[test]
fn a_derived_name_that_collides_with_a_declaration_is_refused() {
    let error = FrozenUnit::Error {
        docstring: None,
        parameters: vec![],
        ordinal: 0,
        imported_from: None,
        name: "GatewayError".to_string(),
        message: "no".into(),
        fields: vec![],
        span: None,
        message_span: None,
        uninterpolated: vec![],
    };
    let proto = FrozenUnit::Protocol {
        docstring: String::new(),
        parameters: vec![],
        name: "Gateway".to_string(),
        functions: vec![],
        span: (0, 0),
    };
    let schemas = vec![("gateway".to_string(), vec![error, proto])];
    let err = generate_rust(&code_req(&schemas)).expect_err("the name is taken");
    let message = err.to_string();
    assert!(
        message.contains("declares error `GatewayError`"),
        "{message}"
    );
    assert!(
        message.contains("protocol `Gateway`'s error union"),
        "{message}"
    );
}

/// The per-function error type has the same hazard, one name deeper.
#[test]
fn a_derived_per_function_name_collides_too() {
    let declared = FrozenUnit::Struct {
        docstring: None,
        parameters: vec![],
        name: "ChatSendError".to_string(),
        fields: vec![],
        span: (0, 0),
    };
    let proto = FrozenUnit::Protocol {
        docstring: String::new(),
        parameters: vec![],
        name: "Chat".to_string(),
        functions: vec![FrozenUnit::Function {
            docstring: String::new(),
            name: "send".to_string(),
            parameters: vec![],
            arguments: vec![],
            _return: Some(TypeRef::Unit),
            throws: vec![],
            span: (0, 0),
        }],
        span: (0, 0),
    };
    let schemas = vec![("chat".to_string(), vec![declared, proto])];
    let err = generate_rust(&code_req(&schemas)).expect_err("the name is taken");
    assert!(
        err.to_string().contains("declares struct `ChatSendError`"),
        "{err}"
    );
}

/// Two protocols can reach for one name without any declaration being
/// involved: the per-function type is `<Proto><Fn>Error`, so `Chat::send` and
/// a `ChatSend` protocol both want `ChatSendError`.
#[test]
fn two_protocols_deriving_the_same_name_are_refused() {
    let chat = FrozenUnit::Protocol {
        docstring: String::new(),
        parameters: vec![],
        name: "Chat".to_string(),
        functions: vec![FrozenUnit::Function {
            docstring: String::new(),
            name: "send".to_string(),
            parameters: vec![],
            arguments: vec![],
            _return: Some(TypeRef::Unit),
            throws: vec![],
            span: (0, 0),
        }],
        span: (0, 0),
    };
    let chat_send = FrozenUnit::Protocol {
        docstring: String::new(),
        parameters: vec![],
        name: "ChatSend".to_string(),
        functions: vec![],
        span: (0, 0),
    };
    let schemas = vec![("chat".to_string(), vec![chat, chat_send])];
    let err = generate_rust(&code_req(&schemas)).expect_err("both want ChatSendError");
    assert!(err.to_string().contains("twice"), "{err}");
}

/// A one-way function derives a params struct but no error type, so a
/// declaration may legitimately take the error name it would have used.
#[test]
fn a_one_way_function_reserves_no_error_name() {
    let declared = FrozenUnit::Struct {
        docstring: None,
        parameters: vec![],
        name: "ChatPokeError".to_string(),
        fields: vec![],
        span: (0, 0),
    };
    let proto = FrozenUnit::Protocol {
        docstring: String::new(),
        parameters: vec![],
        name: "Chat".to_string(),
        functions: vec![FrozenUnit::Function {
            docstring: String::new(),
            name: "poke".to_string(),
            parameters: vec![],
            arguments: vec![],
            _return: None, // one-way
            throws: vec![],
            span: (0, 0),
        }],
        span: (0, 0),
    };
    let schemas = vec![("chat".to_string(), vec![declared, proto])];
    assert!(generate_rust(&code_req(&schemas)).is_ok());
}

/// Unions have no Rust rendering yet: the generator says so, instead of
/// emitting a placeholder that fails to compile somewhere inside generated
/// code.
#[test]
fn a_union_is_refused_with_a_clear_error() {
    let schemas = vec![(
        "api".to_string(),
        vec![FrozenUnit::Struct {
            docstring: None,
            parameters: vec![],
            name: "S".to_string(),
            fields: vec![FrozenUnit::Field {
                docstring: None,
                parameters: vec![],
                optional: false,
                name: "either".to_string(),
                ty: TypeRef::Union(vec![
                    TypeRef::Primitive(PrimitiveType::String),
                    TypeRef::Primitive(PrimitiveType::U32),
                ]),
                default: None,
                span: (0, 0),
            }],
            span: (0, 0),
        }],
    )];
    let err = generate_rust(&code_req(&schemas)).unwrap_err().to_string();
    assert_eq!(
        err,
        "the Rust generator doesn't support unions (unions): Rust has no anonymous sum type to render one as\n  \
         - schema 'api', struct 'S', field 'either'"
    );
}

/// `serde_big_array` reaches a field's own type only - a big fixed array
/// nested in a list, or optional, is refused up front.
#[test]
fn a_big_fixed_array_out_of_reach_of_the_attribute_is_refused() {
    let big = TypeRef::Array { element: Box::new(TypeRef::Primitive(PrimitiveType::U8)), length: Some(64) };
    for (optional, ty) in [
        (true, big.clone()),
        (false, TypeRef::Array { element: Box::new(big.clone()), length: None }),
    ] {
        let schemas = vec![(
            "api".to_string(),
            vec![FrozenUnit::Struct {
                docstring: None,
                parameters: vec![],
                name: "S".to_string(),
                fields: vec![FrozenUnit::Field {
                    docstring: None,
                    parameters: vec![],
                    optional,
                    name: "d".to_string(),
                    ty,
                    default: None,
                    span: (0, 0),
                }],
                span: (0, 0),
            }],
        )];
        let err = generate_rust(&code_req(&schemas)).unwrap_err().to_string();
        assert!(err.contains("doesn't support fixed arrays over 32 elements, nested"), "{err}");
        assert!(err.contains("schema 'api', struct 'S', field 'd'"), "{err}");
    }
}

/// A field that is a big fixed array gets the attribute, and code mode
/// says which dependency that needs.
#[test]
fn a_big_fixed_array_field_uses_serde_big_array() {
    let schemas = vec![(
        "api".to_string(),
        vec![FrozenUnit::Struct {
            docstring: None,
            parameters: vec![],
            name: "S".to_string(),
            fields: vec![FrozenUnit::Field {
                docstring: None,
                parameters: vec![],
                optional: false,
                name: "digest".to_string(),
                ty: TypeRef::Array { element: Box::new(TypeRef::Primitive(PrimitiveType::U8)), length: Some(64) },
                default: None,
                span: (0, 0),
            }],
            span: (0, 0),
        }],
    )];
    let src = generate_rust(&code_req(&schemas)).unwrap().remove(0).contents;
    assert!(src.starts_with("// This file uses `serde_big_array` - add it to your Cargo.toml:\n// serde-big-array = \"0.5\"\n"), "{src}");
    assert!(src.contains("    #[serde(with = \"serde_big_array::BigArray\")]\n    pub digest: [u8; 64],\n"), "{src}");
}

// ── validators ─────────────────────────────────────────────────────────────

fn validated(source: &str) -> Vec<(String, Vec<FrozenUnit>)> {
    use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
    use comline_core::schema::ir::compiler::Compile;
    vec![("app".to_string(), IncrementalInterpreter::from_source(source))]
}

const NON_EMPTY: &str = "validator NonEmpty for string {\n    validate {\n        \
                         assert(value.length > 0, f\"{value.name} is empty\")\n    }\n}\n\
                         struct Note {\n    @validators = [NonEmpty()]\n    text: string\n}\n";

fn app_file(files: &[comline_codegen::GeneratedFile]) -> &str {
    &files
        .iter()
        .find(|f| f.path.ends_with("app.rs"))
        .expect("app.rs")
        .contents
}

#[test]
fn a_checked_struct_implements_validate_and_decodes_unchanged_by_default() {
    let schemas = validated(NON_EMPTY);
    let files = generate_rust(&code_req(&schemas)).unwrap();
    let app = app_file(&files);
    assert!(app.contains("impl comline_runtime::validation::Validate for Note {"), "{app}");
    assert!(app.contains("if !((value.chars().count() as i128) > 0) {"), "{app}");
    assert!(app.contains("message: format!(\"{} is empty\", path),"), "{app}");
    assert!(!app.contains("try_from"), "off leaves decoding alone:\n{app}");
}

#[test]
fn validate_on_decode_first_decodes_through_validate_first() {
    let schemas = validated(NON_EMPTY);
    let req = GenRequest {
        validate_on_decode: comline_codegen::validation::ValidateOnDecode::First,
        ..code_req(&schemas)
    };
    let app = generate_rust(&req).unwrap().remove(0).contents;
    assert!(app.contains("#[serde(try_from = \"__NoteUnchecked\")]\npub struct Note {"), "{app}");
    assert!(app.contains("type Error = comline_runtime::validation::ValidationError;"), "{app}");
    assert!(app.contains("comline_runtime::validation::Validate::validate_first(&value)?;"), "{app}");
}

#[test]
fn code_mode_names_the_runtime_dependency_validation_needs() {
    let schemas = validated(NON_EMPTY);
    let app = generate_rust(&code_req(&schemas)).unwrap().remove(0).contents;
    assert!(
        app.starts_with("// This file uses `comline_runtime` - add it to your Cargo.toml:\n"),
        "{app}"
    );
}

#[test]
fn lib_mode_depends_on_the_runtime_for_validation_alone() {
    let schemas = validated(NON_EMPTY);
    let files = generate_rust(&lib_req(&schemas)).unwrap();
    let manifest = &files.iter().find(|f| f.path.ends_with("Cargo.toml")).unwrap().contents;
    assert!(manifest.contains("comline-runtime = { git = "), "{manifest}");
}

#[test]
fn a_struct_without_checks_gets_no_validate_impl() {
    let schemas = validated("struct Plain {\n    n: u32\n}\n");
    let files = generate_rust(&lib_req(&schemas)).unwrap();
    assert!(!app_file(&files).contains("Validate"));
    let manifest = &files.iter().find(|f| f.path.ends_with("Cargo.toml")).unwrap().contents;
    assert!(!manifest.contains("comline-runtime"), "{manifest}");
}

#[test]
fn a_validator_missing_from_the_request_is_an_error_naming_the_field() {
    let schemas = validated("struct Note {\n    @validators = [Gone()]\n    text: string\n}\n");
    let error = generate_rust(&code_req(&schemas)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "schema 'app', 'Note.text': validator 'Gone' isn't declared in any schema being generated"
    );
}
