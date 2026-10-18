//! Rust code generator.
//!
//! - `code` — one `<namespace>.rs` per schema: serde data types, plain traits,
//!   C-like enums (ported from `comline-core`'s `codelib_gen::rust` in de-rot G1).
//! - `lib` — a buildable crate: `Cargo.toml` + `src/lib.rs` + `src/<namespace>.rs`.
//!
//! FFI / `dylib` (G2c) targets a pre-audit `FrozenUnit` and needs a rewrite, not
//! a port — the modules are kept for reference but not built:
//!   `rust_c_ffi/`, `rust_abi_stable/`, `lib_gen_rust/`, `lib_gen_rust_c_ffi/`.
//! See `design/generation.md`.

mod generator;
mod message;
mod validate;

use comline_codegen::features::{Feature, Support, SupportTable};

pub use generator::{generate_rust, STD_REV};

/// The toolchain version this generator registers under - what a
/// congregation names as `rust#1.70.0`. Public so `xtask`, which generates
/// `comline_std` through the very same entry point every consumer uses, can
/// state it rather than repeat the literal.
pub const LANG_VERSION: &str = "1.70.0";

/// What this generator implements, feature by feature. Each claim is proven
/// by `comline-conformance`, not taken on trust: see `comline_codegen::features`.
pub const SUPPORT: SupportTable = &[
    (Feature::Structs, Support::supported()),
    (Feature::Enums, Support::supported()),
    (Feature::OptionalFields, Support::supported()),
    (Feature::Arrays, Support::supported()),
    (Feature::FixedArrays, Support::supported()),
    (
        Feature::LargeFixedArraysNested,
        Support::unsupported(
            "a fixed array over 32 elements renders only as a struct or error field's own, \
             non-optional type (serde's derive stops at 32)",
        ),
    ),
    (Feature::Unions, Support::unsupported("Rust has no anonymous sum type to render one as")),
    (Feature::TypeAliases, Support::supported()),
    (Feature::Constants, Support::supported()),
    (Feature::Protocols, Support::supported()),
    (Feature::Errors, Support::supported()),
    (Feature::Throws, Support::supported()),
    (Feature::Validators, Support::supported()),
    (Feature::Imports, Support::supported()),
    (Feature::NestedNamespaces, Support::supported()),
    (Feature::LibMode, Support::supported()),
    (Feature::ExternalStd, Support::supported()),
    (Feature::ValidateOnDecode, Support::supported()),
    (Feature::FramingJsonRpc, Support::supported()),
];

/// Contribute the Rust generator to a CLI's [`Registry`](comline_codegen::Registry).
pub fn register(registry: &mut comline_codegen::Registry) {
    registry.register("rust", "rs", LANG_VERSION, generate_rust, SUPPORT);
}
