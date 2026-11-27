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

/// Shortest-form unsigned LEB128, as the wire writes a number.
fn varint(mut value: u32, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// The bytes of the `Hash128([0x.., ..])` written on `line`.
fn hash_on(line: &str) -> Vec<u8> {
    let inner = line
        .split("Hash128([")
        .nth(1)
        .and_then(|rest| rest.split("])").next())
        .unwrap_or_else(|| panic!("no hash on `{line}`"));
    inner
        .split(", ")
        .map(|byte| u8::from_str_radix(byte.trim_start_matches("0x"), 16).unwrap())
        .collect()
}

/// The numbers in the parentheses or brackets that follow `marker` on `line`.
fn numbers_after(line: &str, marker: &str, close: char) -> Vec<u32> {
    let rest = line
        .split(marker)
        .nth(1)
        .unwrap_or_else(|| panic!("no `{marker}` on `{line}`"));
    let inner = rest.split(close).next().unwrap();
    inner
        .split(',')
        .filter(|n| !n.trim().is_empty())
        .map(|n| n.trim().parse().unwrap())
        .collect()
}

/// A release written `c2::Release::new(0, 1, 0)` on `line`, as the wire has it.
fn release_on(line: &str, out: &mut Vec<u8>) {
    for part in numbers_after(line, "Release::new(", ')') {
        varint(part, out);
    }
}

/// The Hello of the package's whole connection, put together from the header
/// and the protocol entries the package module holds: what its `HELLO` constant
/// was before a Hello was built at runtime, from the same bytes. `lines`
/// starts after the header's opening line.
fn header_bytes<'a>(lines: &mut impl Iterator<Item = &'a str>) -> Vec<u8> {
    let (mut flag, mut id, mut version, mut pre) = (0u8, vec![], vec![], String::new());
    let mut encodings = Vec::new();
    for line in lines.by_ref() {
        let line = line.trim();
        if line == "};" {
            break;
        }
        if line.starts_with("exchange: ") {
            flag = u8::from(line.contains("Symmetric"));
        } else if line.starts_with("package_id: ") {
            id = hash_on(line);
        } else if line.starts_with("version: ") {
            release_on(line, &mut version);
        } else if let Some(rest) = line.strip_prefix("pre: \"") {
            pre = rest.trim_end_matches("\",").to_string();
        } else if line.starts_with("c2::Encoding {") {
            let kind = ["Format", "Framing", "Handshake"]
                .iter()
                .position(|k| line.contains(&format!("EncodingKind::{k},")))
                .unwrap();
            encodings.push((kind as u8, hash_on(line)));
        }
    }
    let mut out = vec![b'C', b'O', 2, 1, flag];
    out.extend(id);
    out.extend(version);
    varint(pre.len() as u32, &mut out);
    out.extend(pre.bytes());
    varint(encodings.len() as u32, &mut out);
    for (kind, hash) in encodings {
        out.push(kind);
        out.extend(hash);
    }
    out
}

/// One protocol's entry, as the Hello lists it. `lines` starts after the
/// entry's opening line.
fn entry_bytes<'a>(lines: &mut impl Iterator<Item = &'a str>) -> Vec<u8> {
    let mut out = Vec::new();
    for line in lines.by_ref() {
        let line = line.trim();
        if line == "};" {
            break;
        }
        if line.starts_with("number: ") {
            varint(numbers_after(line, "number: ", ',')[0], &mut out);
        } else if line.starts_with("line: ") {
            release_on(line, &mut out);
        } else if line.starts_with("shape_hash: ") {
            out.extend(hash_on(line));
        } else if line.starts_with("functions: ") {
            let functions = numbers_after(line, "functions: &[", ']');
            varint(functions.len() as u32, &mut out);
            for f in functions {
                varint(f, &mut out);
            }
        }
    }
    out
}

/// Everything in `files` that is a wire value, one per line. Each is read
/// off the generated text where it's written: nothing here depends on how
/// the rest of a file is laid out.
///
/// A package's Hello is built at runtime from the header and the entries its
/// module holds, so the whole connection's Hello (`HELLO = ..`) is put
/// together here from them: the bytes the pinned file has held since before
/// that, so any change to a value in the header or an entry shows as one.
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
        let mut header: Option<Vec<u8>> = None;
        let mut entries: Vec<Vec<u8>> = Vec::new();
        while let Some(line) = lines.next() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("pub const IR_HASH: u64 = ") {
                writeln!(out, "{path}: IR_HASH = {}", rest.trim_end_matches(';')).unwrap();
            } else if let Some(rest) = line.strip_prefix("pub const CONNECTION_IR_HASH: u64 = ") {
                package_hashes.insert(rest.trim_end_matches(';').to_string());
            } else if line.starts_with("pub const HEADER: c2::HelloHeader") {
                header = Some(header_bytes(&mut lines));
            } else if line.starts_with("pub const P_") && line.contains(": c2::ProtocolEntry") {
                entries.push(entry_bytes(&mut lines));
            } else if line.starts_with("const PROTOCOLS: ") && header.is_some() {
                let mut hello = header.take().unwrap_or_default();
                varint(entries.len() as u32, &mut hello);
                hello.extend(entries.drain(..).flatten());
                let bytes: Vec<String> = hello.iter().map(|b| format!("{b:#04x}")).collect();
                let bytes: String = bytes
                    .chunks(12)
                    .map(|chunk| format!("{},", chunk.join(", ")))
                    .collect();
                writeln!(out, "{path}: HELLO = {bytes}").unwrap();
                writeln!(out, "{path}: {}", line.trim_start_matches("pub ")).unwrap();
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
