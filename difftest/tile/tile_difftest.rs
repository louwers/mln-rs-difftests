// Differential test of the MVT decoding path: //crates/mln_tile's
// VectorMVTTileData (on //vendor/vector-tile, //vendor/protozero and, for
// version 1 polygons, //vendor/wagyu) against the C++ VectorMVTTileData.
//
// Inputs:
// 1. every file in the C++ fixtures that may be a vector tile (.mvt, .pbf,
//    .tile) under metrics/integration, test/fixtures, vendor/vector-tile
//    and vendor/protozero test data, plus every blob in the SQLite fixtures
//    (offline/cache databases, MBTiles); data with a gzip/zlib header is
//    inflated first, as `util::decompress` does (both are compared);
// 2. truncations of each real tile;
// 3. generated tiles with malformed content but correct wire types for the
//    known fields (bad varints, odd tags, out-of-range indexes, unknown
//    commands, huge coordinates, extent 0, ...);
// 4. with an NDEBUG oracle (--config=oracle-opt) only: random byte
//    mutations, which reach protozero's wire-type assertions in debug C++.
//
// Both sides write the same dump (dump.rs / reference.cpp); C++ byte strings
// are normalized UTF-8-lossy first (see DEVIATIONS.md).

mod dump;

use std::io::Read;
use std::path::{Path, PathBuf};

use dump::*;

fn runfiles() -> PathBuf {
    let base =
        std::env::var("RUNFILES_DIR").or_else(|_| std::env::var("TEST_SRCDIR")).expect("RUNFILES_DIR or TEST_SRCDIR");
    PathBuf::from(base).join("maplibre+")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// `util::is_compressed`.
fn is_compressed(v: &[u8]) -> bool {
    if v.len() > 2 {
        if v[0] == 0x1f && v[1] == 0x8b {
            return true;
        }
        if v[0] == 0x78 {
            return matches!(v[1], 0x01 | 0x5e | 0x9c | 0xda);
        }
    }
    false
}

/// `util::decompress` (zlib inflate with gzip/zlib auto-detection).
fn decompress(v: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let ok = if v.len() >= 2 && v[0] == 0x1f && v[1] == 0x8b {
        flate2::read::GzDecoder::new(v).read_to_end(&mut out).is_ok()
    } else {
        flate2::read::ZlibDecoder::new(v).read_to_end(&mut out).is_ok()
    };
    ok.then_some(out)
}

fn sqlite_blobs(path: &Path) -> Vec<(String, Vec<u8>)> {
    use rusqlite::{Connection, OpenFlags};
    let uri = format!("file:{}?immutable=1", path.display());
    let Ok(conn) = Connection::open_with_flags(&uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI)
    else {
        return Vec::new();
    };
    let mut blobs = Vec::new();
    for (table, data, compressed) in
        [("tiles", "data", "compressed"), ("resources", "data", "compressed"), ("tiles", "tile_data", "0")]
    {
        let sql = format!("SELECT {data}, {compressed} FROM {table}");
        let Ok(mut stmt) = conn.prepare(&sql) else { continue };
        let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, Option<Vec<u8>>>(0)?, row.get::<_, i64>(1)?))) else {
            continue;
        };
        for (i, row) in rows.flatten().enumerate() {
            if let (Some(blob), compressed) = row {
                let name = format!("{}#{table}[{i}]", path.display());
                if compressed != 0 {
                    if let Some(d) = decompress(&blob) {
                        blobs.push((name, d));
                        continue;
                    }
                }
                blobs.push((name, blob));
            }
        }
    }
    blobs
}

/// The real inputs: (name, bytes).
fn corpus() -> Vec<(String, Vec<u8>)> {
    let root = runfiles();
    let mut files = Vec::new();
    for dir in ["metrics", "test/fixtures", "vendor/vector-tile", "vendor/protozero"] {
        walk(&root.join(dir), &mut files);
    }
    let mut inputs = Vec::new();
    for f in files {
        let name = f.strip_prefix(&root).unwrap_or(&f).display().to_string();
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "mvt" | "pbf" | "tile" => {
                let data = std::fs::read(&f).unwrap();
                if is_compressed(&data) {
                    if let Some(d) = decompress(&data) {
                        inputs.push((format!("{name} (inflated)"), d));
                    }
                }
                inputs.push((name, data));
            }
            "db" | "mbtiles" => {
                for (n, blob) in sqlite_blobs(&f) {
                    let n = n.strip_prefix(&format!("{}/", root.display())).unwrap_or(&n).to_string();
                    inputs.push((n, blob));
                }
            }
            _ => {}
        }
    }
    inputs
}

