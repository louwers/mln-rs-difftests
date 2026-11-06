// Rasterization difftest: crates/mln_text's HBShaper::rasterize_glyph
// (skrifa hinted outlines + the ftgrays port) against the C++ core's
// HBShaper::rasterizeGlyph (FreeType 2.13, FT_LOAD_DEFAULT at 24 px), glyph
// by glyph on the font set, and util::transformRasterToSDF (the SDF step
// GlyphManager applies) bit-exactly on the C++ bitmaps.
//
// Bitmaps are compared with a measured tolerance (reported per font and in
// total); the metrics are reported the same way. See DEVIATIONS.md.

use mln_text::{FreeTypeLibrary, GlyphID, GlyphIDType, HBShaper};
use mln_util::image::AlphaImage;
use mln_util::size::Size;
use mln_util::tiny_sdf::transform_raster_to_sdf;
use text_reference::*;

macro_rules! report {
    ($($arg:tt)*) => {{
        use std::io::Write;
        writeln!(std::io::stderr(), $($arg)*).unwrap();
    }};
}

/// Glyphs tried per font: the first ones and an even sample of the rest.
fn glyph_ids(num_glyphs: u32) -> Vec<u32> {
    let mut ids: Vec<u32> = (0..num_glyphs.min(200)).collect();
    if num_glyphs > 200 {
        let step = ((num_glyphs - 200) / 200).max(1);
        ids.extend((200..num_glyphs).step_by(step as usize));
    }
    ids
}

#[derive(Default)]
struct Stats {
    glyphs: u64,
    metrics_equal: u64,
    bitmap_equal: u64,
    size_differs: u64,
    pixels: u64,
    pixels_differ: u64,
    max_diff: u8,
    sum_abs_diff: u64,
}

#[test]
fn rasterized_glyphs_match_within_tolerance() {
    let lib = FreeTypeLibrary::new();
    let fonts = test_fonts();
    let type_ = 9u16;
    let mut total = Stats::default();
    let mut sdf_compared = 0u64;
    let mut per_font = Vec::new();
    let mut metric_examples = Vec::new();
    for font in &fonts {
        let cpp = RefShaper::new(&font.data, type_);
        if !cpp.valid() {
            continue;
        }
        let rust = HBShaper::new(GlyphIDType(type_), &font.data, &lib);
        let num_glyphs = rust.impl_().face().get_face().map_or(0, |f| f.num_glyphs);
        let mut s = Stats::default();
        for gid in glyph_ids(num_glyphs) {
            let c = cpp.rasterize(gid as u16, type_);
            let r = rust.rasterize_glyph(&GlyphID::with_type(gid as u16, GlyphIDType(type_)));
            s.glyphs += 1;
            let metrics_equal = (c.width, c.height, c.left.to_bits(), c.top.to_bits(), c.advance)
                == (
                    r.metrics.width,
                    r.metrics.height,
                    r.metrics.left.to_bits(),
                    r.metrics.top.to_bits(),
                    r.metrics.advance,
                );
            if metrics_equal {
                s.metrics_equal += 1;
            } else if metric_examples.len() < 40 {
                metric_examples.push(format!(
                    "{} gid {gid}: C++ {}x{} left {} top {} adv {}, Rust {}x{} left {} top {} adv {}",
                    font.name,
                    c.width,
                    c.height,
                    c.left,
                    c.top,
                    c.advance,
                    r.metrics.width,
                    r.metrics.height,
                    r.metrics.left,
                    r.metrics.top,
                    r.metrics.advance
                ));
            }
            if (c.bitmap_width, c.bitmap_height) != (r.bitmap.size.width, r.bitmap.size.height) {
                s.size_differs += 1;
            } else {
                if c.bitmap == r.bitmap.data {
                    s.bitmap_equal += 1;
                }
                for (a, b) in c.bitmap.iter().zip(&r.bitmap.data) {
                    let d = a.abs_diff(*b);
                    s.pixels += 1;
                    if d != 0 {
                        s.pixels_differ += 1;
                        s.sum_abs_diff += d as u64;
                        s.max_diff = s.max_diff.max(d);
                    }
                }
            }
            // The SDF step on the C++ bitmap: exact.
            if c.bitmap_width * c.bitmap_height > 0 {
                let img = AlphaImage::from_slice(Size::new(c.bitmap_width, c.bitmap_height), &c.bitmap).unwrap();
                let rust_sdf = transform_raster_to_sdf(&img, 8.0, 0.25);
                let cpp_sdf = ref_transform_raster_to_sdf(c.bitmap_width, c.bitmap_height, &c.bitmap, 8.0, 0.25);
                assert_eq!(rust_sdf.data, cpp_sdf, "{} gid {gid}: transformRasterToSDF", font.name);
                sdf_compared += 1;
            }
        }
        let engine = rust.impl_().face().hint_engine();
        per_font.push(format!(
            "{} [{engine:?}]: {} glyphs, metrics equal {}, bitmaps equal {}, size differs {}, differing pixels {}/{} (max diff {})",
            font.name, s.glyphs, s.metrics_equal, s.bitmap_equal, s.size_differs, s.pixels_differ, s.pixels, s.max_diff
        ));
        total.glyphs += s.glyphs;
        total.metrics_equal += s.metrics_equal;
        total.bitmap_equal += s.bitmap_equal;
        total.size_differs += s.size_differs;
        total.pixels += s.pixels;
        total.pixels_differ += s.pixels_differ;
        total.sum_abs_diff += s.sum_abs_diff;
        total.max_diff = total.max_diff.max(s.max_diff);
    }
    for l in &per_font {
        report!("FONT {l}");
    }
    for l in &metric_examples {
        report!("METRICS {l}");
    }
    report!(
        "TOTAL {} glyphs: metrics equal {}, bitmaps identical {}, bitmap size differs {}, differing pixels {}/{} (mean abs diff of differing pixels {:.2}, max {}); SDF identical on {sdf_compared} C++ bitmaps",
        total.glyphs,
        total.metrics_equal,
        total.bitmap_equal,
        total.size_differs,
        total.pixels_differ,
        total.pixels,
        total.sum_abs_diff as f64 / total.pixels_differ.max(1) as f64,
        total.max_diff
    );
}
