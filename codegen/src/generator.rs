//! Rust generation.
//!
//! - `code` — one `<namespace>.rs` per schema: serde data types, C-like enums,
//!   and, per `protocol`, the full RPC shape (params structs, error enums, a
//!   provider trait, a `Dispatch` impl, and a `Client`-backed stub) against
//!   `comline-runtime`'s surface-4 contract.
//! - `lib` — a buildable crate: `Cargo.toml` + `src/lib.rs` (the `pub mod`
//!   tree) + `src/<namespace>.rs`. See design/generation.md.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
use comline_core::schema::ir::frozen::schema_ir_hash;
use comline_core::schema::ir::frozen::unit::{FrozenArgument, FrozenUnit};

use eyre::Result;

use comline_codegen::collisions::{self, Derived};
use comline_codegen::modules::{NamespaceTree, TreeLayout};
use comline_codegen::naming::{pascal, screaming_snake};
use comline_codegen::stdlib::{self, is_std_namespace};
use comline_codegen::symbols::{error_names, Origin, SymbolTable};
use comline_codegen::{GenRequest, GeneratedFile, Mode, PackageMeta};

/// `comline-runtime` — the crate the generated RPC code links against. Pinned
/// by git rev (no crates.io yet); see design/runtime-repo-structure.md.
const RUNTIME_GIT: &str = "https://github.com/ComlineProject/runtime";
const RUNTIME_REV: &str = "ae214701b08521f2e0f0a480afa387d0ec4d646c";

/// `comline_std` — std's own pre-generated bindings, referenced instead of
/// inlined when `external_std` is on. Same git-pin shape as
/// `comline-runtime` above, pointing at `comline-rust`'s own `std-extra/`
/// (see `xtask/`, which regenerates it and is what this pin must match).
const STD_GIT: &str = "https://github.com/ComlineProject/comline-rust";
pub const STD_REV: &str = "30a70cb30dfbd9e1812e85ea1949df02410c4d7e";

pub fn generate_rust(req: &GenRequest) -> Result<Vec<GeneratedFile>> {
    let default_framing = req.default_framing.as_deref();

    // Only *emission* is narrowed by `external_std`; `req.schemas` stays
    // the list every reference resolves against. `stdlib::split` carries
    // that distinction, and the "declare the dependency only if std was
    // actually there" subtlety with it.
    let std = stdlib::split(req.schemas, req.external_std);
    let schemas = std.emitted.as_ref();

    // Before emitting anything: a name this generator is about to invent may
    // already mean something in the schema. `protocol Gateway` plus an
    // `error GatewayError` used to emit two types of that name and leave the
    // author with a redefinition error pointing into code they never wrote.
    // Checked for every schema first, so a collision in the last one does not
    // arrive after the earlier files are already written.
    for (namespace, units) in schemas {
        collisions::check(namespace, units, derived_names(units))?;
    }

    match req.mode {
        // No manifest to add `comline_std` to on the caller's behalf here
        // (unlike `lib` mode's `Cargo.toml`, below) - a file that ends up
        // referencing it gets a comment naming the dependency line to add
        // by hand.
        Mode::Code => Ok(schemas
            .iter()
            .map(|(namespace, units)| {
                let contents =
                    schema_source(units, req.schemas, default_framing, req.external_std);
                let contents = if contents.contains("comline_std::") {
                    format!(
                        "// This file references `comline_std` - add it to your Cargo.toml:\n\
                         // comline_std = {{ git = \"{STD_GIT}\", rev = \"{STD_REV}\" }}\n{contents}"
                    )
                } else {
                    contents
                };
                GeneratedFile { path: PathBuf::from(format!("{namespace}.rs")), contents }
            })
            .collect()),

        Mode::Lib => {
            let tree = NamespaceTree::build(schemas, &RustLayout)?;

            let has_protocol = schemas
                .iter()
                .any(|(_, units)| units.iter().any(|u| matches!(u, FrozenUnit::Protocol { .. })));

            let mut files = vec![GeneratedFile {
                path: PathBuf::from("Cargo.toml"),
                contents: cargo_toml(
                    &req.package,
                    &req.lang_version,
                    has_protocol,
                    std.needs_std_dependency,
                ),
            }];
            // The render closure takes every schema in the request, not just
            // the ones in this tree: a reference resolves against all of
            // them, including an external `std` this crate emits no file for.
            files.extend(tree.emit("src", &RustLayout, &|_namespace, units| {
                schema_source(units, req.schemas, default_framing, req.external_std)
            }));
            Ok(files)
        }
    }
}

/// What Rust decides differently about the `src/` tree
/// ([`comline_codegen::modules`] holds the rest).
///
/// Three things: the extension, a module file *beside* its directory rather
/// than inside it (2021-edition style — `src/std.rs` + `src/std/http.rs`, no
/// `mod.rs`), with the crate root the one special case (`src/lib.rs`, not
/// `src.rs`), and `pub mod` as the declaration — which, unlike TypeScript's
/// `export * as`, is the same line whether the child is a schema or a
/// grouping, since Rust resolves `foo` to `foo.rs` or `foo/` itself.
///
/// Plus the one reserved root: `lib` is the crate root.
struct RustLayout;

impl TreeLayout for RustLayout {
    fn ext(&self) -> &str {
        "rs"
    }

    /// Beside the directory, except at the crate root.
    fn group_file(&self, prefix: &str, is_root: bool) -> String {
        if is_root {
            format!("{prefix}/lib.rs")
        } else {
            format!("{prefix}.rs")
        }
    }

    fn declare_child(&self, segment: &str, _child_is_leaf: bool) -> String {
        format!("pub mod {segment};\n")
    }

    fn reserved_roots(&self) -> &[&str] {
        &["lib"]
    }

    /// `src/std.rs` has to be *either* `std`'s own generated code *or* its
    /// `pub mod` declarations, and there is nowhere to put the other.
    fn ambiguity_reason(&self, _path: &str) -> String {
        "there's no single file for that to be".to_string()
    }