/// A small deterministic RNG (splitmix64) for the generated inputs.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

// --- a minimal protobuf/MVT encoder for the generated tiles ---------------

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7f) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn key(out: &mut Vec<u8>, tag: u32, wire: u32) {
    varint(out, u64::from(tag << 3 | wire));
}

fn ld(out: &mut Vec<u8>, tag: u32, bytes: &[u8]) {
    key(out, tag, 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn vfield(out: &mut Vec<u8>, tag: u32, v: u64) {
    key(out, tag, 0);
    varint(out, v);
}

/// A field with a tag no reader knows, of any valid wire type.
fn unknown_field(rng: &mut Rng, out: &mut Vec<u8>, tag: u32) {
    match rng.below(4) {
        0 => vfield(out, tag, rng.next() >> rng.below(64)),
        1 => {
            key(out, tag, 1);
            out.extend_from_slice(&rng.next().to_le_bytes());
        }
        2 => ld(out, tag, &rng.next().to_le_bytes()[..rng.below(9) as usize]),
        _ => {
            key(out, tag, 5);
            out.extend_from_slice(&(rng.next() as u32).to_le_bytes());
        }
    }
}

const STRINGS: [&str; 10] = ["name", "type", "class", "", "lake", "water", "ä", "日本", "x y", "name"];

fn gen_string(rng: &mut Rng) -> Vec<u8> {
    if rng.chance(3) {
        // Invalid UTF-8 (values only; see the normalization).
        return vec![b'a', 0xff, 0xbf, b'z'];
    }
    STRINGS[rng.below(STRINGS.len() as u64) as usize].as_bytes().to_vec()
}

fn gen_value(rng: &mut Rng) -> Vec<u8> {
    let mut v = Vec::new();
    let n = if rng.chance(10) { rng.below(4) } else { 1 };
    for _ in 0..n {
        match rng.below(9) {
            0 => {
                let s = gen_string(rng);
                ld(&mut v, 1, &s)
            }
            1 => {
                key(&mut v, 2, 5);
                let f = match rng.below(5) {
                    0 => f32::NAN.to_bits(),
                    1 => (-0.0f32).to_bits(),
                    2 => f32::INFINITY.to_bits(),
                    _ => rng.next() as u32,
                };
                v.extend_from_slice(&f.to_le_bytes());
            }
            2 => {
                key(&mut v, 3, 1);
                v.extend_from_slice(&rng.next().to_le_bytes());
            }
            3 => vfield(&mut v, 4, rng.next() >> rng.below(64)),
            4 => vfield(&mut v, 5, rng.next() >> rng.below(64)),
            5 => vfield(&mut v, 6, rng.next() >> rng.below(64)),
            6 => {
                // bool, sometimes non-canonical (80 00 reads as true).
                key(&mut v, 7, 0);
                match rng.below(3) {
                    0 => v.push(0),
                    1 => v.push(1),
                    _ => v.extend_from_slice(&[0x80, 0x00]),
                }
            }
            _ => {
                let tag = 8 + rng.below(20) as u32;
                unknown_field(rng, &mut v, tag)
            }
        }
    }
    if rng.chance(3) {
        // A truncated value message.
        v.pop();
    }
    v
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn gen_geometry(rng: &mut Rng, geom_type: u64) -> Vec<u8> {
    let mut g = Vec::new();
    let commands = 1 + rng.below(8);
    let big = rng.chance(10);
    for _ in 0..commands {
        let cmd = match rng.below(20) {
            0 => rng.below(8) as u32, // includes the unknown 0, 3, 4, 5, 6
            1..=6 => 1,
            7..=14 => 2,
            _ => 7,
        };
        let count = match rng.below(20) {
            0 => 0,
            1 => 1 + rng.below(100_000) as u32,
            _ => 1 + rng.below(if geom_type == 1 { 3 } else { 6 }) as u32,
        };
        varint(&mut g, u64::from(cmd | (count << 3)) & 0xffff_ffff);
        if cmd == 1 || cmd == 2 {
            let params = if count > 50 { rng.below(10) } else { u64::from(count) * 2 };
            for _ in 0..params {
                let d = if big { rng.next() as i32 as i64 >> rng.below(16) } else { rng.below(400) as i64 - 200 };
                varint(&mut g, zigzag(d) & 0xffff_ffff);
            }
        }
    }
    if rng.chance(4) {
        // A varint running into the end of the packed field.
        g.push(0x80);
    }
    g
}

fn gen_feature(rng: &mut Rng, nkeys: u64, nvalues: u64) -> Vec<u8> {
    let mut f = Vec::new();
    let fields = 1 + rng.below(6);
    let geom_type = match rng.below(12) {
        0 => 0,
        1 => 4 + rng.below(300),
        2 => u64::MAX, // a negative int32 enum
        n => 1 + n % 3,
    };
    if rng.chance(80) {
        vfield(&mut f, 1, rng.next() >> rng.below(64));
    }
    for _ in 0..fields {
        match rng.below(6) {
            0 | 1 => {
                let mut tags = Vec::new();
                let pairs = rng.below(5);
                for _ in 0..pairs {
                    varint(&mut tags, if rng.chance(5) { nkeys + rng.below(3) } else { rng.below(nkeys.max(1)) });
                    varint(&mut tags, if rng.chance(5) { nvalues + rng.below(3) } else { rng.below(nvalues.max(1)) });
                }
                if rng.chance(8) {
                    varint(&mut tags, rng.below(4));
                }
                if rng.chance(4) {
                    tags.push(0x80);
                }
                ld(&mut f, 2, &tags);
            }
            2 => vfield(&mut f, 3, geom_type),
            3 | 4 => {
                let g = gen_geometry(rng, geom_type);
                ld(&mut f, 4, &g)
            }
            _ => {
                let tag = 5 + rng.below(10) as u32;
                unknown_field(rng, &mut f, tag)
            }
        }
    }
    if rng.chance(3) {
        f.pop();
    }
    f
}

fn gen_layer(rng: &mut Rng, names: &[&str]) -> Vec<u8> {
    let mut l = Vec::new();
    let nkeys = rng.below(6);
    let nvalues = rng.below(6);
    if !rng.chance(4) {
        ld(&mut l, 1, names[rng.below(names.len() as u64) as usize].as_bytes());
    }
    if !rng.chance(4) {
        vfield(&mut l, 15, [1, 2, 2, 2, 3, 1 << 33][rng.below(6) as usize]);
    }
    if !rng.chance(4) {
        vfield(&mut l, 5, [4096, 4096, 512, 8192, 0, 1, 1 << 32, 3][rng.below(8) as usize]);
    }
    for _ in 0..nkeys {
        let k = STRINGS[rng.below(STRINGS.len() as u64) as usize];
        ld(&mut l, 3, k.as_bytes());
    }
    for _ in 0..nvalues {
        let v = gen_value(rng);
        ld(&mut l, 4, &v);
    }
    for _ in 0..rng.below(5) {
        let f = gen_feature(rng, nkeys, nvalues);
        ld(&mut l, 2, &f);
    }
    if rng.chance(10) {
        {
            let tag = 6 + rng.below(8) as u32;
            unknown_field(rng, &mut l, tag)
        }
    }
    if rng.chance(5) {
        // A second name field (the last one counts).
        ld(&mut l, 1, b"renamed");
    }
    l
}

fn gen_tile(rng: &mut Rng) -> Vec<u8> {
    let names = ["a", "b", "water", "a b", ""];
    let mut t = Vec::new();
    for _ in 0..rng.below(4) {
        match rng.below(8) {
            0 => {
                let tag = [1, 2, 4, 5, 19, 20000][rng.below(6) as usize];
                unknown_field(rng, &mut t, tag)
            }
            _ => {
                let l = gen_layer(rng, &names);
                ld(&mut t, 3, &l)
            }
        }
    }
    match rng.below(30) {
        0 => {
            // Bad key varints / tags (exceptions in next()).
            t.extend_from_slice(&[0xff; 11]);
        }
        1 => t.push(0x00),               // tag 0
        2 => t.push((1 << 3) | 3),       // wire type 3
        3 => varint(&mut t, 19500 << 3), // reserved tag
        4 => {
            t.pop();
        }
        _ => {}
    }
    t
}

// --- inputs that reach a protozero assertion in debug C++ -----------------

/// Whether decoding `data` may reach one of protozero's wire-type
/// assertions (a known field of the wrong wire type), which abort the C++
/// oracle when it is built without NDEBUG. Over-approximates: it checks
/// every field the C++ could read.
fn may_assert(data: &[u8]) -> bool {
    use protozero::{PbfReader, PbfWireType};

    fn value(view: &[u8]) -> bool {
        let mut r = PbfReader::new(view);
        while let Ok(true) = r.next() {
            let expected = match r.tag() {
                1 => PbfWireType::LengthDelimited,
                2 => PbfWireType::Fixed32,
                3 => PbfWireType::Fixed64,
                4..=7 => PbfWireType::Varint,
                _ => r.wire_type(),
            };
            if r.wire_type() != expected {
                return true;
            }
            if r.skip().is_err() {
                return false;
            }
        }
        false
    }

    fn feature(view: &[u8]) -> bool {
        let mut r = PbfReader::new(view);
        while let Ok(true) = r.next() {
            let expected = match r.tag() {
                1 | 3 => PbfWireType::Varint,
                2 | 4 => PbfWireType::LengthDelimited,
                _ => r.wire_type(),
            };
            if r.wire_type() != expected {
                return true;
            }
            if r.skip().is_err() {
                return false;
            }
        }
        false
    }

    fn layer(view: &[u8]) -> bool {
        let mut r = PbfReader::new(view);
        while let Ok(true) = r.next() {
            match r.tag() {
                1..=4 => {
                    if r.wire_type() != PbfWireType::LengthDelimited {
                        return true;
                    }
                    let tag = r.tag();
                    let Ok(v) = r.get_view() else { return false };
                    if (tag == 2 && feature(v)) || (tag == 4 && value(v)) {
                        return true;
                    }
                }
                5 | 15 => {
                    if r.wire_type() != PbfWireType::Varint {
                        return true;
                    }
                    if r.skip().is_err() {
                        return false;
                    }
                }
                _ => {
                    if r.skip().is_err() {
                        return false;
                    }
                }
            }
        }
        false
    }

    let mut r = PbfReader::new(data);
    while let Ok(true) = r.next() {
        if r.tag() == 3 {
            if r.wire_type() != PbfWireType::LengthDelimited {
                return true;
            }
            let Ok(v) = r.get_view() else { return false };
            if layer(v) {
                return true;
            }
        } else if r.skip().is_err() {
            return false;
        }
    }
    false
}

struct Totals {
    skipped_debug_assert: usize,
    inputs: usize,
    with_layers: usize,
    layers: usize,
    features: usize,
    mismatches: usize,
    lossy_matched: usize,
    lossy_mismatched: usize,
    invalid_utf8_layer_names: usize,
    reports: Vec<String>,
}

impl Totals {
    fn new() -> Self {
        Totals {
            skipped_debug_assert: 0,
            inputs: 0,
            with_layers: 0,
            layers: 0,
            features: 0,
            mismatches: 0,
            lossy_matched: 0,
            lossy_mismatched: 0,
            invalid_utf8_layer_names: 0,
            reports: Vec::new(),
        }
    }

    fn check(&mut self, name: &str, data: &[u8]) {
        if !reference_ndebug() && may_assert(data) {
            self.skipped_debug_assert += 1;
            return;
        }
        let cpp = reference_decode(data);
        let mut stats = DumpStats::default();
        let rust = rust_decode(data, &mut stats);
        self.inputs += 1;
        if stats.layers > 0 {
            self.with_layers += 1;
        }
        self.layers += stats.layers;
        self.features += stats.features;
        if cpp == rust {
            return;
        }
        let (normalized, lossy) = normalize_lossy(&cpp);
        if lossy && normalized == rust {
            self.lossy_matched += 1;
            return;
        }
        if lossy || stats.invalid_utf8_layer_names > 0 {
            // Invalid UTF-8 the port cannot represent (DEVIATIONS.md).
            self.lossy_mismatched += 1;
            self.invalid_utf8_layer_names += stats.invalid_utf8_layer_names;
            if self.reports.len() < 5 {
                self.reports.push(format!("[lossy] {name}: {}", first_difference(&normalized, &rust)));
            }
            return;
        }
        self.mismatches += 1;
        if self.reports.len() < 20 {
            self.reports.push(format!("{name}: {}", first_difference(&cpp, &rust)));
        }
    }

    fn finish(&self, what: &str) {
        eprintln!(
            "{what}: {} inputs ({} with layers), {} layers, {} features; {} mismatches; \
             invalid UTF-8: {} matched after lossy normalization, {} not comparable; \
             {} skipped (reach a C++ debug assertion; compared with --config=oracle-opt)",
            self.inputs,
            self.with_layers,
            self.layers,
            self.features,
            self.mismatches,
            self.lossy_matched,
            self.lossy_mismatched,
            self.skipped_debug_assert
        );
        for r in &self.reports {
            eprintln!("{r}");
        }
        assert_eq!(self.mismatches, 0, "{what}: mismatches (see stderr)");
    }
}

#[test]
fn fixture_tiles() {
    let inputs = corpus();
    assert!(inputs.len() > 200, "found only {} inputs", inputs.len());
    let mut totals = Totals::new();
    for (name, data) in &inputs {
        totals.check(name, data);
    }
    totals.finish("fixture tiles");
    assert!(totals.features > 100_000, "decoded only {} features", totals.features);
}

#[test]
fn truncated_fixture_tiles() {
    let mut totals = Totals::new();
    let mut rng = Rng(7);
    for (name, data) in corpus() {
        if data.first() != Some(&0x1a) {
            continue;
        }
        for _ in 0..4 {
            let cut = rng.below(data.len() as u64) as usize;
            totals.check(&format!("{name}[..{cut}]"), &data[..cut]);
        }
        for (i, t) in inner_truncations(&data, &mut rng).iter().enumerate() {
            totals.check(&format!("{name} (inner truncation {i})"), t);
        }
    }
    totals.finish("truncated tiles");
}

/// One field of a message: its raw bytes and, if length-delimited, payload.
struct RawField<'a> {
    tag: u32,
    raw: &'a [u8],
    payload: Option<&'a [u8]>,
}

fn split_fields(data: &[u8]) -> Option<Vec<RawField<'_>>> {
    let mut r = protozero::PbfReader::new(data);
    let mut out = Vec::new();
    loop {
        let before = r.data();
        match r.next() {
            Ok(true) => {}
            Ok(false) => return Some(out),
            Err(_) => return None,
        }
        let tag = r.tag();
        let payload = if r.wire_type() == protozero::PbfWireType::LengthDelimited { r.get_view().ok() } else { None };
        if payload.is_none() && r.skip().is_err() {
            return None;
        }
        let after = r.data();
        out.push(RawField { tag, raw: &before[..before.len() - after.len()], payload });
    }
}

/// The message with field `index` replaced by a length-delimited field of
/// the same tag holding `payload`.
fn replace_field(fields: &[RawField<'_>], index: usize, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        if i == index {
            ld(&mut out, f.tag, payload);
        } else {
            out.extend_from_slice(f.raw);
        }
    }
    out
}

