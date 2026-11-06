// Differential test of //vendor/protozero against the C++ protozero: a walk
// over every field of a buffer with every getter of the field's wire type
// (on copies of the reader) and every packed iterator type, recursing into
// length-delimited fields (pbf_reference.cpp has the same walk), plus
// decode_varint/skip_varint on raw bytes. Inputs: every .pbf/.mvt fixture
// (glyph PBFs, protozero's test data, tiles), random bytes and generated
// messages with random (sometimes malformed) fields.

use std::fmt::Write;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use protozero::{Exception, FixedValue, IteratorRange, PbfReader, PbfWireType};

unsafe extern "C" {
    fn mln_pbf_ref_walk(data: *const u8, size: usize, out_len: *mut usize) -> *mut c_char;
    fn mln_pbf_ref_varint(data: *const u8, size: usize, out_len: *mut usize) -> *mut c_char;
    fn mln_pbf_ref_free(p: *mut c_char);
}

fn take(p: *mut c_char, len: usize) -> String {
    // SAFETY: the reference returns a malloc'd buffer of `len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(p as *const u8, len) }.to_vec();
    // SAFETY: allocated by the reference, freed once.
    unsafe { mln_pbf_ref_free(p) };
    String::from_utf8(bytes).unwrap()
}

fn reference_walk(data: &[u8]) -> String {
    let mut len = 0;
    // SAFETY: (ptr, len) of a live slice.
    take(unsafe { mln_pbf_ref_walk(data.as_ptr(), data.len(), &mut len) }, len)
}

fn reference_varint(data: &[u8]) -> String {
    let mut len = 0;
    // SAFETY: (ptr, len) of a live slice.
    take(unsafe { mln_pbf_ref_varint(data.as_ptr(), data.len(), &mut len) }, len)
}

fn hex(out: &mut String, data: &[u8]) {
    out.push('x');
    for b in data {
        let _ = write!(out, "{b:02x}");
    }
}

fn num(out: &mut String, v: impl std::fmt::Display) {
    let _ = write!(out, " {v}");
}

fn f32bits(out: &mut String, v: f32) {
    let _ = write!(out, " f{:08x}", v.to_bits());
}

fn f64bits(out: &mut String, v: f64) {
    let _ = write!(out, " d{:016x}", v.to_bits());
}

fn attempt(out: &mut String, f: impl FnOnce(&mut String) -> Result<(), Exception>) {
    if let Err(e) = f(out) {
        out.push_str(" !");
        out.push_str(e.what());
    }
}

/// `packed(...)` over varint/svarint ranges: C++ `*it++` until end().
fn packed_varint<T: protozero::VarintValue + std::fmt::Display>(
    out: &mut String,
    name: &str,
    range: IteratorRange<protozero::ConstVarintIterator<'_, T>>,
) {
    let _ = write!(out, " {name}[{}]", range.size());
    let mut it = range.begin();
    let end = range.end();
    while it != end {
        match it.post_increment().and_then(|tmp| tmp.deref()) {
            Ok(v) => num(out, v),
            Err(e) => {
                out.push_str(" !");
                out.push_str(e.what());
                return;
            }
        }
    }
}

fn packed_svarint<T: protozero::SvarintValue + std::fmt::Display>(
    out: &mut String,
    name: &str,
    range: IteratorRange<protozero::ConstSvarintIterator<'_, T>>,
) {
    let _ = write!(out, " {name}[{}]", range.size());
    let mut it = range.begin();
    let end = range.end();
    while it != end {
        match it.post_increment().and_then(|tmp| tmp.deref()) {
            Ok(v) => num(out, v),
            Err(e) => {
                out.push_str(" !");
                out.push_str(e.what());
                return;
            }
        }
    }
}

fn packed_fixed<T: FixedValue>(
    out: &mut String,
    name: &str,
    range: IteratorRange<protozero::ConstFixedIterator<'_, T>>,
    emit: impl Fn(&mut String, T),
) {
    let _ = write!(out, " {name}[{}]", range.size());
    for v in range {
        emit(out, v);
    }
}

