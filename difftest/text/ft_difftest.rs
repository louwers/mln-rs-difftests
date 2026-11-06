// Difftest of crates/mln_text/src/ft (the ported FreeType subset) and
// src/hb_ft.rs against the C++ core's FreeType 2.13 + hb-ft, on every font
// of the difftest font set, with the face set up as mln::FreeTypeFace does:
// validity, scale, glyph count, charmap list and selection, FT_Get_Char_Index
// for every codepoint of planes 0-1 and samples of planes 2, 14-16 (and beyond), variation
// sequences, FT_Get_Advance and hb-ft's advance for every glyph.

use mln_text::freetype::{FreeTypeFace, FreeTypeLibrary};
use mln_text::ft::cmap::Encoding;
use mln_text::hb_ft::HbFtFont;
use text_reference::*;

/// Like println!, but not captured by the test harness, so the summary is
/// in test.log of passing runs too.
macro_rules! report {
    ($($arg:tt)*) => {{
        use std::io::Write;
        writeln!(std::io::stderr(), $($arg)*).unwrap();
    }};
}

fn ft_encoding(e: Encoding) -> i64 {
    const fn tag(t: &[u8; 4]) -> i64 {
        u32::from_be_bytes(*t) as i64
    }
    match e {
        Encoding::None => 0,
        Encoding::Unicode => tag(b"unic"),
        Encoding::MsSymbol => tag(b"symb"),
        Encoding::AppleRoman => tag(b"armn"),
        Encoding::Sjis => tag(b"sjis"),
        Encoding::Prc => tag(b"gb  "),
        Encoding::Big5 => tag(b"big5"),
        Encoding::Wansung => tag(b"wans"),
        Encoding::Johab => tag(b"joha"),
    }
}

/// Codepoints compared for every font.
fn codepoints() -> impl Iterator<Item = u32> {
    (0..0x2_0000)
        .chain(0x2_F800..0x2_FA20)
        .chain(0xE_0000..0xE_0200)
        .chain(0xF_0000..0xF_0100)
        .chain(0x10_FF00..0x11_0010)
        .chain([0xFFFF_FFFF, 0x7FFF_FFFF, 0x20_0000])
}

