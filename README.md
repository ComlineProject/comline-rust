# comline-rust

The **Rust target** for [Comline](https://github.com/ComlineProject) — one repo
per language, holding that language's codegen, libgen, runtime and std-extra
together.

Today: just `codegen/` (`comline-codegen-rust`), extracted from
`ComlineProject/generation`. The FFI / `dylib` (`lib`-mode) work is gated
pending a rewrite against the current IR (G2c); a runtime and std-extra follow.

## `codegen/`

`comline-codegen-rust` — frozen IR → Rust source.

- `code` — one `<namespace>.rs` per schema: serde data types, plain traits,
  C-like enums.
- `lib` — a buildable crate: `Cargo.toml` + `src/lib.rs` (module tree) +
  `src/<namespace>.rs`.

Depends on `comline-codegen` (the language-neutral contract + `Registry`) and
`comline-core` (the IR), both by git rev. `register(&mut Registry)` contributes
the generator under `rust` at version `1.70.0`; the Comline CLI composes it into
its `Registry` at startup.

```sh
cargo test
```

The `rust_c_ffi/`, `rust_abi_stable/`, `lib_gen_rust*/` modules are kept for
reference but not built — they target a pre-audit `FrozenUnit` and need a
rewrite, not a port.

## Design

See `ComlineProject/docs` → Design:

- *Runtime & generation repository structure* — why one repo per language
- *Generation* — what codegen / libgen / runtime each mean
- *The `core` ↔ target contract* — the boundary this repo builds against
