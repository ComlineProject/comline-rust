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

## Features

What this generator implements, feature by feature. Each claim is checked by
the conformance suite in [`ComlineProject/generation`](https://github.com/ComlineProject/generation):
a supported feature has a fixture that generates and matches its golden output,
an ignored one generates exactly what it would without the feature, and an
unsupported one is refused with an error naming every use. The full matrix,
with the fixtures that prove each claim, is in
[`FEATURES.md`](https://github.com/ComlineProject/generation/blob/master/FEATURES.md).

<!-- features:start -->
<!-- Generated from `SUPPORT` in `codegen/src/lib.rs` - don't edit by hand. -->

| Feature | rust 1.70.0 |
|---|---|
| structs <br><sub>`structs`</sub> | ✅ supported |
| enums <br><sub>`enums`</sub> | ✅ supported |
| optional fields <br><sub>`optional-fields`</sub> | ✅ supported |
| arrays <br><sub>`arrays`</sub> | ✅ supported |
| fixed-length arrays <br><sub>`fixed-arrays`</sub> | ✅ supported |
| fixed arrays over 32 elements, nested <br><sub>`large-fixed-arrays-nested`</sub> | ❌ unsupported: a fixed array over 32 elements renders only as a struct or error field's own, non-optional type (serde's derive stops at 32) |
| unions <br><sub>`unions`</sub> | ❌ unsupported: Rust has no anonymous sum type to render one as |
| type aliases <br><sub>`type-aliases`</sub> | ✅ supported |
| constants <br><sub>`constants`</sub> | ✅ supported |
| protocols <br><sub>`protocols`</sub> | ✅ supported |
| errors <br><sub>`errors`</sub> | ✅ supported |
| functions that throw <br><sub>`throws`</sub> | ✅ supported |
| validators <br><sub>`validators`</sub> | ✅ supported |
| imports between schemas <br><sub>`imports`</sub> | ✅ supported |
| nested namespaces <br><sub>`nested-namespaces`</sub> | ✅ supported |
| `lib` mode <br><sub>`lib-mode`</sub> | ✅ supported |
| `external_std` <br><sub>`external-std`</sub> | ✅ supported |
| `validate_on_decode` <br><sub>`validate-on-decode`</sub> | ✅ supported |
| JSON-RPC framing <br><sub>`framing-jsonrpc`</sub> | ✅ supported |
| no handshake <br><sub>`handshake-none`</sub> | ✅ supported |
| connections <br><sub>`connections`</sub> | 🟡 partial: over `datagram@2`; JSON-RPC on a connection isn't supported yet (runtime#19) |

<!-- features:end -->

## Design

See `ComlineProject/docs` → Design:

- *Runtime & generation repository structure* — why one repo per language
- *Generation* — what codegen / libgen / runtime each mean
- *The `core` ↔ target contract* — the boundary this repo builds against


## License

**GNU General Public License v3.0 only** ([LICENSE](LICENSE) or
<https://www.gnu.org/licenses/gpl-3.0.html>). This generator links
`comline-core` and is part of Comline's GPL toolchain. Copyleft covers the
generator, **not** the code it emits — generated bindings are yours and link
only `comline-runtime` (MPL-2.0). Contributions are GPL-3.0-only. Rationale +
per-repo split: [`design/licensing.md`](https://github.com/ComlineProject/docs).
