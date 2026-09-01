use comline_codegen::{GenRequest, Mode, PackageMeta};
use comline_codegen_rust::generate_rust;
use comline_core::schema::ir::frozen::unit::{FrozenUnit, FrozenArgument};
use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};

fn code_req(schemas: &[(String, Vec<FrozenUnit>)]) -> GenRequest<'_> {
    GenRequest {
        mode: Mode::Code,
        schemas,
        package: PackageMeta { name: "test".into(), version: "0.1.0".into() },
    }
}

fn lib_req(schemas: &[(String, Vec<FrozenUnit>)]) -> GenRequest<'_> {
    GenRequest {
        mode: Mode::Lib,
        schemas,
        package: PackageMeta { name: "chat".into(), version: "0.3.0".into() },
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
                kind_value: KindValue::Namespaced("s32".to_string(), None),
                span: (0, 0),
            },
            FrozenUnit::Field {
                docstring: None,
                parameters: vec![],
                optional: false,
                name: "username".to_string(),
                kind_value: KindValue::Namespaced("string".to_string(), None),
                span: (0, 0),
            },
            FrozenUnit::Field {
                docstring: None,
                parameters: vec![],
                optional: false,
                name: "tags".to_string(),
                kind_value: KindValue::Namespaced("string[]".to_string(), None),
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
            FrozenUnit::EnumVariant(KindValue::EnumVariant("Active".to_string(), None), (0, 0)),
            FrozenUnit::EnumVariant(KindValue::EnumVariant("Inactive".to_string(), None), (0, 0)),
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
                kind: KindValue::Primitive(Primitive::S32(None)),
                span: (0, 0),
            }],
            _return: Some(KindValue::Namespaced("User".to_string(), None)),
            throws: vec![],
            span: (0, 0),
        }],
        span: (0, 0),
    };

    let schemas = vec![("account".to_string(), vec![enum_unit, proto])];
    let src = generate_rust(&code_req(&schemas)).unwrap().remove(0).contents;

    assert!(src.contains("pub enum Status"));
    assert!(src.contains("Active,"));
    // provider trait: `&self`, a `Result`, a per-function error enum
    assert!(src.contains("pub trait UserService {"));
    assert!(src.contains(
        "fn get_user(&self, id: i32) -> Result<User, UserServiceGetUserError>;"
    ));
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
    assert!(src.contains(
        "let hs = Handshake::new(IR_HASH, format.name(), FRAMING_DATAGRAM, 0);"
    ));
    assert!(src.contains("impl<S: UserService> UserServiceDispatcher<S> {"));
    assert!(src.contains(
        "pub fn serve<T: Transport, W: WireFormat>(self, transport: &mut T, format: W)"
    ));
    assert!(src.contains("Server::new(self, format).serve_handshaked(transport, hs)"));
}

#[test]
fn a_schema_without_a_protocol_has_no_ir_hash() {
    let schemas = vec![("plain".to_string(), vec![user_struct()])];
    let src = generate_rust(&code_req(&schemas)).unwrap().remove(0).contents;
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
            kind_value: KindValue::Namespaced("string".to_string(), None),
            span: (0, 0),
        }],
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
                    kind: KindValue::Namespaced("string".to_string(), None),
                    span: (0, 0),
                }],
                _return: Some(KindValue::Unit),
                throws: vec![0],
                span: (0, 0),
            },
            FrozenUnit::Function {
                docstring: String::new(),
                name: "ping".to_string(),
                parameters: vec![],
                arguments: vec![],
                _return: Some(KindValue::Unit),
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
                    kind: KindValue::Namespaced("string".to_string(), None),
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
    let src = generate_rust(&code_req(&schemas)).unwrap().remove(0).contents;

    // the error struct
    assert!(src.contains("pub struct Rejected {"));
    // per-function enum from the throw ordinal
    assert!(src.contains("pub enum ChatSendError {\n    Rejected(Rejected),\n}"));
    // a non-throwing request/response function still gets an (empty) enum
    assert!(src.contains("pub enum ChatPingError {\n}"));
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
    assert!(src.contains("fn ping(&self) -> Result<(), ChatPingError>;"));
    // both addresses on the wire; the framing picks
    assert!(src.contains(r#"self.0.call(Call::new(1, "ping"), &())"#));
    // a `str` arg is borrowed
    assert!(src.contains("fn send(&self, body: &str) -> Result<(), ChatSendError>;"));
    assert!(src.contains("pub struct ChatSendParams<'a> {\n    #[serde(borrow)]\n    pub body: &'a str,\n}"));
    assert!(src.contains("pub fn send(&mut self, body: &str) -> Result<(), CallError<ChatSendError>>"));

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
                    expression: Some("2500".to_string()),
                }],
                arguments: vec![],
                _return: Some(KindValue::Primitive(Primitive::U32(None))),
                throws: vec![],
                span: (0, 0),
            },
            // no annotation
            FrozenUnit::Function {
                docstring: String::new(),
                name: "fast".to_string(),
                parameters: vec![],
                arguments: vec![],
                _return: Some(KindValue::Primitive(Primitive::U32(None))),
                throws: vec![],
                span: (0, 0),
            },
        ],
        span: (0, 0),
    };
    let schemas = vec![("api".to_string(), vec![proto])];
    let src = generate_rust(&code_req(&schemas)).unwrap().remove(0).contents;

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
            expression: Some("jsonrpc".to_string()),
        }],
        name: "Rpc".to_string(),
        functions: vec![FrozenUnit::Function {
            docstring: String::new(),
            name: "now".to_string(),
            parameters: vec![],
            arguments: vec![],
            _return: Some(KindValue::Primitive(Primitive::U32(None))),
            throws: vec![],
            span: (0, 0),
        }],
        span: (0, 0),
    };
    let schemas = vec![("rpc".to_string(), vec![proto])];
    let src = generate_rust(&code_req(&schemas)).unwrap().remove(0).contents;

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
                kind: KindValue::Namespaced("string".to_string(), None),
                span: (0, 0),
            }],
            _return: None,
            throws: vec![],
            span: (0, 0),
        }],
        span: (0, 0),
    };
    let schemas = vec![("bus".to_string(), vec![proto])];
    let src = generate_rust(&code_req(&schemas)).unwrap().remove(0).contents;
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

    assert!(by_path("src/account.rs").expect("src/account.rs").contents.contains("pub struct User"));
    assert!(by_path("src/billing.rs").is_some());
}

#[test]
fn lib_mode_rejects_nested_namespaces() {
    let schemas = vec![("account/user".to_string(), vec![user_struct()])];
    let err = generate_rust(&lib_req(&schemas)).unwrap_err().to_string();
    assert!(err.contains("nested namespaces"));
}
