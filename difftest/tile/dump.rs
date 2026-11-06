// The Rust half of the dump format of reference.cpp, plus the FFI to the
// C++ functions. Shared by the tile and geometry difftests.

#![allow(dead_code)]

use std::cell::RefCell;
use std::fmt::Write;
use std::os::raw::c_char;
use std::sync::Arc;

use feature::{Identifier, Value};
use mln_tile::{Error, VectorMVTTileData};
use mln_tile_id::{GeometryCollection, GeometryTileFeature, GeometryTileLayer};
use mln_util::logging::{Event, EventSeverity, Log, Observer};

unsafe extern "C" {
    fn mln_tile_ref_ndebug() -> i32;
    fn mln_tile_ref_free(p: *mut c_char);
    fn mln_tile_ref_decode(data: *const u8, size: usize, out_len: *mut usize) -> *mut c_char;
    fn mln_tile_ref_fixup_polygons(
        coords: *const i16,
        ring_lens: *const u32,
        nrings: usize,
        out_len: *mut usize,
    ) -> *mut c_char;
    fn mln_tile_ref_classify_rings(
        coords: *const i16,
        ring_lens: *const u32,
        nrings: usize,
        out_len: *mut usize,
    ) -> *mut c_char;
    fn mln_tile_ref_limit_holes(
        coords: *const i16,
        ring_lens: *const u32,
        nrings: usize,
        max_holes: u32,
        out_len: *mut usize,
    ) -> *mut c_char;
    fn mln_tile_ref_wagyu(
        coords: *const i32,
        ring_lens: *const u32,
        ring_types: *const u8,
        nrings: usize,
        clip_type: u8,
        subject_fill: u8,
        clip_fill: u8,
        reverse: u8,
        out_len: *mut usize,
    ) -> *mut c_char;
    fn mln_tile_ref_convert_geometry(
        type_: u8,
        coords: *const i16,
        ring_lens: *const u32,
        nrings: usize,
        z: u8,
        x: u32,
        y: u32,
        out_len: *mut usize,
    ) -> *mut c_char;
    fn mln_tile_ref_convert_geometry_to_tile(
        kind: u8,
        coords: *const f64,
        ring_lens: *const u32,
        nrings: usize,
        z: u8,
        x: u32,
        y: u32,
        out_len: *mut usize,
    ) -> *mut c_char;
}

fn take(p: *mut c_char, len: usize) -> String {
    // SAFETY: the reference returns a malloc'd buffer of `len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(p as *const u8, len) }.to_vec();
    // SAFETY: allocated by the reference, freed once.
    unsafe { mln_tile_ref_free(p) };
    String::from_utf8(bytes).expect("the dump is ASCII")
}

/// Whether the oracle was built with NDEBUG (no C++ asserts).
pub fn reference_ndebug() -> bool {
    // SAFETY: no arguments.
    unsafe { mln_tile_ref_ndebug() != 0 }
}

pub fn reference_decode(data: &[u8]) -> String {
    let mut len = 0;
    // SAFETY: (ptr, len) of a live slice.
    let p = unsafe { mln_tile_ref_decode(data.as_ptr(), data.len(), &mut len) };
    take(p, len)
}

/// Flattened rings for the FFI.
pub fn flatten<T: Copy>(rings: &[Vec<(T, T)>]) -> (Vec<T>, Vec<u32>) {
    let mut coords = Vec::new();
    let mut lens = Vec::new();
    for r in rings {
        lens.push(r.len() as u32);
        for &(x, y) in r {
            coords.push(x);
            coords.push(y);
        }
    }
    (coords, lens)
}

pub fn reference_fixup_polygons(rings: &[Vec<(i16, i16)>]) -> String {
    let (coords, lens) = flatten(rings);
    let mut len = 0;
    // SAFETY: arrays of the advertised sizes.
    let p = unsafe { mln_tile_ref_fixup_polygons(coords.as_ptr(), lens.as_ptr(), lens.len(), &mut len) };
    take(p, len)
}

