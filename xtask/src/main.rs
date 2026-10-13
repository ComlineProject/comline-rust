//! Regenerates `std-extra/` from Comline's real `std` schemas, by running
//! the same `generate_rust` every consumer's `comline generate -m lib` call
//! goes through - so `comline_std` is never hand-edited, only regenerated.
//!
//! `cargo run -p xtask` writes it; `cargo run -p xtask -- --check` diffs
//! instead and exits non-zero on drift (the CI guard).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use comline_codegen::{GenRequest, Mode, PackageMeta};
use comline_codegen_rust::{generate_rust, STD_REV};
use comline_core::package::build::PackageSources;
use comline_core::package::stdlib;
use comline_core::schema::ir::frozen::unit::FrozenUnit;

fn main() -> eyre::Result<()> {
    let check = std::env::args().any(|a| a == "--check");
    // Deliberately its own flag, not part of `--check`. Regenerating
    // `std-extra/` and repointing `STD_REV` at it cannot happen in one
    // commit - the rev to point at is the merge commit that does not exist
    // yet - so a PR that legitimately changes std's output would always
    // fail a combined check. CI runs `--check` on every push and this one
    // only on the default branch, where the merge commit exists.
    if std::env::args().any(|a| a == "--check-pin") {
        std::process::exit(if stale_pin()? { 1 } else { 0 });
    }

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
        package: PackageMeta {
            name: "comline_std".to_string(),
            version: "0.1.0".to_string(),
        },
        // The one generator this crate registers, so the version it
        // registers under is the one `comline_std`'s own manifest states.
        lang_version: comline_codegen_rust::LANG_VERSION.to_string(),
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

/// Whether `generator.rs`'s `STD_REV` points at a commit older than the
/// `std-extra/` sitting beside it.
///
/// Checking the *contents* is only half the guarantee. Every consumer
/// generating in `lib` mode links `comline_std` by default, and reaches it
/// through that pinned rev - so a correctly regenerated `std-extra/` with a
/// pin still on the previous commit ships the old std to everyone, and
/// nothing fails anywhere. This is what stands between a regeneration and
/// that.
///
/// Skipped (not failed) when git can't answer - a published tarball, a
/// shallow clone that doesn't contain `STD_REV`. A missing guard beats a
/// false alarm in a tree that cannot run the guard at all.
fn stale_pin() -> eyre::Result<bool> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");

    let reachable = Command::new("git")
        .args(["cat-file", "-e", &format!("{STD_REV}^{{commit}}")])
        .current_dir(&repo)
        .status();
    match reachable {
        Ok(status) if status.success() => {}
        _ => {
            eprintln!("note: STD_REV ({STD_REV}) isn't in this checkout - skipping the pin check");
            return Ok(false);
        }
    }

    let diff = Command::new("git")
        .args(["diff", "--quiet", STD_REV, "HEAD", "--", "std-extra"])
        .current_dir(&repo)
        .status()?;
    if diff.success() {
        return Ok(false);
    }

    eprintln!(
        "STD_REV ({STD_REV}) predates changes to std-extra/ - every `lib`-mode \
         consumer would link the old std.\n\
         Merge this, then repoint STD_REV in codegen/src/generator.rs at the \
         merge commit."
    );
    Ok(true)
}