fn walk(out: &mut String, data: &[u8], depth: u32) {
    let mut r = PbfReader::new(data);
    loop {
        match r.next() {
            Ok(false) => {
                out.push_str("end\n");
                return;
            }
            Ok(true) => {}
            Err(e) => {
                let _ = writeln!(out, "next! {} tag {} wt {}", e.what(), r.tag(), r.wire_type().0);
                return;
            }
        }
        let _ = write!(out, "f {} {} {}", r.tag(), r.wire_type().0, r.tag_and_type());
        let mut nested = String::new();
        match r.wire_type() {
            PbfWireType::Varint => {
                attempt(out, |o| Ok(num(o, r.clone().get_uint64()?)));
                attempt(out, |o| Ok(num(o, r.clone().get_int64()?)));
                attempt(out, |o| Ok(num(o, r.clone().get_sint64()?)));
                attempt(out, |o| Ok(num(o, r.clone().get_uint32()?)));
                attempt(out, |o| Ok(num(o, r.clone().get_int32()?)));
                attempt(out, |o| Ok(num(o, r.clone().get_sint32()?)));
                attempt(out, |o| Ok(num(o, r.clone().get_enum()?)));
                attempt(out, |o| Ok(num(o, u8::from(r.clone().get_bool()?))));
            }
            PbfWireType::Fixed64 => {
                attempt(out, |o| Ok(num(o, r.clone().get_fixed64()?)));
                attempt(out, |o| Ok(num(o, r.clone().get_sfixed64()?)));
                attempt(out, |o| Ok(f64bits(o, r.clone().get_double()?)));
            }
            PbfWireType::Fixed32 => {
                attempt(out, |o| Ok(num(o, r.clone().get_fixed32()?)));
                attempt(out, |o| Ok(num(o, r.clone().get_sfixed32()?)));
                attempt(out, |o| Ok(f32bits(o, r.clone().get_float()?)));
            }
            PbfWireType::LengthDelimited => {
                attempt(out, |o| {
                    let mut c = r;
                    let v = c.get_view()?;
                    o.push(' ');
                    hex(o, v);
                    if depth < 3 {
                        walk(&mut nested, v, depth + 1);
                    }
                    let _ = write!(o, " rest {}", c.length());
                    Ok(())
                });
                attempt(out, |o| Ok(packed_varint(o, "u32", r.clone().get_packed_uint32()?)));
                attempt(out, |o| Ok(packed_varint(o, "i32", r.clone().get_packed_int32()?)));
                attempt(out, |o| Ok(packed_svarint(o, "s32", r.clone().get_packed_sint32()?)));
                attempt(out, |o| Ok(packed_varint(o, "u64", r.clone().get_packed_uint64()?)));
                attempt(out, |o| Ok(packed_varint(o, "i64", r.clone().get_packed_int64()?)));
                attempt(out, |o| Ok(packed_svarint(o, "s64", r.clone().get_packed_sint64()?)));
                attempt(out, |o| Ok(packed_varint(o, "bool", r.clone().get_packed_bool()?)));
                attempt(out, |o| Ok(packed_varint(o, "enum", r.clone().get_packed_enum()?)));
                attempt(out, |o| Ok(packed_fixed(o, "f32", r.clone().get_packed_fixed32()?, |o, v| num(o, v))));
                attempt(out, |o| Ok(packed_fixed(o, "sf32", r.clone().get_packed_sfixed32()?, |o, v| num(o, v))));
                attempt(out, |o| Ok(packed_fixed(o, "f64", r.clone().get_packed_fixed64()?, |o, v| num(o, v))));
                attempt(out, |o| Ok(packed_fixed(o, "sf64", r.clone().get_packed_sfixed64()?, |o, v| num(o, v))));
                attempt(out, |o| Ok(packed_fixed(o, "float", r.clone().get_packed_float()?, f32bits)));
                attempt(out, |o| Ok(packed_fixed(o, "double", r.clone().get_packed_double()?, f64bits)));
            }
            _ => {}
        }
        out.push('\n');
        out.push_str(&nested);
        if let Err(e) = r.skip() {
            let _ = writeln!(out, "skip! {}", e.what());
            return;
        }
        let _ = writeln!(out, "len {}", r.length());
    }
}

fn rust_walk(data: &[u8]) -> String {
    let mut out = String::new();
    walk(&mut out, data, 0);
    out
}

fn rust_varint(data: &[u8]) -> String {
    let mut out = String::new();
    let mut d: &[u8] = data;
    match protozero::decode_varint(&mut d) {
        Ok(v) => {
            let _ = write!(out, "{v} {}", data.len() - d.len());
        }
        Err(e) => {
            let _ = write!(out, "!{} {}", e.what(), data.len() - d.len());
        }
    }
    let mut d: &[u8] = data;
    match protozero::skip_varint(&mut d) {
        Ok(()) => {
            let _ = write!(out, " skip {}", data.len() - d.len());
        }
        Err(e) => {
            let _ = write!(out, " skip!{} {}", e.what(), data.len() - d.len());
        }
    }
    out
}

fn first_difference(a: &str, b: &str) -> String {
    for (i, (la, lb)) in a.lines().zip(b.lines()).enumerate() {
        if la != lb {
            return format!(
                "line {}:\n  C++:  {}\n  Rust: {}",
                i + 1,
                &la[..la.len().min(300)],
                &lb[..lb.len().min(300)]
            );
        }
    }
    format!("length differs: {} vs {} lines", a.lines().count(), b.lines().count())
}

fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk_dir(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn fixture_pbfs() {
    let base = std::env::var("RUNFILES_DIR").or_else(|_| std::env::var("TEST_SRCDIR")).unwrap();
    let root = PathBuf::from(base).join("maplibre+");
    let mut files = Vec::new();
    for dir in ["metrics", "test/fixtures", "vendor/vector-tile", "vendor/protozero"] {
        walk_dir(&root.join(dir), &mut files);
    }
    let (mut n, mut fields, mut mismatches) = (0, 0, 0);
    for f in files {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "pbf" | "mvt" | "tile") {
            continue;
        }
        let data = std::fs::read(&f).unwrap();
        let cpp = reference_walk(&data);
        let rust = rust_walk(&data);
        n += 1;
        fields += cpp.lines().filter(|l| l.starts_with("f ")).count();
        if cpp != rust {
            mismatches += 1;
            if mismatches < 10 {
                eprintln!("{}: {}", f.display(), first_difference(&cpp, &rust));
            }
        }
    }
    eprintln!("fixture PBFs: {n} files, {fields} fields walked, {mismatches} mismatches");
    assert!(n > 300, "found only {n} files");
    assert_eq!(mismatches, 0);
}

/// A random protobuf message: valid keys with any wire type (also 3, 4, 6,
/// 7 and reserved tags), payloads that may be truncated or malformed.
fn message_strategy() -> impl Strategy<Value = Vec<u8>> {
    let field = (
        prop_oneof![8 => 1u64..40, 1 => 18990u64..20010, 1 => any::<u32>().prop_map(u64::from)],
        prop_oneof![5 => Just(0u64), 2 => Just(1u64), 5 => Just(2u64), 2 => Just(5u64), 1 => 0u64..8],
        proptest::collection::vec(any::<u8>(), 0..12),
        any::<u64>(),
        0u8..6,
    )
        .prop_map(|(tag, wt, payload, value, shape)| {
            let mut out = Vec::new();
            protozero::add_varint_to_buffer(&mut out, (tag << 3) | wt);
            match wt {
                0 => match shape {
                    0 => out.extend_from_slice(&[0x80, 0x00]),
                    1 => out.extend_from_slice(&[0xff; 10]),
                    _ => protozero::add_varint_to_buffer(&mut out, value >> (value % 64)),
                },
                1 => out.extend_from_slice(&value.to_le_bytes()[..if shape == 0 { 5 } else { 8 }]),
                5 => out.extend_from_slice(&value.to_le_bytes()[..if shape == 0 { 3 } else { 4 }]),
                2 => {
                    let len = if shape == 0 { payload.len() as u64 + 3 } else { payload.len() as u64 };
                    protozero::add_varint_to_buffer(&mut out, len);
                    out.extend_from_slice(&payload);
                }
                _ => out.extend_from_slice(&payload),
            }
            out
        });
    proptest::collection::vec(field, 0..8).prop_map(|fields| fields.concat())
}

/// Nested messages: a message whose length-delimited fields hold messages.
fn nested_strategy() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec((1u64..8, message_strategy()), 0..5).prop_map(|parts| {
        let mut out = Vec::new();
        for (tag, msg) in parts {
            protozero::add_varint_to_buffer(&mut out, (tag << 3) | 2);
            protozero::add_varint_to_buffer(&mut out, msg.len() as u64);
            out.extend_from_slice(&msg);
        }
        out
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 20000, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn random_bytes(data in proptest::collection::vec(any::<u8>(), 0..64)) {
        prop_assert_eq!(rust_walk(&data), reference_walk(&data));
        prop_assert_eq!(rust_varint(&data), reference_varint(&data));
    }

    #[test]
    fn generated_messages(data in message_strategy()) {
        prop_assert_eq!(rust_walk(&data), reference_walk(&data));
    }

    #[test]
    fn nested_messages(data in nested_strategy()) {
        prop_assert_eq!(rust_walk(&data), reference_walk(&data));
    }

    #[test]
    fn varints(
        prefix in proptest::collection::vec(0x80u8..=0xff, 0..12),
        last in any::<u8>(),
        tail in proptest::collection::vec(any::<u8>(), 0..3),
    ) {
        let mut data = prefix;
        data.push(last);
        data.extend_from_slice(&tail);
        prop_assert_eq!(rust_varint(&data), reference_varint(&data));
        // Without the terminating byte (the slow path's end of buffer).
        let cut = &data[..data.len() - 1 - tail.len()];
        prop_assert_eq!(rust_varint(cut), reference_varint(cut));
    }
}
