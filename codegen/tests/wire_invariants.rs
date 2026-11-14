//! What the generated code puts on the wire, pinned: how a package is laid
//! out in Rust may change (names, imports, which items rustdoc shows), but
//! no wire byte, protocol number, function number, error ordinal, `IR_HASH`,
//! `CONNECTION_IR_HASH` or `HELLO` does (comline-rust#129).
//!
//! `wire_invariants.txt` holds those values for every package of the
//! conformance corpus, in both modes, as the generator produced them before
//! the entry points were reshaped. Regenerating the file is a decision, not
//! a chore: `COMLINE_BLESS=1 cargo test -p comline-codegen-rust --test
//! wire_invariants` writes it, and a diff in it is a wire change.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use comline_codegen::{GeneratedFile, Mode};
use comline_codegen_rust::generate_rust;
use comline_conformance::{corpus, Fixture};

mod common;
use common::{lockstep, request};

/// Everything in `files` that is a wire value, one per line. Each is read
/// off the generated text where it's written: nothing here depends on how
/// the rest of a file is laid out.
fn wire_values(files: &[GeneratedFile]) -> String {
    let mut out = String::new();
    // Every file's own digest, and the package's, which a protocol file may
    // state itself or take from the package's module: the set of values is
    // what's pinned.
    let mut package_hashes = BTreeSet::new();
    let mut files: Vec<&GeneratedFile> = files.iter().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    for file in files {
        let path = file.path.display();
        let mut lines = file.contents.lines();
        while let Some(line) = lines.next() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("pub const IR_HASH: u64 = ") {
                writeln!(out, "{path}: IR_HASH = {}", rest.trim_end_matches(';')).unwrap();
            } else if let Some(rest) = line.strip_prefix("pub const CONNECTION_IR_HASH: u64 = ") {
                package_hashes.insert(rest.trim_end_matches(';').to_string());
            } else if line.starts_with("pub const HELLO: &[u8] = &[") {
                let mut bytes = String::new();
                for line in lines.by_ref() {
                    if line.trim() == "];" {
                        break;
                    }
                    bytes.push_str(line.trim());
                }
                writeln!(out, "{path}: HELLO = {bytes}").unwrap();
            } else if line.contains("PROTOCOL: ") && line.contains("ProtocolNumber = ") {
                writeln!(out, "{path}: {}", line.trim_start_matches("pub ")).unwrap();
            } else if line.contains("_CALLS: &[&str] = ") || line.starts_with("const PROTOCOLS: ") {
                writeln!(out, "{path}: {}", line.trim_start_matches("pub ")).unwrap();
            }
            // Each call's number and name, the number a check is made by,
            // and each error's ordinal on the wire.
            for (marker, stop) in [
                ("Call::new(", ')'),
                ("check_call(", ')'),
                ("check_call_async(", ')'),
                ("reply.err(", ','),
                ("Envelope::Err { id: ", ','),
            ] {
                let mut rest = line;
                while let Some(at) = rest.find(marker) {
                    let tail = &rest[at + marker.len()..];
                    let end = tail.find(stop).unwrap_or(tail.len());
                    writeln!(out, "{path}: {marker}{}", &tail[..end]).unwrap();
                    rest = tail;
                }
            }
        }
    }
    for hash in package_hashes {
        writeln!(out, "CONNECTION_IR_HASH = {hash}").unwrap();
    }
    out
}

fn snapshot() -> String {
    let mut shapes: Vec<(String, Fixture, bool)> = Vec::new();
    for fx in corpus() {
        shapes.push((fx.name.to_string(), fx, false));
    }
    // A connection run async, and the lockstep shape (a connection is the
    // default now).
    for name in ["protocol", "framing_datagram3", "handshake_comline2"] {
        let fx = corpus().into_iter().find(|f| f.name == name).unwrap();
        shapes.push((format!("{name} async"), fx, true));
    }
    let fx = corpus().into_iter().find(|f| f.name == "protocol").unwrap();
    shapes.push(("lockstep".into(), lockstep(fx), false));

    let mut out = String::new();
    for (name, fx, async_api) in shapes {
        for (label, mode) in [("code", Mode::Code), ("lib", Mode::Lib)] {
            let req = request(&fx, mode, async_api);
            // A package the Rust generator refuses has no output to pin.
            let Ok(files) = generate_rust(&req) else {
                continue;
            };
            writeln!(out, "== {name} ({label})").unwrap();
            out.push_str(&wire_values(&files));
        }
    }
    out
}

#[test]
fn the_wire_values_are_the_ones_pinned() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/wire_invariants.txt");
    let now = snapshot();
    if std::env::var_os("COMLINE_BLESS").is_some() {
        std::fs::write(&path, &now).unwrap();
        return;
    }
    let pinned = std::fs::read_to_string(&path).expect("tests/wire_invariants.txt");
    if pinned != now {
        let first = pinned
            .lines()
            .zip(now.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| pinned.lines().count().min(now.lines().count()));
        panic!(
            "a wire value changed (first difference at line {}):\n  pinned: {:?}\n  now:    {:?}\n\
             If that is meant, `COMLINE_BLESS=1` rewrites the file: but no change to how \
             the code reads should ever change one.",
            first + 1,
            pinned.lines().nth(first),
            now.lines().nth(first),
        );
    }
    // The pin isn't empty: it holds hashes, numbers and a Hello.
    assert!(now.contains("HELLO = "), "{now}");
    assert!(now.contains("CONNECTION_IR_HASH = 0x"), "{now}");
    assert!(now.contains("PROTOCOL: "), "{now}");
}
