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

pub use generator::generate_rust;

/// Contribute the Rust generator to a CLI's [`Registry`](comline_codegen::Registry).
pub fn register(registry: &mut comline_codegen::Registry) {
    registry.register("rust", "rs", "1.70.0", generate_rust);
}
