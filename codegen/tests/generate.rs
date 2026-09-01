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
            synchronous: true,
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
    assert!(src.contains("pub trait UserService"));
    assert!(src.contains("fn get_user(id: i32) -> User;"));
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
