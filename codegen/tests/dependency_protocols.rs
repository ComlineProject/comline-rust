//! A package that uses only a dependency's types generates, though the
//! dependency declares a protocol (generation#143): the protocol is generated
//! as a type, but it isn't on the package's connection, so nothing addresses
//! it by wire numbers, checks it, or puts it in the Hello. Nor does it clash
//! with the package's own protocols (generation#147).

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use comline_codegen::protocol::{decode, encode, FileOut, Request, Response};
use comline_codegen::validation::ValidateOnDecode;
use comline_codegen::{connections, facts, GenerateRequest, Mode, PackageMeta, TargetLanguage};
use comline_codegen_rust::generate_rust;
use comline_conformance::{dependency_and_own_protocols, dependency_protocol, Fixture};
use comline_core::schema::ir::frozen::types::{PrimitiveType, TypeRef};
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

/// The request `comline generation` sends for `fx` in `mode`: with the
/// package's dependencies, and the facts of its own schemas when a protocol
/// of its own needs them.
fn request(fx: &Fixture, mode: Mode) -> GenerateRequest {
    let dependencies: Vec<String> = fx.dependencies.iter().map(|d| d.to_string()).collect();
    let own: Vec<_> = fx
        .schemas
        .iter()
        .filter(|(namespace, _)| !connections::is_dependency(&dependencies, namespace))
        .cloned()
        .collect();
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
        validate_on_decode: ValidateOnDecode::Off,
        async_api: false,
        encodings: comline_codegen::encodings::official_for("rust"),
        facts: None,
        dependencies,
    };
    if facts::needed_in(&req) {
        req.facts = Some(comline_conformance::facts_for(&own, "packed@1"));
    }
    req
}

/// The generated file at `path`'s contents.
fn file<'a>(files: &'a [comline_codegen::GeneratedFile], path: &str) -> Option<&'a str> {
    files
        .iter()
        .find(|f| f.path.to_str() == Some(path))
        .map(|f| f.contents.as_str())
}

#[test]
fn a_package_using_only_a_dependencys_types_generates() {
    for mode in [Mode::Code, Mode::Lib] {
        let req = request(&dependency_protocol(), mode);
        assert!(req.facts.is_none());
        let files = generate_rust(&req).unwrap_or_else(|e| panic!("{mode:?}: {e:#}"));

        // The dependency's protocol is a type of the generated code, with its
        // trait, sender and dispatcher...
        // (`lib` mode keeps its modules under `src/`.)
        let path = match mode {
            Mode::Code => "localdeps/greeting.rs",
            Mode::Lib => "src/localdeps/greeting.rs",
        };
        let greeting = file(&files, path).unwrap_or_else(|| panic!("{mode:?}: {files:#?}"));
        assert!(greeting.contains("trait Foo"), "{greeting}");
        assert!(greeting.contains("FooSender"), "{greeting}");
        assert!(greeting.contains("FooDispatcher"), "{greeting}");
        // ...addressed by position, which no facts were asked for.
        assert!(greeting.contains("Call::new(0, \"greet\")"), "{greeting}");

        // But there's no package connection to put it on, and no Hello.
        assert!(
            files.iter().all(|f| !f.path.ends_with("connection.rs")),
            "{mode:?}"
        );
        assert!(
            files
                .iter()
                .all(|f| !f.contents.contains("HELLO") && !f.contents.contains("HEADER")),
            "{mode:?}"
        );
        assert!(
            files.iter().all(|f| !f.contents.contains("Services")),
            "{mode:?}"
        );
    }
}

#[test]
fn a_dependencys_protocol_is_off_the_package_connection_beside_the_packages_own() {
    let req = request(&dependency_and_own_protocols(), Mode::Code);
    let facts = req
        .facts
        .as_ref()
        .expect("the package's own protocol needs facts");
    let numbered: Vec<_> = facts.protocols.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(numbered, ["api::Chat"]);
    let chat = facts.protocols[0].number;

    // The connection carries only `Chat`, and so does the Hello.
    let connection = connections::package_connection_among(&req.schemas, &req.dependencies)
        .unwrap()
        .expect("the package's own protocol is on a connection");
    let served: Vec<_> = connection
        .server
        .iter()
        .map(|p| format!("{}::{}", p.namespace, p.name))
        .collect();
    assert_eq!(served, ["api::Chat"]);
    assert!(connection.client.is_empty());
    let (_, hello) = facts::hello(&req, &connection).unwrap();
    assert_eq!(
        hello
            .iter()
            .map(|p| (p.name.as_str(), p.number))
            .collect::<Vec<_>>(),
        [("Chat", chat)]
    );

    // Counting the dependency's protocol, as a caller that can't tell it
    // from the package's does, puts it on the connection too.
    let counted = connections::package_connection_of(&req.schemas)
        .unwrap()
        .unwrap();
    assert_eq!(counted.server.len(), 2);

    // What's generated agrees: `Services` provides `Chat` alone.
    let files = generate_rust(&req).unwrap();
    let connection = file(&files, "connection.rs").expect("the package connection");
    assert!(connection.contains("Chat"), "{connection}");
    assert!(!connection.contains("Foo"), "{connection}");
    let hello = format!("{hello:?}");
    assert!(!hello.contains("Foo"), "{hello}");

    // `Chat` is addressed by its wire number, `Foo` by position.
    let mut addressed = BTreeMap::new();
    for (namespace, units) in &req.schemas {
        for unit in units {
            let FrozenUnit::Protocol {
                name,
                parameters,
                functions,
                ..
            } = unit
            else {
                continue;
            };
            let at = facts::addressing_in(&req, namespace, name, parameters, functions).unwrap();
            addressed.insert(name.as_str(), (at.protocol, at.checks.is_some()));
        }
    }
    assert_eq!(addressed["Chat"], (chat, true));
    assert_eq!(addressed["Foo"], (0, false));
    let greeting = file(&files, "localdeps/greeting.rs").unwrap();
    assert!(greeting.contains("trait Foo"), "{greeting}");
}

