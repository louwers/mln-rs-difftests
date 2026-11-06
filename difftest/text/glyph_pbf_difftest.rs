// Difftest of crates/mln_text's parse_glyph_pbf against the C++ core's
// parseGlyphPBF: the glyph PBF fixtures of test/fixtures, every glyph PBF of
// metrics/cache-style.db and metrics/cache-metrics.db (the render tests'
// glyphs), and generated PBFs (random valid, invalid and out-of-range
// glyphs, unknown fields, truncations). Glyph ids, metrics and bitmaps must
// be identical, and so must the protozero exception messages.

use mln_text::GlyphRange;
use mln_text::glyph::GlyphIDType;
use mln_text::glyph_pbf::parse_glyph_pbf;
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, TestRng, TestRunner};
use text_reference::*;

macro_rules! report {
    ($($arg:tt)*) => {{
        use std::io::Write;
        writeln!(std::io::stderr(), $($arg)*).unwrap();
    }};
}

/// The serialization of reference.cpp's `ref_parse_glyph_pbf`.
fn rust_parse(first: u32, second: u32, data: &[u8]) -> Result<Vec<u8>, String> {
    let glyphs = parse_glyph_pbf(&GlyphRange::new(first, second, GlyphIDType::FontPBF), data)
        .map_err(|e| e.what().to_string())?;
    let mut out = Vec::new();
    for g in glyphs {
        for v in [
            g.id.hash(),
            g.metrics.width,
            g.metrics.height,
            g.metrics.left.to_bits(),
            g.metrics.top.to_bits(),
            g.metrics.advance,
            g.bitmap.size.width,
            g.bitmap.size.height,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&g.bitmap.data);
    }
    Ok(out)
}

/// `<first>-<second>.pbf` -> (first, second); (0, 65535) otherwise.
fn range_of(name: &str) -> (u32, u32) {
    let stem = name.rsplit('/').next().unwrap().trim_end_matches(".pbf");
    let parts: Vec<&str> = stem.rsplit('-').take(2).collect();
    match (parts.get(1).and_then(|a| a.parse().ok()), parts.first().and_then(|b| b.parse().ok())) {
        (Some(a), Some(b)) => (a, b),
        _ => (0, 65535),
    }
}

fn db_glyph_pbfs(db: &str) -> Vec<(String, Vec<u8>)> {
    db_rows(db, "%glyphs%.pbf").into_iter().map(|(url, data)| (format!("{db}:{url}"), data)).collect()
}