    /// A schema named `lib` used to be emitted straight over `src/lib.rs`,
    /// taking the whole `pub mod` tree with it. Core refuses the name when a
    /// package is compiled; this catches a `GenRequest` built by hand, since
    /// that is a public contract.
    ///
    /// The check is on a namespace's *first* segment, so `lib/foo` is
    /// refused too — it would make `lib` a grouping node whose own file is
    /// `src/lib.rs`, clobbering the crate root exactly as a schema named
    /// `lib` did. Core's own check covers both from a real project
    /// (`reserved_in_namespace` looks at every segment); this is for a
    /// `GenRequest` built by hand.
    fn reserved_root_reason(&self, _name: &str) -> String {
        "`src/lib.rs` is the crate root, so a schema there would overwrite \
         the module tree - rename the schema"
            .to_string()
    }
}

/// `lang_version` is the toolchain the target declared (`rust#1.70.0`), so
/// the crate states the version it was generated for rather than leaving a
/// consumer to discover it by failing to build.
fn cargo_toml(
    pkg: &PackageMeta,
    lang_version: &str,
    has_protocol: bool,
    needs_std_dep: bool,
) -> String {
    let mut deps = String::from("serde = { version = \"1\", features = [\"derive\"] }\n");
    if has_protocol {
        deps.push_str(&format!(
            "comline-runtime = {{ git = \"{RUNTIME_GIT}\", rev = \"{RUNTIME_REV}\" }}\n"
        ));
    }
    if needs_std_dep {
        deps.push_str(&format!(
            "comline_std = {{ git = \"{STD_GIT}\", rev = \"{STD_REV}\" }}\n"
        ));
    }
    format!(
        "# Generated by Comline\n\
         [package]\n\
         name = \"{}\"\n\
         version = \"{}\"\n\
         edition = \"2021\"\n\
         rust-version = \"{lang_version}\"\n\
         # A schema namespace named `main` would otherwise be picked up as a\n\
         # binary target (`src/main.rs`) with no `fn main`. Core refuses that\n\
         # name outright now; this stays as a belt-and-braces guard for a\n\
         # `GenRequest` built by hand.\n\
         autobins = false\n\
         \n\
         [dependencies]\n\
         {deps}",
        pkg.name, pkg.version
    )
}

fn schema_source(
    units: &[FrozenUnit],
    schemas: &[(String, Vec<FrozenUnit>)],
    default_framing: Option<&str>,
    external_std: bool,
) -> String {
    let has_protocol = units
        .iter()
        .any(|u| matches!(u, FrozenUnit::Protocol { .. }));
    let errors = error_names(units);
    let imports = import_paths(units, schemas, external_std);

    // Which framings the protocols in this file reach for. A non-datagram pick
    // pulls in the `Framing` trait (its helpers call `framing.name()`); the
    // datagram default pulls in the `FRAMING_DATAGRAM` name constant.
    let protocol_framings = || {
        units.iter().filter_map(|u| match u {
            FrozenUnit::Protocol { parameters, .. } => {
                Some(framing_choice(parameters, default_framing))
            }
            _ => None,
        })
    };
    let needs_framing_trait = protocol_framings().any(|f| f.explicit);
    let needs_datagram_const = protocol_framings().any(|f| !f.explicit);

    // Only a `derive`d type needs serde, and a schema can legitimately
    // declare none - `std/validators` declares only validators, so its
    // generated file was an unused import and nothing else, which is a
    // warning a consumer cannot do anything about. A `type` alias needs no
    // derive either; a protocol does, for its generated params structs.
    let needs_serde = units.iter().any(|u| {
        matches!(
            u,
            FrozenUnit::Struct { .. }
                | FrozenUnit::Error { .. }
                | FrozenUnit::Enum { .. }
                | FrozenUnit::Protocol { .. }
        )
    });

    let mut output = String::new();
    output.push_str("// Generated by Comline\n");
    if needs_serde {
        output.push_str("use serde::{Deserialize, Serialize};\n");
    }
    if has_protocol && !needs_framing_trait {
        // The common case — kept byte-for-byte for the conformance goldens.
        output.push_str(
            "use comline_runtime::client::Client;\n\
             use comline_runtime::contract::{\n    \
                 Call, CallError, Dispatch, Envelope, Fault, Handshake, Kind, Reply,\n    \
                 RuntimeError, WireFormat, FRAMING_DATAGRAM,\n\
             };\n\
             use comline_runtime::serve::Server;\n\
             use comline_runtime::transport::Transport;\n",
        );
    } else if has_protocol {
        let mut names = vec![
            "Call",
            "CallError",
            "Dispatch",
            "Envelope",
            "Fault",
            "Framing",
            "Handshake",
            "Kind",
            "Reply",
            "RuntimeError",
            "WireFormat",
        ];
        if needs_datagram_const {
            names.push("FRAMING_DATAGRAM");
        }
        output.push_str("use comline_runtime::client::Client;\n");
        output.push_str("use comline_runtime::contract::{\n");
        for name in names {
            output.push_str("    ");
            output.push_str(name);
            output.push_str(",\n");
        }
        output.push_str(
            "};\n\
             use comline_runtime::serve::Server;\n\
             use comline_runtime::transport::Transport;\n",
        );
    }
    output.push('\n');

    if has_protocol {
        output.push_str(&format!(
            "/// Canonical digest of the frozen IR this file was generated from — the\n\
             /// two ends of a connection [`Handshake`] must agree on it.\n\
             pub const IR_HASH: u64 = {:#018x};\n\n",
            schema_ir_hash(units)
        ));
    }

    for unit in units {
        match unit {
            FrozenUnit::Struct { name, fields, .. } => {
                output.push_str(&data_struct(name, fields, &imports));
            }
            FrozenUnit::Enum { name, variants, .. } => {
                output.push_str(&data_enum(name, variants));
            }
            FrozenUnit::Error {
                name,
                message,
                fields,
                ..
            } => {
                output.push_str(&error_struct(name, message, fields, &imports));
            }
            FrozenUnit::TypeAlias { name, target, .. } => {
                output.push_str(&type_alias(name, target, &imports));
            }
            FrozenUnit::Constant {
                docstring,
                name,
                kind_value,
                ..
            } => {
                output.push_str(&constant(docstring.as_deref(), name, kind_value));
            }
            FrozenUnit::Protocol {
                name,
                functions,
                parameters,
                ..
            } => {
                output.push_str(&protocol(
                    name,
                    functions,
                    parameters,
                    default_framing,
                    &errors,
                    &imports,
                ));
            }
            _ => {}
        }
    }

    output
}