/// A `jsonrpc` protocol with one function, called by that bare name, which
/// the server side provides, or the client side when `client`.
fn jsonrpc_protocol(name: &str, function: &str, client: bool) -> FrozenUnit {
    let mut parameters = vec![FrozenUnit::Property {
        name: "framing".to_string(),
        value: ConfigValue::Str("jsonrpc".to_string()),
    }];
    if client {
        parameters.push(FrozenUnit::Property {
            name: "provider".to_string(),
            value: ConfigValue::Name("Client".to_string()),
        });
    }
    FrozenUnit::Protocol {
        docstring: String::new(),
        parameters,
        name: name.to_string(),
        functions: vec![FrozenUnit::Function {
            docstring: String::new(),
            parameters: vec![],
            name: function.to_string(),
            arguments: vec![],
            _return: Some(TypeRef::Primitive(PrimitiveType::U32)),
            throws: vec![],
            span: (0, 0),
        }],
        span: (0, 0),
    }
}

/// A dependency's `jsonrpc` protocol with a function the package's own has is
/// no clash: bare method names only have to tell apart what is on one
/// connection, and the dependency's isn't on the package's (generation#147).
/// Two of the package's own with that function still are.
#[test]
fn a_dependencys_jsonrpc_function_name_is_no_clash_with_the_packages_own() {
    // The package's `Chat` and its client's `Events` share a connection; the
    // dependency's `Greeter` has `Chat`'s `send` too.
    let fixture = |own: Vec<FrozenUnit>| {
        let mut fx = dependency_protocol();
        fx.name = "dependency_jsonrpc_and_own";
        fx.schemas = vec![
            ("api".to_string(), own),
            (
                "localdeps/x".to_string(),
                vec![jsonrpc_protocol("Greeter", "send", false)],
            ),
        ];
        fx
    };
    let chat = || jsonrpc_protocol("Chat", "send", false);
    let events = || jsonrpc_protocol("Events", "notify", true);
    for mode in [Mode::Code, Mode::Lib] {
        let req = request(&fixture(vec![chat(), events()]), mode);
        // A check that counts the dependency's protocol refuses it...
        assert!(
            connections::check(&req.schemas, &req.encodings).is_err(),
            "{mode:?}: the fixture clashes when the dependency's protocol is counted"
        );
        // ...and generation doesn't.
        let files = generate_rust(&req).unwrap_or_else(|e| panic!("{mode:?}: {e:#}"));
        let (api, greeter) = match mode {
            Mode::Code => ("api.rs", "localdeps/x.rs"),
            Mode::Lib => ("src/api.rs", "src/localdeps/x.rs"),
        };
        let api = file(&files, api).unwrap_or_else(|| panic!("{mode:?}: {files:#?}"));
        assert!(api.contains("trait Chat"), "{mode:?}: {api}");
        let greeter = file(&files, greeter).unwrap_or_else(|| panic!("{mode:?}: {files:#?}"));
        assert!(greeter.contains("trait Greeter"), "{mode:?}: {greeter}");

        // Two of the package's own with that function still clash.
        let admin = jsonrpc_protocol("Admin", "send", false);
        let req = request(&fixture(vec![chat(), admin, events()]), mode);
        let refused = generate_rust(&req).expect_err("two of the package's own clash");
        assert!(format!("{refused:#}").contains("send"), "{refused:#}");
    }
}

/// `request` on `comline-gen-rust`'s stdin, its stdout decoded.
fn run(request: &Request) -> Response {
    let mut child = Command::new(env!("CARGO_BIN_EXE_comline-gen-rust"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn comline-gen-rust");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&encode(request).unwrap())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "comline-gen-rust exited with {}",
        out.status
    );
    decode(&out.stdout).expect("a Response on stdout")
}

/// The dependencies travel in the request to the generator process, which
/// generates what the linked generator does.
#[test]
fn the_binary_leaves_a_dependencys_protocol_off_the_connection() {
    for fx in [dependency_protocol(), dependency_and_own_protocols()] {
        let req = request(&fx, Mode::Code);
        let expected: Vec<FileOut> = generate_rust(&req)
            .unwrap()
            .into_iter()
            .map(FileOut::from)
            .collect();
        let Response::Generate(generated) = run(&Request::generate(req)) else {
            panic!("{}: not a Generate", fx.name)
        };
        assert_eq!(generated.files, expected, "{}", fx.name);
    }
}