/// Truncations inside a real tile, re-encoded with correct lengths: a cut
/// layer, a cut feature or value, or a cut packed tags/geometry field.
fn inner_truncations(data: &[u8], rng: &mut Rng) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let Some(tile) = split_fields(data) else { return out };
    let layers: Vec<usize> = (0..tile.len()).filter(|&i| tile[i].tag == 3 && tile[i].payload.is_some()).collect();
    for _ in 0..4 {
        if layers.is_empty() {
            break;
        }
        let li = layers[rng.below(layers.len() as u64) as usize];
        let layer = tile[li].payload.unwrap();
        match rng.below(3) {
            0 => {
                let cut = rng.below(layer.len() as u64 + 1) as usize;
                out.push(replace_field(&tile, li, &layer[..cut]));
            }
            n => {
                let Some(lf) = split_fields(layer) else { continue };
                let want = if n == 1 { [2u32, 4] } else { [2, 2] };
                let cands: Vec<usize> =
                    (0..lf.len()).filter(|&i| want.contains(&lf[i].tag) && lf[i].payload.is_some()).collect();
                if cands.is_empty() {
                    continue;
                }
                let fi = cands[rng.below(cands.len() as u64) as usize];
                let field = lf[fi].payload.unwrap();
                let new_field = if n == 1 {
                    let cut = rng.below(field.len() as u64 + 1) as usize;
                    field[..cut].to_vec()
                } else {
                    // Cut the packed tags or geometry of a feature.
                    let Some(ff) = split_fields(field) else { continue };
                    let packed: Vec<usize> = (0..ff.len())
                        .filter(|&i| (ff[i].tag == 2 || ff[i].tag == 4) && ff[i].payload.is_some())
                        .collect();
                    if packed.is_empty() {
                        continue;
                    }
                    let pi = packed[rng.below(packed.len() as u64) as usize];
                    let p = ff[pi].payload.unwrap();
                    let cut = rng.below(p.len() as u64 + 1) as usize;
                    replace_field(&ff, pi, &p[..cut])
                };
                let new_layer = replace_field(&lf, fi, &new_field);
                out.push(replace_field(&tile, li, &new_layer));
            }
        }
    }
    out
}