// ── data types ─────────────────────────────────────────────────────────────

fn field_lines(fields: &[FrozenUnit], imports: &HashMap<String, String>) -> String {
    let mut s = String::new();
    for field in fields {
        if let FrozenUnit::Field {
            name,
            kind_value,
            optional,
            ..
        } = field
        {
            let ty = rust_type(kind_value, imports);
            let ty = if *optional {
                format!("Option<{ty}>")
            } else {
                ty
            };
            s.push_str(&format!("    pub {name}: {ty},\n"));
        }
    }
    s
}

fn data_struct(name: &str, fields: &[FrozenUnit], imports: &HashMap<String, String>) -> String {
    format!(
        "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct {name} {{\n{}}}\n\n",
        field_lines(fields, imports)
    )
}

/// `type NAME = TARGET;` - transparent like Rust's own `type` (not a
/// newtype): `rust_type` already maps any `KindValue` to a real Rust type
/// expression, named or primitive, so this is the same mapping every field
/// already goes through, just at the top level of the file instead of
/// inside a struct.
fn type_alias(name: &str, target: &KindValue, imports: &HashMap<String, String>) -> String {
    format!("pub type {name} = {};\n\n", rust_type(target, imports))
}

/// `const MAX_RETRIES: u8 = 5` -> `pub const MAX_RETRIES: u8 = 5;`.
///
/// Declarations used to vanish here: a schema of nothing but constants
/// generated an empty file in both targets. The type comes from the
/// primitive the IR carries, which is only trustworthy since core stopped
/// inferring it from the literal (core#92) - before that a `u8` constant
/// would have been emitted as `u64`.
///
/// Rendered from the primitive directly rather than through
/// [`rust_type`]: a `const` is not a struct field, and the two differ. A
/// text constant is `&str` - `String` cannot be built in a `const`, and the
/// explicit `'static` a `const` would accept is what clippy's
/// `redundant_static_lifetimes` fires on, which is the same generated-code
/// noise the conditional serde import above exists to avoid.
fn constant(docstring: Option<&str>, name: &str, kind_value: &KindValue) -> String {
    let KindValue::Primitive(primitive) = kind_value else {
        // Validation refuses a constant of a named type ("only primitives
        // allowed"), so this is unreachable from a real build. Emitting
        // nothing rather than guessing keeps it that way.
        return String::new();
    };
    let Some(literal) = primitive.value_str() else {
        // The grammar requires `= VALUE`, so a constant always has one.
        return String::new();
    };

    let (ty, value) = match primitive {
        Primitive::String(_) => ("&str".to_string(), format!("{literal:?}")),
        Primitive::Boolean(_) => ("bool".to_string(), literal),
        // Every integer width names itself in Rust except the IDL's
        // `s`-prefixed signed ones, which `map_str_type` already translates.
        other => (map_str_type(other.name(), &HashMap::new()), literal),
    };

    let mut s = String::new();
    for line in docstring.unwrap_or_default().lines() {
        s.push_str(&format!("/// {line}\n"));
    }
    s.push_str(&format!("pub const {name}: {ty} = {value};\n\n"));
    s
}