pub fn reference_classify_rings(rings: &[Vec<(i16, i16)>]) -> String {
    let (coords, lens) = flatten(rings);
    let mut len = 0;
    // SAFETY: arrays of the advertised sizes.
    let p = unsafe { mln_tile_ref_classify_rings(coords.as_ptr(), lens.as_ptr(), lens.len(), &mut len) };
    take(p, len)
}

pub fn reference_limit_holes(rings: &[Vec<(i16, i16)>], max_holes: u32) -> String {
    let (coords, lens) = flatten(rings);
    let mut len = 0;
    // SAFETY: arrays of the advertised sizes.
    let p = unsafe { mln_tile_ref_limit_holes(coords.as_ptr(), lens.as_ptr(), lens.len(), max_holes, &mut len) };
    take(p, len)
}

pub fn reference_wagyu(
    rings: &[Vec<(i32, i32)>],
    types: &[u8],
    clip_type: u8,
    subject_fill: u8,
    clip_fill: u8,
    reverse: bool,
) -> String {
    let (coords, lens) = flatten(rings);
    let mut len = 0;
    // SAFETY: arrays of the advertised sizes.
    let p = unsafe {
        mln_tile_ref_wagyu(
            coords.as_ptr(),
            lens.as_ptr(),
            types.as_ptr(),
            lens.len(),
            clip_type,
            subject_fill,
            clip_fill,
            u8::from(reverse),
            &mut len,
        )
    };
    take(p, len)
}

pub fn reference_convert_geometry(type_: u8, rings: &[Vec<(i16, i16)>], z: u8, x: u32, y: u32) -> String {
    let (coords, lens) = flatten(rings);
    let mut len = 0;
    // SAFETY: arrays of the advertised sizes.
    let p =
        unsafe { mln_tile_ref_convert_geometry(type_, coords.as_ptr(), lens.as_ptr(), lens.len(), z, x, y, &mut len) };
    take(p, len)
}

pub fn reference_convert_geometry_to_tile(kind: u8, rings: &[Vec<(f64, f64)>], z: u8, x: u32, y: u32) -> String {
    let (coords, lens) = flatten(rings);
    let mut len = 0;
    // SAFETY: arrays of the advertised sizes.
    let p = unsafe {
        mln_tile_ref_convert_geometry_to_tile(kind, coords.as_ptr(), lens.as_ptr(), lens.len(), z, x, y, &mut len)
    };
    take(p, len)
}

// ---- the Rust dump -------------------------------------------------------

pub fn hex(out: &mut String, s: &[u8]) {
    out.push('x');
    for b in s {
        let _ = write!(out, "{b:02x}");
    }
}

pub fn bits(out: &mut String, d: f64) {
    let _ = write!(out, "{:016x}", d.to_bits());
}

pub fn value(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push('n'),
        Value::Bool(b) => out.push_str(if *b { "b1" } else { "b0" }),
        Value::Uint(u) => {
            let _ = write!(out, "u{u}");
        }
        Value::Int(i) => {
            let _ = write!(out, "i{i}");
        }
        Value::Double(d) => {
            out.push('d');
            bits(out, *d);
        }
        Value::String(s) => {
            out.push('s');
            hex(out, s.as_bytes());
        }
        _ => out.push('?'),
    }
}

fn identifier(out: &mut String, id: &Identifier) {
    match id {
        Identifier::Null => out.push('n'),
        Identifier::Uint(u) => {
            let _ = write!(out, "u{u}");
        }
        Identifier::Int(i) => {
            let _ = write!(out, "i{i}");
        }
        Identifier::Double(d) => {
            out.push('d');
            bits(out, *d);
        }
        Identifier::String(s) => {
            out.push('s');
            hex(out, s.as_bytes());
        }
    }
}

pub fn rings(out: &mut String, g: &[Vec<geometry::Point<i16>>]) {
    let _ = writeln!(out, "{}", g.len());
    for r in g {
        let _ = write!(out, "r {}", r.len());
        for p in r {
            let _ = write!(out, " {} {}", p.x, p.y);
        }
        out.push('\n');
    }
}