#[test]
fn generated_tiles() {
    let mut totals = Totals::new();
    let mut rng = Rng(42);
    for i in 0..20_000 {
        let tile = gen_tile(&mut rng);
        totals.check(&format!("generated #{i}"), &tile);
    }
    totals.finish("generated tiles");
}

#[test]
fn mutated_fixture_tiles() {
    if !reference_ndebug() {
        eprintln!(
            "skipped: random byte mutations reach protozero's wire-type assertions, which abort the \
             C++ oracle without NDEBUG; run with --config=oracle-opt"
        );
        return;
    }
    let mut totals = Totals::new();
    let mut rng = Rng(99);
    for (name, data) in corpus() {
        if data.first() != Some(&0x1a) || data.len() > 100_000 {
            continue;
        }
        for _ in 0..6 {
            let mut d = data.clone();
            for _ in 0..1 + rng.below(4) {
                let i = rng.below(d.len() as u64) as usize;
                match rng.below(3) {
                    0 => d[i] = rng.next() as u8,
                    1 => d.insert(i, rng.next() as u8),
                    _ => {
                        d.remove(i);
                    }
                }
            }
            totals.check(&format!("{name} (mutated)"), &d);
        }
    }
    totals.finish("mutated tiles");
}

#[test]
fn edge_cases() {
    let mut totals = Totals::new();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("one byte", vec![0x1a]),
        ("empty layer", vec![0x1a, 0x00]),
        ("name only", vec![0x1a, 0x03, 0x0a, 0x01, b'a']),
        ("tag 0", vec![0x00]),
        ("wire type 7", vec![0x0f]),
        ("10-byte key", vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]),
        ("11-byte key", vec![0xff; 11]),
        ("length past end", vec![0x1a, 0x7f, 0x0a]),
        ("length > 2^32", vec![0x1a, 0x81, 0x80, 0x80, 0x80, 0x10, 0x0a]),
        ("reserved tag", vec![0xc0, 0xd4, 0x09, 0x00]),
    ];
    for (name, data) in &cases {
        totals.check(name, data);
    }
    // A complete tile with all value types and geometry commands.
    let mut rng = Rng(1);
    let mut layer = Vec::new();
    ld(&mut layer, 1, b"edge");
    vfield(&mut layer, 15, 2);
    vfield(&mut layer, 5, 0); // extent 0: scale is inf
    ld(&mut layer, 3, b"k");
    let v = gen_value(&mut rng);
    ld(&mut layer, 4, &v);
    let mut feature = Vec::new();
    vfield(&mut feature, 3, 1);
    ld(&mut feature, 2, &[0, 0]);
    // MoveTo (0,0), then (1,0): 0 * inf = NaN (x) and inf (y)
    ld(&mut feature, 4, &[9, 0, 0, 9, 2, 0]);
    ld(&mut layer, 2, &feature);
    let mut tile = Vec::new();
    ld(&mut tile, 3, &layer);
    totals.check("extent 0", &tile);
    totals.finish("edge cases");
}
