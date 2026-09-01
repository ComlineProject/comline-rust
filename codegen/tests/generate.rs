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
            FrozenUnit::Function {
                docstring: String::new(),
                name: "send".to_string(),
                parameters: vec![],
                arguments: vec![FrozenArgument {
                    name: "body".to_string(),
                    kind: KindValue::Namespaced("string".to_string(), None),
                    span: (0, 0),
                }],
                _return: None, // one-way today: empty ack
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
        ],
        span: (0, 0),
    };

    let schemas = vec![("chat".to_string(), vec![rejected, proto])];
    let src = generate_rust(&code_req(&schemas)).unwrap().remove(0).contents;

    // the error struct
    assert!(src.contains("pub struct Rejected {"));
    // per-function enum from the throw ordinal
    assert!(src.contains("pub enum ChatSendError {\n    Rejected(Rejected),\n}"));
    // a non-throwing function still gets an (empty) enum
    assert!(src.contains("pub enum ChatPingError {\n}"));
    // per-protocol union + From impl
    assert!(src.contains("pub enum ChatError {\n    Rejected(Rejected),\n}"));
    assert!(src.contains("impl From<ChatSendError> for ChatError"));
    // dispatcher encodes the error at its ordinal
    assert!(src.contains("Envelope::encode_err(0u16, &body, out);"));
    // client maps that ordinal back
    assert!(src.contains("Envelope::Err { id: 0u16, body } =>"));
    // zero-arg call sends `&()`
    assert!(src.contains("self.0.call(1u16, &())"));
    // unit / one-way returns render as `()`
    assert!(src.contains("fn ping(&self) -> Result<(), ChatPingError>;"));
    // a `str` arg is borrowed: `&str` in the signature, `&'de str` in the
    // params struct (decoded borrowed from the receive buffer)
    assert!(src.contains("fn send(&self, body: &str) -> Result<(), ChatSendError>;"));
    assert!(src.contains("pub struct ChatSendParams<'a> {\n    #[serde(borrow)]\n    pub body: &'a str,\n}"));
    assert!(src.contains("pub fn send(&mut self, body: &str) -> Result<(), CallError<ChatSendError>>"));
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