pub fn exception(out: &mut String, prefix: &str, e: &Error) {
    out.push_str(prefix);
    out.push_str("! ");
    out.push_str(match e {
        Error::Pbf(_) => "pbf ",
        Error::OutOfRange(_) => "out_of_range ",
        Error::Runtime(_) => "runtime ",
    });
    hex(out, e.what().as_bytes());
    out.push('\n');
}

thread_local! {
    static TILE_LOG: RefCell<String> = const { RefCell::new(String::new()) };
}

struct Capture;

impl Observer for Capture {
    fn on_record(&self, severity: EventSeverity, event: Event, _code: i64, msg: &str) -> bool {
        if event == Event::ParseTile {
            TILE_LOG.with(|l| {
                let mut l = l.borrow_mut();
                let _ = write!(l, "log {} ", severity as u8);
                hex(&mut l, msg.as_bytes());
                l.push('\n');
            });
            return true;
        }
        false
    }
}

const EXTRA_KEYS: [&str; 5] = ["name", "type", "class", "zzz-missing", ""];

fn dump_feature(
    out: &mut String,
    layer: &mln_tile::VectorMVTTileLayer<'_>,
    raw: Option<&vector_tile::Layer<'_>>,
    i: usize,
) {
    let _ = writeln!(out, "f {i}");
    let feature = match layer.try_get_feature(i) {
        Ok(f) => f,
        Err(e) => {
            exception(out, "f", &e);
            return;
        }
    };
    let _ = writeln!(out, "type {}", feature.get_type() as u8);
    out.push_str("id ");
    identifier(out, &feature.get_id());
    out.push('\n');

    let mut keys: Vec<String> = Vec::new();
    match feature.try_get_properties() {
        Ok(props) => {
            let _ = writeln!(out, "props {}", props.len());
            for (k, v) in props {
                out.push_str("p ");
                hex(out, k.as_bytes());
                out.push(' ');
                value(out, v);
                out.push('\n');
                keys.push(k.clone());
            }
        }
        Err(e) => exception(out, "props", &e),
    }
    keys.extend(EXTRA_KEYS.iter().map(|k| k.to_string()));
    for k in &keys {
        match feature.try_get_value(k) {
            Ok(v) => {
                out.push_str("v ");
                hex(out, k.as_bytes());
                out.push(' ');
                match v {
                    Some(v) => value(out, &v),
                    None => out.push_str("none"),
                }
                out.push('\n');
            }
            Err(e) => {
                let mut prefix = String::from("v ");
                hex(&mut prefix, k.as_bytes());
                exception(out, &prefix, &e);
            }
        }
    }

    if let Some(raw) = raw {
        let r = raw
            .get_feature(i)
            .and_then(|view| vector_tile::Feature::new(view, raw))
            .and_then(|f| f.get_geometries::<i16>(1.0));
        match r {
            Ok(g) => {
                out.push_str("raw_geom ");
                rings(out, &g);
            }
            Err(e) => exception(out, "raw_geom", &e),
        }
    }

    TILE_LOG.with(|l| l.borrow_mut().clear());
    let previous = Log::set_thread_observer(Some(Box::new(Capture)));
    let r = feature.try_get_geometries().map(|g: &GeometryCollection| g.clone());
    Log::set_thread_observer(previous);
    TILE_LOG.with(|l| out.push_str(&l.borrow()));
    match r {
        Ok(g) => {
            out.push_str("geom ");
            rings(out, &g);
        }
        Err(e) => {
            exception(out, "geom", &e);
            // A second call: retried, or the lines cached before fixupPolygons threw.
            match feature.try_get_geometries() {
                Ok(g) => {
                    out.push_str("geom2 ");
                    rings(out, g);
                }
                Err(e) => exception(out, "geom2", &e),
            }
        }
    }
}

/// Counts gathered while dumping (Rust side).
#[derive(Default, Debug, Clone, Copy)]
pub struct DumpStats {
    pub layers: usize,
    pub features: usize,
    pub invalid_utf8_layer_names: usize,
}

