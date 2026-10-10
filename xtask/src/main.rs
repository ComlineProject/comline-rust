//! Regenerates `std-extra/` from Comline's real `std` schemas, by running
//! the same `generate_rust` every consumer's `comline generate -m lib` call
//! goes through - so `comline_std` is never hand-edited, only regenerated.
//!
//! `cargo run -p xtask` writes it; `cargo run -p xtask -- --check` diffs
//! instead and exits non-zero on drift (the CI guard).

use std::fs;
use std::path::PathBuf;

use comline_codegen::{GenRequest, Mode, PackageMeta};
use comline_codegen_rust::generate_rust;
use comline_core::package::build::PackageSources;
use comline_core::package::stdlib;
use comline_core::schema::ir::frozen::unit::FrozenUnit;

fn main() -> eyre::Result<()> {
    let check = std::env::args().any(|a| a == "--check");

    // std's own real manifest (`congregation std`) - compiling it standalone
    // means every schema's own namespace comes out prefixed with that
    // congregation name (`["std", "http"]`), same as any package's own
    // schemas would be under its own name. Strip that leading segment:
    // `comline_std`'s module tree should be flat (`http`, `validators`), so
    // a consumer references `comline_std::http::Request`, not
    // `comline_std::std::http::Request`.
    let mut sources = PackageSources::new().config(stdlib::manifest());
    for (namespace, source) in stdlib::schemas() {
        sources = sources.schema(namespace, source);
    }
    let context = sources.compile()?;

    let schemas: Vec<(String, Vec<FrozenUnit>)> = context
        .schema_contexts
        .iter()
        .filter_map(|sc| {
            let sc = sc.borrow();
            let units = sc.frozen_schema.borrow().clone()?;
            let rest = match sc.namespace.split_first() {
                Some((first, rest)) if first == "std" => rest,
                _ => &sc.namespace[..],
            };
            Some((rest.join("/"), units))
        })
        .collect();

    let req = GenRequest {
        mode: Mode::Lib,
        schemas: &schemas,
        package: PackageMeta { name: "comline_std".to_string(), version: "0.1.0".to_string() },
        default_framing: None,
        // comline_std is generated inline, as an ordinary standalone
        // crate - external_std only matters to a *consumer* referencing
        // std, not to std's own generation.
        external_std: false,
    };

    let files = generate_rust(&req)?;

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../std-extra");
    let mut drift = false;

    for file in &files {
        let path = out_dir.join(&file.path);
        if check {
            let existing = fs::read_to_string(&path).unwrap_or_default();
            if existing != file.contents {
                eprintln!("drift: {}", path.display());
                drift = true;
            }
        } else {
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(&path, &file.contents)?;
            println!("wrote {}", path.display());
        }
    }

    if check && drift {
        eprintln!("std-extra/ is out of date - run `cargo run -p xtask` to regenerate it");
        std::process::exit(1);
    }

    Ok(())
}