/// protozero writer helpers for the generated corpus.
fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}
fn field_varint(out: &mut Vec<u8>, tag: u32, v: u64) {
    varint(out, (tag as u64) << 3);
    varint(out, v);
}
fn field_bytes(out: &mut Vec<u8>, tag: u32, bytes: &[u8]) {
    varint(out, ((tag as u64) << 3) | 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}
fn zigzag(v: i32) -> u64 {
    ((v << 1) ^ (v >> 31)) as u32 as u64
}

/// A random glyph message: each field present or not, values in and out of
/// the accepted ranges, bitmaps of the right or a wrong size, unknown fields.
fn glyph_strategy() -> impl Strategy<Value = Vec<u8>> {
    (
        proptest::option::weighted(0.95, 0u32..70000),
        proptest::option::weighted(0.95, prop_oneof![0u32..8, 250u32..260]),
        proptest::option::weighted(0.95, prop_oneof![0u32..8, 250u32..260]),
        proptest::option::weighted(0.95, -130i32..130),
        proptest::option::weighted(0.95, -130i32..130),
        proptest::option::weighted(0.95, prop_oneof![0u32..30, 250u32..260]),
        0usize..3,
        proptest::bool::weighted(0.8),
        proptest::collection::vec(any_unknown(), 0..2),
    )
        .prop_map(|(id, w, h, l, t, a, bitmap_mode, correct_size, unknown)| {
            let mut m = Vec::new();
            for (tag, bytes) in &unknown {
                field_bytes(&mut m, *tag, bytes);
            }
            if let Some(id) = id {
                field_varint(&mut m, 1, id as u64);
            }
            if bitmap_mode > 0 {
                let (w, h) = (w.unwrap_or(0) as usize, h.unwrap_or(0) as usize);
                let mut n = (w + 6) * (h + 6);
                if !correct_size {
                    n = n.saturating_sub(1);
                }
                let bitmap: Vec<u8> = (0..n.min(70000)).map(|i| (i * 7) as u8).collect();
                field_bytes(&mut m, 2, &bitmap);
            }
            if let Some(w) = w {
                field_varint(&mut m, 3, w as u64);
            }
            if let Some(h) = h {
                field_varint(&mut m, 4, h as u64);
            }
            if let Some(l) = l {
                field_varint(&mut m, 5, zigzag(l));
            }
            if let Some(t) = t {
                field_varint(&mut m, 6, zigzag(t));
            }
            if let Some(a) = a {
                field_varint(&mut m, 7, a as u64);
            }
            m
        })
}

/// Unknown length-delimited fields (tags 8..20).
fn any_unknown() -> impl Strategy<Value = (u32, Vec<u8>)> {
    (8u32..20, proptest::collection::vec(proptest::num::u8::ANY, 0..5))
}

use proptest::prop_oneof;

/// A glyph PBF: font stacks (tag 1) of glyphs (tag 3), a name (tag 1 inside),
/// optionally truncated.
fn pbf_strategy() -> impl Strategy<Value = (Vec<u8>, Option<usize>)> {
    (
        proptest::collection::vec(proptest::collection::vec(glyph_strategy(), 0..6), 1..3),
        proptest::option::weighted(0.2, 0usize..10000),
    )
        .prop_map(|(stacks, cut)| {
            let mut out = Vec::new();
            for glyphs in stacks {
                let mut stack = Vec::new();
                field_bytes(&mut stack, 1, b"Open Sans Regular");
                for g in glyphs {
                    field_bytes(&mut stack, 3, &g);
                }
                field_varint(&mut stack, 2, 0); // range, unused
                field_bytes(&mut out, 1, &stack);
            }
            let cut = cut.map(|c| c % (out.len() + 1));
            (out, cut)
        })
}

#[test]
fn glyph_pbfs_match() {
    let root = maplibre_root();
    let mut corpus: Vec<(String, Vec<u8>)> = [
        "test/fixtures/resources/glyphs-12244-12543.pbf",
        "test/fixtures/resources/fake_glyphs-0-255.pbf",
        "test/fixtures/resources/glyphs.pbf",
        "test/fixtures/offline_download/glyph.pbf",
        "test/fixtures/map/offline/glyph.pbf",
    ]
    .iter()
    .map(|p| (p.to_string(), std::fs::read(root.join(p)).unwrap()))
    .collect();
    corpus.extend(db_glyph_pbfs("metrics/cache-style.db"));
    corpus.extend(db_glyph_pbfs("metrics/cache-metrics.db"));
    let files = corpus.len();

    let mut runner = TestRunner::new_with_rng(Config::default(), TestRng::deterministic_rng(Default::default()));
    let strategy = pbf_strategy();
    for i in 0..2000 {
        let (mut data, cut) = strategy.new_tree(&mut runner).unwrap().current();
        if let Some(cut) = cut {
            data.truncate(cut);
        }
        corpus.push((format!("generated #{i}"), data));
    }

    let mut glyphs = 0usize;
    let mut errors = 0usize;
    for (name, data) in &corpus {
        let (first, second) = range_of(name);
        let ranges = [(first, second), (0, 255), (256, 511), (0, 65535)];
        for (first, second) in ranges {
            let cpp = ref_parse_glyph_pbf(first, second, data);
            let rust = rust_parse(first, second, data);
            assert_eq!(rust, cpp, "{name} range {first}-{second}");
            match &cpp {
                Ok(bytes) => {
                    // Count glyphs: walk the serialization.
                    let mut p = 0;
                    while p < bytes.len() {
                        let w = u32::from_le_bytes(bytes[p + 24..p + 28].try_into().unwrap()) as usize;
                        let h = u32::from_le_bytes(bytes[p + 28..p + 32].try_into().unwrap()) as usize;
                        p += 32 + w * h;
                        glyphs += 1;
                    }
                }
                Err(_) => errors += 1,
            }
        }
    }
    report!(
        "{} PBFs ({files} files: test/fixtures + cache DBs, {} generated) x 4 ranges: identical; {glyphs} glyphs, {errors} protozero errors",
        corpus.len(),
        corpus.len() - files
    );
}