/// The Rust port's dump of `data`, as `mln_tile_ref_decode` writes it.
pub fn rust_decode(data: &[u8], stats: &mut DumpStats) -> String {
    let mut out = String::new();
    let tile = VectorMVTTileData::new(Arc::new(data.to_vec()));

    let names = match tile.layer_names() {
        Ok(n) => n,
        Err(e) => {
            exception(&mut out, "names", &e);
            return out;
        }
    };
    let _ = writeln!(out, "names {}", names.len());

    let buffer = match vector_tile::Buffer::new(data) {
        Ok(b) => b,
        Err(e) => {
            exception(&mut out, "raw_buffer", &e);
            vector_tile::Buffer::default()
        }
    };
    let raw_layers = buffer.get_layers();

    // Iterate the names as bytes (C++ order and lookup); a name that is not
    // UTF-8 cannot be looked up through the &str API.
    for (name_bytes, view) in &raw_layers {
        out.push_str("layer ");
        hex(&mut out, name_bytes);
        out.push('\n');
        let Ok(name) = std::str::from_utf8(name_bytes) else {
            stats.invalid_utf8_layer_names += 1;
            out.push_str("invalid-utf8-layer-name\n");
            continue;
        };
        let layer = match tile.try_get_layer(name) {
            Ok(Some(l)) => l,
            Ok(None) => {
                out.push_str("nolayer\n");
                continue;
            }
            Err(e) => {
                exception(&mut out, "layer", &e);
                continue;
            }
        };
        stats.layers += 1;
        out.push_str("info ");
        hex(&mut out, layer.get_name().as_bytes());
        let _ = writeln!(out, " {}", layer.feature_count());

        let raw = match vector_tile::Layer::new(view) {
            Ok(l) => {
                let _ = writeln!(out, "raw {} {}", l.get_extent(), l.get_version());
                Some(l)
            }
            Err(e) => {
                exception(&mut out, "raw", &e);
                None
            }
        };

        for i in 0..=layer.feature_count() {
            dump_feature(&mut out, &layer, raw.as_ref(), i);
        }
        stats.features += layer.feature_count();
    }

    match tile.try_get_layer("zzz-missing-layer") {
        Ok(Some(_)) => out.push_str("missing found\n"),
        Ok(None) => out.push_str("missing none\n"),
        Err(e) => exception(&mut out, "missing", &e),
    }
    out
}

/// Replaces every `x<hex>` token by the hex of its UTF-8-lossy decoding.
/// Returns the normalized dump and whether any token changed.
pub fn normalize_lossy(dump: &str) -> (String, bool) {
    let mut changed = false;
    let mut out = String::with_capacity(dump.len());
    for line in dump.split_inclusive('\n') {
        let mut first = true;
        for token in line.trim_end_matches('\n').split(' ') {
            if !first {
                out.push(' ');
            }
            first = false;
            // Tokens like `sx..` (string values) carry the hex after a prefix.
            let (prefix, rest) = match token.find('x') {
                Some(i) if i <= 1 && token[i + 1..].bytes().all(|b| b.is_ascii_hexdigit()) => token.split_at(i),
                _ => {
                    out.push_str(token);
                    continue;
                }
            };
            let hexdigits = &rest[1..];
            let bytes: Vec<u8> = (0..hexdigits.len() / 2)
                .map(|i| u8::from_str_radix(&hexdigits[2 * i..2 * i + 2], 16).unwrap())
                .collect();
            let lossy = String::from_utf8_lossy(&bytes);
            if lossy.as_bytes() != bytes.as_slice() {
                changed = true;
            }
            out.push_str(prefix);
            hex(&mut out, lossy.as_bytes());
        }
        if line.ends_with('\n') {
            out.push('\n');
        }
    }
    (out, changed)
}

/// The first differing line of two dumps, for reports.
pub fn first_difference(a: &str, b: &str) -> String {
    for (i, (la, lb)) in a.lines().zip(b.lines()).enumerate() {
        if la != lb {
            let cut = |s: &str| s.chars().take(300).collect::<String>();
            return format!("line {}:\n  C++:  {}\n  Rust: {}", i + 1, cut(la), cut(lb));
        }
    }
    format!("length differs: C++ {} lines, Rust {} lines", a.lines().count(), b.lines().count())
}