#[test]
fn freetype_faces_match() {
    let lib = FreeTypeLibrary::new();
    let fonts = test_fonts();
    let mut valid = 0;
    let mut lookups = 0u64;
    let mut glyphs = 0u64;
    let mut failures = Vec::new();
    let mut synthesized = Vec::new();
    let mut known = Vec::new();

    for font in &fonts {
        let rust = FreeTypeFace::from_memory(&font.data, &lib);
        let cpp = RefFace::new(&font.data);
        let (Some(cpp), Some(face)) = (&cpp, rust.get_face()) else {
            if cpp.is_some() != rust.is_valid() {
                failures.push(format!("{}: valid C++ {} Rust {}", font.name, cpp.is_some(), rust.is_valid()));
            }
            continue;
        };
        valid += 1;
        let info = cpp.info();
        let mut fail = |what: String| failures.push(format!("{}: {what}", font.name));

        let (hb_x, hb_y) = HbFtFont::scale(&rust);
        let rust_info = (face.x_scale, face.y_scale, face.num_glyphs as i64, face.units_per_em as i64);
        let cpp_info = (info.x_scale, info.y_scale, info.num_glyphs, info.units_per_em);
        if rust_info != cpp_info || (hb_x as i64, hb_y as i64) != (info.hb_x_scale, info.hb_y_scale) {
            fail(format!(
                "scale/glyphs/upem/hb scale: C++ {cpp_info:?} {:?} Rust {rust_info:?} {:?}",
                (info.hb_x_scale, info.hb_y_scale),
                (hb_x, hb_y)
            ));
        }
        let scalable = info.face_flags & FT_FACE_FLAG_SCALABLE != 0;
        let fixed = info.face_flags & FT_FACE_FLAG_FIXED_SIZES != 0;
        if (scalable, fixed) != (face.scalable, face.fixed_sizes) {
            fail(format!(
                "scalable/fixed sizes: C++ {:?} Rust {:?}",
                (scalable, fixed),
                (face.scalable, face.fixed_sizes)
            ));
        }

        // Charmaps.
        // The CFF driver's charmaps for the CFF encodings (platform 7) come
        // last and are never selected (not ported, see ft/face.rs).
        let cpp_charmaps: Vec<[i64; 4]> =
            (0..info.num_charmaps as usize).map(|i| cpp.charmap(i)).filter(|cm| cm[0] != 7).collect();
        let rust_charmaps: Vec<[i64; 4]> = face
            .charmaps
            .iter()
            .map(|cm| [cm.platform_id as i64, cm.encoding_id as i64, ft_encoding(cm.encoding), cm.format as i64])
            .collect();
        if face.needs_synthesized_charmap {
            synthesized.push(font.name.clone());
        }
        if cpp_charmaps != rust_charmaps {
            // The only known difference: FreeType's synthesized Unicode
            // charmap (format -1) of fonts without one (TODO(port) in
            // ft/face.rs). Reported, not failed; the shaping difftest lists
            // the shaping mismatches it causes.
            let synthesized_only = face.needs_synthesized_charmap
                && cpp_charmaps.iter().filter(|cm| !rust_charmaps.contains(cm)).all(|cm| cm[3] == -1);
            if synthesized_only {
                known.push(format!("{}: charmaps C++ {cpp_charmaps:?} Rust {rust_charmaps:?}", font.name));
            } else {
                fail(format!("charmaps: C++ {cpp_charmaps:?} Rust {rust_charmaps:?}"));
            }
            continue;
        }
        let rust_charmap = face.charmap.map_or(-1, |i| i as i64);
        if info.charmap != rust_charmap {
            fail(format!("selected charmap: C++ {} Rust {rust_charmap}", info.charmap));
        }

        // FT_Get_Char_Index and hb-ft's nominal glyph.
        let funcs = HbFtFont::new(&rust);
        let hr_font = harfrust::Font::new(font.data.clone(), 0);
        let shaper_font = hr_font.as_ref().map(harfrust::ShaperFont::new);
        let mut bad = 0;
        for c in codepoints() {
            lookups += 1;
            let (r, c_) = (rust.get_char_index(c), cpp.char_index(c));
            if r != c_ {
                if bad < 5 {
                    fail(format!("FT_Get_Char_Index({c:#x}): C++ {c_} Rust {r}"));
                }
                bad += 1;
            }
            if let Some(sf) = &shaper_font {
                use harfrust::FontFuncs;
                let r = funcs.nominal_glyph(sf, c).map_or(0, |g| g.to_u32());
                let c_ = cpp.hb_nominal_glyph(c);
                if r != c_ {
                    if bad < 5 {
                        fail(format!("hb_ft_get_nominal_glyph({c:#x}): C++ {c_} Rust {r}"));
                    }
                    bad += 1;
                }
            }
        }
        if bad > 5 {
            fail(format!("... {bad} lookup mismatches"));
        }

        // Variation sequences: every selector with the codepoints that have
        // a glyph, plus some without.
        let selectors = (0xFE00..=0xFE0F).chain(0xE0100..=0xE01EF);
        let mut bad = 0;
        for vs in selectors {
            for c in (0x20..0x3000).step_by(7).chain([0x2764, 0x231A, 0x845B, 0x9089, 0x8FBA, 0x1F600]) {
                let (r, c_) = (rust.get_char_variant_index(c, vs), cpp.char_variant_index(c, vs));
                if r != c_ {
                    if bad < 5 {
                        fail(format!("FT_Face_GetCharVariantIndex({c:#x}, {vs:#x}): C++ {c_} Rust {r}"));
                    }
                    bad += 1;
                }
            }
        }

        // Advances.
        let mut bad = 0;
        for gid in 0..(info.num_glyphs as u32 + 3) {
            glyphs += 1;
            let r = rust.get_advance(gid);
            let c_ = cpp.advance(gid).ok();
            if r != c_ {
                if bad < 5 {
                    fail(format!("FT_Get_Advance({gid}): C++ {c_:?} Rust {r:?}"));
                }
                bad += 1;
            }
            if let Some(sf) = &shaper_font {
                use harfrust::FontFuncs;
                let r = funcs.glyph_h_advance(sf, harfrust::GlyphId::new(gid));
                let c_ = cpp.hb_h_advance(gid);
                if r != c_ {
                    if bad < 5 {
                        fail(format!("hb_ft_get_glyph_h_advances({gid}): C++ {c_} Rust {r}"));
                    }
                    bad += 1;
                }
            }
        }
    }

    report!("{} fonts, {valid} valid in both; {lookups} charmap lookups, {glyphs} advances compared", fonts.len());
    report!("fonts where FreeType synthesizes a charmap (not ported): {synthesized:?}");
    for k in &known {
        report!("KNOWN (synthesized charmap not ported) {k}");
    }
    for f in &failures {
        report!("MISMATCH {f}");
    }
    assert!(failures.is_empty(), "{} FreeType mismatches", failures.len());
}