/// An `error`'s payload struct, plus the `Display` that renders the `message`
/// the schema requires. Without it the sentence the author wrote was dropped
/// on the floor and a raised error printed as its `Debug` shape at best.
///
/// No `core::error::Error` impl: that trait is only stable from Rust 1.81,
/// and this generator's target declares `rust#1.70.0` (`LANG_VERSION`, which
/// it also writes into the generated manifest as `rust-version`). It belongs
/// with a `rust#1.81.0` target.
fn error_struct(
    name: &str,
    message: &str,
    fields: &[FrozenUnit],
    imports: &HashMap<String, String>,
) -> String {
    let field_names: Vec<String> = fields
        .iter()
        .filter_map(|f| match f {
            FrozenUnit::Field { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    format!(
        "#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]\npub struct {name} {{\n{}}}\n\n\
         impl core::fmt::Display for {name} {{\n    \
             fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {{\n        \
                 {}\n    \
             }}\n\
         }}\n\n",
        field_lines(fields, imports),
        crate::message::render(message, &field_names).write_call("f"),
    )
}

fn data_enum(name: &str, variants: &[FrozenUnit]) -> String {
    let mut s = format!(
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]\npub enum {name} {{\n"
    );
    for variant in variants {
        if let FrozenUnit::EnumVariant(kv, _) = variant {
            let variant_name = match kv {
                KindValue::EnumVariant(n, _) | KindValue::Namespaced(n, _) => n.clone(),
                _ => "Unknown".to_string(),
            };
            s.push_str(&format!("    {variant_name},\n"));
        }
    }
    s.push_str("}\n\n");
    s
}

/// Every name this generator will invent for `units`, each described the way a
/// person reading a build failure needs it.
///
/// This is the half of collision checking that cannot live in
/// `comline-codegen`: what gets derived is a fact about *Rust* output, not
/// about the schema. Keep it in step with the emitters below — a name added
/// there and not here is a collision that still reaches the compiler.
fn derived_names(units: &[FrozenUnit]) -> Vec<Derived> {
    let mut derived = Vec::new();
    let mut any_protocol = false;

    for unit in units {
        let FrozenUnit::Protocol {
            name: proto,
            functions,
            ..
        } = unit
        else {
            continue;
        };
        any_protocol = true;

        derived.push(Derived::new(
            format!("{proto}Error"),
            format!("protocol `{proto}`'s error union"),
        ));
        derived.push(Derived::new(
            format!("{proto}Dispatcher"),
            format!("protocol `{proto}`'s dispatcher"),
        ));
        derived.push(Derived::new(
            format!("{proto}Client"),
            format!("protocol `{proto}`'s client"),
        ));
        derived.push(Derived::new(
            format!("{}_CALLS", screaming_snake(proto)),
            format!("protocol `{proto}`'s call table"),
        ));

        for function in functions {
            let FrozenUnit::Function {
                name,
                arguments,
                _return,
                ..
            } = function
            else {
                continue;
            };
            let from = format!("protocol `{proto}`, function `{name}`");
            // A one-way function gets no error type (nowhere to deliver one).
            if _return.is_some() {
                derived.push(Derived::new(
                    format!("{proto}{}Error", pascal(name)),
                    format!("{from}'s error type"),
                ));
            }
            if !arguments.is_empty() {
                derived.push(Derived::new(
                    format!("{proto}{}Params", pascal(name)),
                    format!("{from}'s params struct"),
                ));
            }
        }
    }

    if any_protocol {
        derived.push(Derived::new("IR_HASH", "the schema's IR hash constant"));
    }
    derived
}

/// One generated error type: the per-function `<Proto><Fn>Error`, or the
/// protocol-wide `<Proto>Error` union. `throws` is `(ordinal, error name)` in
/// the order the variants should appear.
///
/// With nothing to put in it this emits a **type alias to the never type**
/// rather than `pub enum Foo {}`. It names the same thing — a function that
/// declares no `!` cannot raise anything, and the wire has no ordinal for a
/// failure the schema never described — but it names it the way every Rust
/// reader already knows, and `Infallible` brings `Display`, `Copy`, `Eq` and
/// `Hash`, where an empty enum brings nothing at all. The name survives, so
/// hand-written code and docs that spell it keep working, and the
/// dispatcher's `match never {}` arm stays valid because `Infallible` is
/// uninhabited too.
///
/// A `Display` impl only makes sense on the real enum: the alias is a foreign
/// type, so implementing a trait for it here would not even compile.
fn error_type(name: &str, throws: &[(u16, String)]) -> String {
    if throws.is_empty() {
        return format!("pub type {name} = ::core::convert::Infallible;\n\n");
    }

    let mut s = format!("#[derive(Debug, Clone, PartialEq)]\npub enum {name} {{\n");
    for (_, err) in throws {
        s.push_str(&format!("    {err}({err}),\n"));
    }
    s.push_str("}\n\n");

    s.push_str(&format!(
        "impl core::fmt::Display for {name} {{\n    \
             fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {{\n        \
                 match self {{\n"
    ));
    for (_, err) in throws {
        s.push_str(&format!("            {name}::{err}(e) => e.fmt(f),\n"));
    }
    s.push_str("        }\n    }\n}\n\n");
    s
}

// ── protocol ───────────────────────────────────────────────────────────────

struct Arg {
    name: String,
    /// The type in the provider trait / client signatures (`&str`, `u32`, `Msg`).
    sig_ty: String,
    /// The type in the params struct — `&'a str` where `sig_ty` is `&str`,
    /// otherwise identical. The params struct decodes borrowed from the wire.
    field_ty: String,
}

impl Arg {
    fn borrows(&self) -> bool {
        self.field_ty.contains("'a")
    }
}

struct FnInfo {
    name: String,
    params_ty: Option<String>,
    /// Whether the params struct needs a `<'a>` (any `&str` arg).
    params_borrows: bool,
    args: Vec<Arg>,
    ret: String,
    err_ty: String,
    /// `(ordinal, error struct type)` for each `!` on this function.
    throws: Vec<(u16, String)>,
    /// `_return: None` — fire-and-forget: `Client::notify`, the dispatcher
    /// writes no envelope, the trait method returns `()` with no error. An
    /// explicit `-> ()` (`KindValue::Unit`) is *not* this — it's a normal
    /// request/response with an empty ack (§4.4).
    one_way: bool,
    /// `@timeout_ms = N` on the function — the client method calls
    /// `Client::call_with_timeout` with `Duration::from_millis(N)`.
    timeout_ms: Option<u64>,
}

/// The `Framing` a protocol's generated `connect` / `serve` helpers wire up.
struct FramingChoice {
    /// Path to the framing type, e.g. `comline_runtime::framing::JsonRpcFraming`.
    ty: &'static str,
    /// `true` for anything but the datagram default — those helpers pass the
    /// framing explicitly (`Client::connect_with_framing` / `Server::with_framing`)
    /// and the `Client` alias carries it as its third type parameter.
    explicit: bool,
}

/// The datagram default — the generated helpers use `Client::connect` /
/// `Server::new` and the `Client` alias keeps its two type parameters.
const DATAGRAM_FRAMING: FramingChoice = FramingChoice {
    ty: "comline_runtime::contract::DatagramFraming",
    explicit: false,
};

/// Map one framing name to a choice. `None` for a name core does not
/// recognise (the caller falls through to the next level).
///
/// The accepted spellings and their aliases come from core, not from a
/// copy kept here - this generator, the TypeScript one and the playground
/// each used to maintain their own, and they had drifted (the
/// playground's was missing `json-rpc`, so a protocol this generator emits
/// as JSON-RPC rendered as datagram there).
fn framing_for(name: &str) -> Option<FramingChoice> {
    match comline_core::schema::idl::annotations::canonical_value("framing", name)? {
        "jsonrpc" => Some(FramingChoice {
            ty: "comline_runtime::framing::JsonRpcFraming",
            explicit: true,
        }),
        "datagram" => Some(DATAGRAM_FRAMING),
        _ => None,
    }
}

/// Resolve a protocol's framing, most specific first: its own `@framing`
/// annotation, then the package `default_framing` (`comline.toml`), then the
/// datagram default. An unrecognised name at either level falls through.
fn framing_choice(parameters: &[FrozenUnit], default_framing: Option<&str>) -> FramingChoice {
    annotation(parameters, "framing")
        .and_then(|name| framing_for(&name))
        .or_else(|| default_framing.and_then(framing_for))
        .unwrap_or(DATAGRAM_FRAMING)
}

fn protocol(
    proto: &str,
    functions: &[FrozenUnit],
    parameters: &[FrozenUnit],
    default_framing: Option<&str>,
    errors: &HashMap<u16, String>,
    imports: &HashMap<String, String>,
) -> String {
    let framing = framing_choice(parameters, default_framing);
    let fns: Vec<FnInfo> = functions
        .iter()
        .filter_map(|f| match f {
            FrozenUnit::Function {
                name,
                parameters,
                arguments,
                _return,
                throws,
                ..
            } => Some(fn_info(
                proto, name, parameters, arguments, _return, throws, errors, imports,
            )),
            _ => None,
        })
        .collect();

    let mut s = String::new();

    // 1. params struct per function that takes arguments
    for f in &fns {
        if let Some(ty) = &f.params_ty {
            let lt = if f.params_borrows { "<'a>" } else { "" };
            s.push_str(&format!(
                "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct {ty}{lt} {{\n"
            ));
            for arg in &f.args {
                if arg.borrows() {
                    s.push_str("    #[serde(borrow)]\n");
                }
                s.push_str(&format!("    pub {}: {},\n", arg.name, arg.field_ty));
            }
            s.push_str("}\n\n");
        }
    }

    // 2. one schema-only error type per request/response function. One-way
    //    functions have no error channel at all.
    for f in &fns {
        if f.one_way {
            continue;
        }
        s.push_str(&error_type(&f.err_ty, &f.throws));
    }

    // 2b. per-protocol union of every `!`, additive, for one broad handler
    let mut union: Vec<(u16, String)> = fns.iter().flat_map(|f| f.throws.clone()).collect();
    union.sort_by_key(|(ord, _)| *ord);
    union.dedup_by_key(|(ord, _)| *ord);
    let union_ty = format!("{proto}Error");
    s.push_str(&error_type(&union_ty, &union));
    for f in &fns {
        if f.throws.is_empty() {
            continue;
        }
        s.push_str(&format!(
            "impl From<{0}> for {union_ty} {{\n    fn from(e: {0}) -> Self {{\n        match e {{\n",
            f.err_ty
        ));
        for (_, err) in &f.throws {
            s.push_str(&format!(
                "            {}::{err}(x) => {union_ty}::{err}(x),\n",
                f.err_ty
            ));
        }
        s.push_str("        }\n    }\n}\n\n");
    }

    // 3. provider trait + the call table
    s.push_str(&format!("pub trait {proto} {{\n"));
    for f in &fns {
        let args = sig_args(&f.args);
        if f.one_way {
            s.push_str(&format!("    fn {}(&self{args});\n", f.name));
        } else {
            // `Fault<E>` rather than a bare `E`: an implementation can fail
            // in ways its schema never described, and before this it could
            // only panic. `E` may be uninhabited while `Fault<E>` is not.
            s.push_str(&format!(
                "    fn {}(&self{args}) -> Result<{}, Fault<{}>>;\n",
                f.name, f.ret, f.err_ty
            ));
        }
    }
    s.push_str("}\n\n");

    let calls_const = format!("{}_CALLS", screaming_snake(proto));
    s.push_str(&format!(
        "pub const {calls_const}: &[&str] = &[{}];\n\n",
        fns.iter()
            .map(|f| format!("\"{}\"", f.name))
            .collect::<Vec<_>>()
            .join(", ")
    ));

    // 4. dispatcher — implements the runtime `Dispatch` trait
    let mut l: Vec<String> = Vec::new();
    l.push(format!("pub struct {proto}Dispatcher<S>(pub S);"));
    l.push(String::new());
    // `reply` is only touched by request/response arms; a protocol that is
    // entirely one-way never records anything.
    let reply_param = if fns.iter().all(|f| f.one_way) {
        "_reply"
    } else {
        "reply"
    };
    l.push(format!(
        "impl<S: {proto}> Dispatch for {proto}Dispatcher<S> {{"
    ));
    l.push("    fn calls(&self) -> &'static [&'static str] {".into());
    l.push(format!("        {calls_const}"));
    l.push("    }".into());
    l.push(String::new());
    l.push("    fn dispatch<W: WireFormat>(".into());
    l.push("        &self,".into());
    l.push("        call: Kind,".into());
    l.push("        params: &[u8],".into());
    l.push("        fmt: &W,".into());
    l.push(format!("        {reply_param}: &mut Reply,"));
    l.push("    ) -> Result<(), RuntimeError> {".into());
    l.push(format!(
        "        match call.resolve({calls_const}).ok_or(RuntimeError::UnknownCall)? {{"
    ));
    for (i, f) in fns.iter().enumerate() {
        l.push(format!("            {i} => {{"));
        // Params that will not decode are this call's failure, not the
        // connection's. A request/response call answers with a fault; a
        // one-way call has no response frame, so there is nowhere to say so
        // and it stops here. Doing this in the dispatcher rather than
        // leaving it to `Server` is what makes the call's arity count: the
        // server cannot see it.
        let on_undecodable: Vec<String> = if f.one_way {
            vec!["                    Err(_) => return Ok(()),".into()]
        } else {
            vec![
                "                    Err(_) => {".into(),
                "                        reply.fault();".into(),
                "                        return Ok(());".into(),
                "                    }".into(),
            ]
        };
        let call_args = match &f.params_ty {
            Some(ty) => {
                l.push(format!(
                    "                let p: {ty} = match fmt.decode(params) {{"
                ));
                l.push("                    Ok(p) => p,".into());
                l.extend(on_undecodable);
                l.push("                };".into());
                f.args
                    .iter()
                    .map(|a| format!("p.{}", a.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
            None => {
                l.push("                let _: () = match fmt.decode(params) {".into());
                l.push("                    Ok(unit) => unit,".into());
                l.extend(on_undecodable);
                l.push("                };".into());
                String::new()
            }
        };
        let call = format!("self.0.{}({call_args})", f.name);
        if f.one_way {
            // Run the handler; record nothing — the `Server` sees `Outcome::None`
            // and replies with nothing.
            l.push(format!("                {call};"));
        } else {
            l.push(format!("                match {call} {{"));
            l.push("                    Ok(value) => {".into());
            l.push("                        let mut body = Vec::new();".into());
            l.push("                        fmt.encode(&value, &mut body)?;".into());
            l.push("                        reply.ok(&body);".into());
            l.push("                    }".into());
            for (ord, err) in &f.throws {
                l.push(format!(
                    "                    Err(Fault::App({}::{err}(e))) => {{",
                    f.err_ty
                ));
                l.push("                        let mut body = Vec::new();".into());
                l.push("                        fmt.encode(&e, &mut body)?;".into());
                l.push(format!(
                    "                        reply.err({ord}u16, &body);"
                ));
                l.push("                    }".into());
            }
            if f.throws.is_empty() {
                // The error type is `Infallible`, so this arm is unreachable -
                // it is what keeps the `match` exhaustive.
                l.push("                    Err(Fault::App(never)) => match never {},".into());
            }
            l.push("                    Err(Fault::Internal) => reply.fault(),".into());
            l.push("                }".into());
        }
        l.push("                Ok(())".into());
        l.push("            }".into());
    }
    l.push("            _ => Err(RuntimeError::UnknownCall),".into());
    l.push("        }".into());
    l.push("    }".into());
    l.push("}".into());
    l.push(String::new());
    l.push(format!("impl<S: {proto}> {proto}Dispatcher<S> {{"));
    l.push("    /// Serve this protocol over `transport`, running the connection".into());
    l.push("    /// handshake (`IR_HASH` + `format`'s name) against the peer first.".into());
    l.push(
        "    pub fn serve<T: Transport, W: WireFormat>(self, transport: &mut T, format: W)".into(),
    );
    l.push("    -> Result<(), RuntimeError> {".into());
    if framing.explicit {
        l.push(format!("        let framing = {};", framing.ty));
        l.push(
            "        let hs = Handshake::new(IR_HASH, format.name(), framing.name(), 0);".into(),
        );
        l.push(
            "        Server::with_framing(self, format, framing).serve_handshaked(transport, hs)"
                .into(),
        );
    } else {
        l.push(
            "        let hs = Handshake::new(IR_HASH, format.name(), FRAMING_DATAGRAM, 0);".into(),
        );
        l.push("        Server::new(self, format).serve_handshaked(transport, hs)".into());
    }
    l.push("    }".into());
    l.push("}".into());
    l.push(String::new());
    l.push(String::new());
    s.push_str(&l.join("\n"));

    // 5. consumer stub — wraps a `Client`
    let client_ty = if framing.explicit {
        format!("Client<T, W, {}>", framing.ty)
    } else {
        "Client<T, W>".to_string()
    };
    let mut l: Vec<String> = Vec::new();
    l.push(format!("pub struct {proto}Client<T, W>(pub {client_ty});"));
    l.push(String::new());
    l.push(format!(
        "impl<T: Transport, W: WireFormat> {proto}Client<T, W> {{"
    ));
    l.push(format!("    pub fn new(client: {client_ty}) -> Self {{"));
    l.push("        Self(client)".into());
    l.push("    }".into());
    l.push(String::new());
    l.push("    /// Bind + run the connection handshake against the provider.".into());
    l.push("    pub fn connect(transport: T, format: W) -> Result<Self, RuntimeError> {".into());
    if framing.explicit {
        l.push(format!("        let framing = {};", framing.ty));
        l.push(
            "        let hs = Handshake::new(IR_HASH, format.name(), framing.name(), 0);".into(),
        );
        l.push(
            "        Ok(Self(Client::connect_with_framing(transport, format, framing, hs)?))"
                .into(),
        );
    } else {
        l.push(
            "        let hs = Handshake::new(IR_HASH, format.name(), FRAMING_DATAGRAM, 0);".into(),
        );
        l.push("        Ok(Self(Client::connect(transport, format, hs)?))".into());
    }
    l.push("    }".into());
    for (i, f) in fns.iter().enumerate() {
        let args = sig_args(&f.args);
        let param_expr = match &f.params_ty {
            Some(ty) => {
                let init = f
                    .args
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("&{ty} {{ {init} }}")
            }
            None => "&()".to_string(),
        };
        // Both addresses — the framing picks (ordinal for datagram, name for
        // a name-oriented framing like JSON-RPC).
        let addr = format!("Call::new({i}, \"{}\")", f.name);
        l.push(String::new());
        if f.one_way {
            // Fire-and-forget: no reply, so no `CallError<E>`.
            l.push(format!(
                "    pub fn {}(&mut self{args}) -> Result<(), RuntimeError> {{",
                f.name
            ));
            l.push(format!("        self.0.notify({addr}, {param_expr})"));
            l.push("    }".into());
            continue;
        }
        l.push(format!(
            "    pub fn {}(&mut self{args}) -> Result<{}, CallError<{}>> {{",
            f.name, f.ret, f.err_ty
        ));
        let call = match f.timeout_ms {
            Some(ms) => format!(
                "self.0.call_with_timeout({addr}, {param_expr}, core::time::Duration::from_millis({ms}))"
            ),
            None => format!("self.0.call({addr}, {param_expr})"),
        };
        l.push(format!("        let (reply, fmt) = {call}?;"));
        l.push("        match reply {".into());
        l.push(
            "            Envelope::Ok(payload) => fmt.decode(payload).map_err(CallError::Runtime),"
                .into(),
        );
        for (ord, err) in &f.throws {
            l.push(format!(
                "            Envelope::Err {{ id: {ord}u16, body }} => {{"
            ));
            l.push(format!("                let e: {err} = fmt.decode(body)?;"));
            l.push(format!(
                "                Err(CallError::App({}::{err}(e)))",
                f.err_ty
            ));
            l.push("            }".into());
        }
        l.push(
            "            Envelope::Err { id, .. } => Err(CallError::Runtime(RuntimeError::Remote { id })),"
                .into(),
        );
        l.push(
            "            Envelope::Fault => Err(CallError::Runtime(RuntimeError::Provider)),"
                .into(),
        );
        l.push("        }".into());
        l.push("    }".into());
    }
    l.push("}".into());
    l.push(String::new());
    l.push(String::new());
    s.push_str(&l.join("\n"));

    s
}

// `errors` and `imports` are two unrelated lookup tables (ordinal → error
// type name; bare name → origin namespace) - bundling them into one
// context struct just to dodge this lint would force every caller of
// `rust_type`/`arg_types` to carry an `errors` table they never use.
#[allow(clippy::too_many_arguments)]
fn fn_info(
    proto: &str,
    name: &str,
    parameters: &[FrozenUnit],
    arguments: &[FrozenArgument],
    ret: &Option<KindValue>,
    throws: &[u16],
    errors: &HashMap<u16, String>,
    imports: &HashMap<String, String>,
) -> FnInfo {
    let pascal_fn = pascal(name);
    let timeout_ms = annotation(parameters, "timeout_ms").and_then(|v| v.parse::<u64>().ok());
    let args: Vec<Arg> = arguments
        .iter()
        .map(|a| {
            let (sig_ty, field_ty) = arg_types(&a.kind, imports);
            Arg {
                name: a.name.clone(),
                sig_ty,
                field_ty,
            }
        })
        .collect();
    let params_borrows = args.iter().any(Arg::borrows);
    let params_ty = if args.is_empty() {
        None
    } else {
        Some(format!("{proto}{pascal_fn}Params"))
    };
    let one_way = ret.is_none();
    let ret = match ret {
        None | Some(KindValue::Unit) => "()".to_string(),
        Some(kv) => rust_type(kv, imports),
    };
    // A one-way function carries no `!` (nowhere to deliver it); ignore any.
    let throws: &[u16] = if one_way { &[] } else { throws };
    let throws: Vec<(u16, String)> = throws
        .iter()
        .map(|ord| {
            (
                *ord,
                errors
                    .get(ord)
                    .cloned()
                    .unwrap_or_else(|| format!("UnknownError{ord}")),
            )
        })
        .collect();
    FnInfo {
        name: name.to_string(),
        params_ty,
        params_borrows,
        args,
        ret,
        err_ty: format!("{proto}{pascal_fn}Error"),
        throws,
        one_way,
        timeout_ms,
    }
}

/// The text of a scalar `@key = value` annotation (frozen as
/// `FrozenUnit::Property`), by key.
///
/// The frozen value is typed now, so this renders it back to text for the
/// callers that only interpolate it. A caller that cares *which kind* of
/// value it got should match `ConfigValue` instead.
fn annotation<'a>(parameters: &'a [FrozenUnit], key: &str) -> Option<Cow<'a, str>> {
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value } if name == key => value.as_text(),
        _ => None,
    })
}

/// `(signature type, params-struct field type)` for a function argument.
/// A `str` / `string` arg is passed by reference and decoded borrowed from
/// the receive buffer; everything else is owned (borrowed arrays / nested
/// borrowed structs are a follow-up).
fn arg_types(kind: &KindValue, imports: &HashMap<String, String>) -> (String, String) {
    let owned = rust_type(kind, imports);
    match owned.as_str() {
        "String" => ("&str".to_string(), "&'a str".to_string()),
        _ => (owned.clone(), owned),
    }
}

// ── name / type helpers ────────────────────────────────────────────────────

/// The `, name: ty, name: ty` suffix for a method taking `args` after `&self`
/// / `&mut self` — empty when there are none.
fn sig_args(args: &[Arg]) -> String {
    if args.is_empty() {
        return String::new();
    }
    let list = args
        .iter()
        .map(|a| format!("{}: {}", a.name, a.sig_ty))
        .collect::<Vec<_>>()
        .join(", ");
    format!(", {list}")
}

/// Bare name as *this* schema binds it → the full path to write for it,
/// already including `crate::` or `comline_std::` as appropriate.
///
/// [`SymbolTable`] does the resolving; this only renders. The split matters
/// because rendering is where the two targets genuinely part ways:
/// TypeScript writes the bare name plus an `import type` header, Rust writes
/// the qualified path inline and has no import block at all.
///
/// The map's value is the **whole path, ending in the name the other schema
/// declares** - not a prefix to append the bound name to. The two differ
/// exactly when there is an alias: `use shared::Shared as Aliased` binds
/// `Aliased` here while `shared` still exports only `Shared`, so the path to
/// write is `crate::shared::Shared`. Appending the bound name instead
/// produced `crate::shared::Aliased`, which names nothing
/// (`error[E0425]: cannot find type \`Aliased\` in module \`crate::shared\``).
/// That is what [`Origin::declared`] exists to keep separate.
fn import_paths(
    units: &[FrozenUnit],
    schemas: &[(String, Vec<FrozenUnit>)],
    external_std: bool,
) -> HashMap<String, String> {
    // Every namespace an import names is nameable here, whether or not this
    // request carries its schema. Unlike TypeScript - which points at a file
    // it wrote, and so must refuse to name a schema it didn't - Rust writes
    // a module path into a crate it controls completely: an origin either
    // lands in this crate's own module tree (everything is inlined by
    // default) or in `comline_std`. There is no third case to narrow to.
    let table = SymbolTable::with_external(units, schemas, |_| true);

    let mut map = HashMap::new();
    for unit in units {
        let FrozenUnit::Import(path, alias, _) = unit else {
            continue;
        };
        if path.ends_with("::*") {
            continue; // the glob marker itself binds no name
        }
        let Some((_, declared)) = path.rsplit_once("::") else {
            continue; // a bare single-segment path binds no name here
        };
        let bound = alias.clone().unwrap_or_else(|| declared.to_string());
        if let Some(origin) = table.origin(&bound) {
            map.insert(bound, rust_path(origin, external_std));
        }
    }
    map
}

/// Where an [`Origin`] is written from, in Rust's own spelling.
///
/// Every non-local schema (a declared dependency or `std`) is inlined into
/// the same generated crate's module tree as the consumer's own schemas by
/// default, so a resolved reference is normally
/// `crate::<namespace>::<name>` regardless of origin. The one exception is
/// `std` specifically when `external_std` is on, where it points outside the
/// crate at `comline_std::<rest>::<name>` instead - `rest` being the
/// namespace with its leading `std` segment stripped, since `comline_std` is
/// its own crate root rather than something nested under `std`.
fn rust_path(origin: &Origin, external_std: bool) -> String {
    let Origin {
        namespace,
        declared,
    } = origin;
    if external_std && is_std_namespace(namespace) {
        let prefix = match namespace.strip_prefix("std/") {
            Some(rest) => format!("comline_std::{}", rest.replace('/', "::")),
            None => "comline_std".to_string(),
        };
        return format!("{prefix}::{declared}");
    }
    format!("crate::{}::{declared}", namespace.replace('/', "::"))
}

fn rust_type(kind: &KindValue, imports: &HashMap<String, String>) -> String {
    match kind {
        KindValue::Primitive(p) => map_str_type(p.name(), imports),
        KindValue::Namespaced(name, _) => map_str_type(name, imports),
        KindValue::EnumVariant(name, _) => name.clone(),
        KindValue::Unit => "()".to_string(),
        _ => "/* unknown_kind */".to_string(),
    }
}

fn map_str_type(s: &str, imports: &HashMap<String, String>) -> String {
    if let Some(inner) = s.strip_suffix("[]") {
        return format!("Vec<{}>", map_str_type(inner, imports));
    }
    match s {
        "string" | "str" => "String".to_string(),
        "bool" => "bool".to_string(),
        "float" => "f64".to_string(),
        "int" => "i32".to_string(),
        "u8" | "u16" | "u32" | "u64" | "u128" => s.to_string(),
        // IDL spells signed integers with an `s` prefix (s8/s16/s32/s64);
        // Rust's own primitives are always `i`-prefixed, so this is a real
        // translation, not a passthrough.
        "s8" => "i8".to_string(),
        "s16" => "i16".to_string(),
        "s32" => "i32".to_string(),
        "s64" => "i64".to_string(),
        "s128" => "i128".to_string(),
        // The map already holds the full path, which is what makes an
        // aliased import resolve to the name the other schema really
        // exports rather than to this schema's name for it.
        other => match imports.get(other) {
            Some(path) => path.clone(),
            None => other.to_string(),
        },
    }
}

// `import_paths` is private, so unlike the rest of this generator's
// behavior (covered by `tests/generate.rs`, alongside the shared
// `generation/conformance` corpus) it can only be tested from inside this
// file.
#[cfg(test)]
mod tests {
    use super::*;

    fn import(path: &str, alias: Option<&str>) -> FrozenUnit {
        FrozenUnit::Import(path.to_string(), alias.map(str::to_string), (0, 0))
    }

    /// The schema list is empty on purpose: it does not restrict what Rust
    /// can name, because every origin is either inlined into this crate or
    /// in `comline_std`. What each case below is really about is the *path*
    /// rendered for a resolved name.
    fn paths(units: &[FrozenUnit], external_std: bool) -> HashMap<String, String> {
        import_paths(units, &[], external_std)
    }

    #[test]
    fn a_plain_import_resolves_to_a_crate_qualified_prefix() {
        let units = vec![import("shared::http::Request", None)];
        let map = paths(&units, false);
        assert_eq!(
            map.get("Request"),
            Some(&"crate::shared::http::Request".to_string())
        );
    }

    #[test]
    fn an_aliased_import_resolves_to_the_name_the_other_schema_exports() {
        // `UUID` is this schema's name for it; `external::uuid` exports
        // `Uuid`. The path has to use the exported name - writing
        // `crate::external::uuid::UUID` names nothing.
        let units = vec![import("external::uuid::Uuid", Some("UUID"))];
        let map = paths(&units, false);
        assert_eq!(
            map.get("UUID"),
            Some(&"crate::external::uuid::Uuid".to_string())
        );
        assert_eq!(map.get("Uuid"), None, "the alias is what this schema binds");
    }

    #[test]
    fn item_list_imports_are_already_one_leaf_entry_each() {
        // Core expands `use ns::{A, B};` into two separate `Import`
        // units before this ever reaches the generator.
        let units = vec![import("shared::A", None), import("shared::B", None)];
        let map = paths(&units, false);
        assert_eq!(map.get("A"), Some(&"crate::shared::A".to_string()));
        assert_eq!(map.get("B"), Some(&"crate::shared::B".to_string()));
    }

    #[test]
    fn a_glob_expanded_leaf_resolves_like_any_other_import() {
        // Core already expands `use ns::*;` into one leaf `Import` per
        // symbol the target schema declares, alongside the glob marker.
        let units = vec![
            import("shared::http::*", None),
            import("shared::http::Request", None),
        ];
        let map = paths(&units, false);
        assert_eq!(
            map.get("Request"),
            Some(&"crate::shared::http::Request".to_string())
        );
    }

    #[test]
    fn the_glob_marker_itself_binds_no_name() {
        let units = vec![import("shared::http::*", None)];
        let map = paths(&units, false);
        assert!(map.is_empty());
    }

    #[test]
    fn std_stays_crate_qualified_when_external_std_is_off() {
        let units = vec![import("std::http::Request", None)];
        let map = paths(&units, false);
        assert_eq!(
            map.get("Request"),
            Some(&"crate::std::http::Request".to_string())
        );
    }

    #[test]
    fn std_points_outside_the_crate_when_external_std_is_on() {
        let units = vec![import("std::http::Request", None)];
        let map = paths(&units, true);
        assert_eq!(
            map.get("Request"),
            Some(&"comline_std::http::Request".to_string())
        );
    }

    #[test]
    fn a_dependency_stays_crate_qualified_even_when_external_std_is_on() {
        // external_std only redirects `std` specifically - everything else
        // keeps inlining, same as today.
        let units = vec![import("shared::Thing", None)];
        let map = paths(&units, true);
        assert_eq!(map.get("Thing"), Some(&"crate::shared::Thing".to_string()));
    }

    #[test]
    fn a_name_directly_under_std_itself_has_no_trailing_separator() {
        // `use std::Foo;` (one segment deep, not `std::http::Foo`) -
        // `comline_std`'s own crate root, not `comline_std::`.
        let units = vec![import("std::Foo", None)];
        let map = paths(&units, true);
        assert_eq!(map.get("Foo"), Some(&"comline_std::Foo".to_string()));
    }
}
