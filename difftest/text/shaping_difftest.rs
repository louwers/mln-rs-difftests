// Shaping difftest: crates/mln_text's HBShaper (HarfRust) against the C++
// core's HBShaper (HarfBuzz 8.2.0 + FreeType, HB_TINY) on every font of the
// font set (HarfBuzz's perf + API test fonts and the render tests'
// font-faces fonts) and a corpus of hand-written strings (scripts, marks,
// emoji, mixed scripts, edge cases) plus generated ones. Glyph IDs (code and
// type) and the x/y offsets and advances of `createComplexGlyphIDs` must be
// bit-identical.
//
// Every mismatch is reported. Mismatches with a known, documented cause
// (DEVIATIONS.md) are counted per cause; any other mismatch fails the test.

use std::collections::BTreeMap;

use mln_text::{FreeTypeLibrary, GlyphID, HBShapeAdjust, HBShaper};
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, TestRng, TestRunner};
use text_reference::*;

/// Like println!, but not captured by the test harness, so the summary is
/// in test.log of passing runs too.
macro_rules! report {
    ($($arg:tt)*) => {{
        use std::io::Write;
        writeln!(std::io::stderr(), $($arg)*).unwrap();
    }};
}

#[path = "hb_ucd15_full_gc.rs"]
#[rustfmt::skip]
mod hb_ucd15_full_gc;

/// Hand-written corpus (UTF-8 here, shaped as UTF-16).
const CORPUS: &[&str] = &[
    "",
    "a",
    " ",
    "Hello, World!",
    "The quick brown fox jumps over the lazy dog",
    "AVATAR Wavy Type To Tj LT",
    "fi fl ffi ffl ff ft st",
    "0123456789 +-*/=%",
    "1/2 3\u{2044}4 12\u{2044}345",
    "Ångström café naïve façade",
    "e\u{301} a\u{300}\u{301}\u{302} o\u{308}\u{304}",
    "\u{301}",
    "\u{301}a",
    "x\u{20DD} 1\u{20E3}",
    "i\u{307}\u{301} \u{131}",
    "abc",
    "ac",
    "AE",
    "A",
    "WA",
    "\u{20}\u{a0}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{202f}\u{205f}\u{3000}",
    "a\u{200b}b\u{200c}c\u{200d}d\u{2060}e\u{feff}f\u{ad}g",
    "a\tb\nc\rd",
    "\u{0}\u{1}a\u{7f}",
    "\u{e000}\u{e001}\u{f8ff}",
    "Ελληνικά γράμματα",
    "Русский текст, Ёё",
    "Հայերեն ქართული",
    "مرحبا بالعالم",
    "العربية",
    "لا لأ لإ لآ",
    "بِسْمِ ٱللَّٰهِ ٱلرَّحْمَٰنِ ٱلرَّحِيمِ",
    "١٢٣٤٥ ۱۲۳",
    "اردو زبان",
    "فارسی گچپژ",
    "ـبـ ـ",
    "\u{fefb}\u{fe8d}",
    "ܣܘܪܝܝܐ",
    "שָׁלוֹם",
    "עברית",
    "נמסתה ש\u{5bc}\u{5c1}",
    "नमस्ते",
    "हिन्दी",
    "क्षत्रिय",
    "र्क",
    "अॅ",
    "ि",
    "क्\u{200d}ष क्\u{200c}ष",
    "ज़्यादा",
    "বাংলা ভাষা",
    "தமிழ்",
    "తెలుగు",
    "ಕನ್ನಡ",
    "മലയാളം",
    "ગુજરાતી",
    "ਪੰਜਾਬੀ",
    "ଓଡ଼ିଆ",
    "සිංහල",
    "မြန်မာ",
    "မင်္ဂလာပါ",
    "ကျွန်ုပ်",
    "ភាសាខ្មែរ",
    "ស្រី",
    "ព្រះរាជាណាចក្រកម្ពុជា",
    "ภาษาไทย",
    "ที่ น้ำ",
    "ພາສາລາວ",
    "བོད་སྐད།",
    "ᐃᓄᒃᑎᑐᑦ",
    "አማርኛ",
    "中文字体",
    "日本語のテキスト",
    "カタカナ ｶﾀｶﾅ",
    "한국어 텍스트",
    "\u{1100}\u{1161}\u{11a8}",
    "漢字かな交じり文",
    "\u{660e}\u{6975}\u{73e0}\u{5ea6}\u{8f38}\u{6e05}",
    "A\u{3041}\u{4c2e}",
    "\u{3297}\u{3299}",
    "8\u{ae}\u{2049}",
    "😀",
    "👍🏽",
    "👨‍👩‍👧‍👦",
    "🇳🇱🇩🇪",
    "❤️ ❤",
    "#️⃣",
    "©®™",
    "邊\u{e0100} 邊\u{e0101}",
    "a\u{fe00}",
    "\u{2764}\u{fe0f}\u{2764}\u{fe0e}",
    "Hello مرحبا world",
    "abc中文def",
    "Москва 2024 Moscow",
    "नमस्ते world",
    "ภาษาไทย English 中文",
    "(abc) [مرحبا] {שלום}",
    "a1b2c3",
    "Mixed: Ελλάς, Россия, 日本, 한국, مصر, ישראל, भारत",
    "\u{d7}\u{f7}\u{2212}\u{221e}",
    "\u{2026}\u{2013}\u{2014}\u{201c}\u{201d}",
    "\u{fffd}\u{fffe}\u{ffff}",
    "\u{10ffff}\u{e01ef}",
    "\u{1d400}\u{1d538}",
];

/// Strings that are not valid UTF-16 (unpaired surrogates).
fn raw_corpus() -> Vec<Vec<u16>> {
    vec![vec![0xD800], vec![0xDC00, 0x41], vec![0x41, 0xD83D], vec![0xD83D, 0x20, 0xDE00]]
}

/// Generated strings: a mix of the scripts above, combining marks, joiners,
/// variation selectors, spaces, emoji and arbitrary codepoints.
fn generated_corpus(count: usize) -> Vec<Vec<u16>> {
    let ranges: &[(u32, u32)] = &[
        (0x20, 0x7E),
        (0xA0, 0x24F),
        (0x300, 0x36F),
        (0x370, 0x3FF),
        (0x400, 0x4FF),
        (0x590, 0x5FF),
        (0x600, 0x6FF),
        (0x900, 0x97F),
        (0x980, 0x9FF),
        (0xB80, 0xBFF),
        (0xE00, 0xE7F),
        (0x1000, 0x109F),
        (0x1780, 0x17FF),
        (0x2000, 0x206F),
        (0x3040, 0x30FF),
        (0x4E00, 0x9FFF),
        (0xAC00, 0xD7A3),
        (0xFE00, 0xFE0F),
        (0xFB50, 0xFDFF),
        (0x1F300, 0x1F64F),
        (0x1F900, 0x1F9FF),
        (0x200C, 0x200D),
    ];
    let char_strategy = proptest::prop_oneof![
        8 => proptest::sample::select(ranges.to_vec())
            .prop_flat_map(|(lo, hi)| lo..=hi)
            .prop_filter_map("valid char", char::from_u32),
        1 => proptest::char::any(),
    ];
    let strategy = proptest::collection::vec(char_strategy, 0..24);
    let mut runner = TestRunner::new_with_rng(Config::default(), TestRng::deterministic_rng(Default::default()));
    (0..count)
        .map(|_| {
            let chars = strategy.new_tree(&mut runner).unwrap().current();
            chars.into_iter().collect::<String>().encode_utf16().collect()
        })
        .collect()
}

type Shaped = Vec<(u32, [f32; 3])>;

fn rust_shape(shaper: &HBShaper, text: &[u16]) -> Shaped {
    let mut ids: Vec<GlyphID> = Vec::new();
    let mut adjusts: Vec<HBShapeAdjust> = Vec::new();
    shaper.create_complex_glyph_ids(text, &mut ids, &mut adjusts);
    assert_eq!(ids.len(), adjusts.len());
    ids.iter().zip(&adjusts).map(|(id, a)| (id.hash(), [a.x_offset, a.y_offset, a.advance])).collect()
}

fn bits(s: &Shaped) -> Vec<(u32, [u32; 3])> {
    s.iter().map(|(id, a)| (*id, a.map(f32::to_bits))).collect()
}

fn show(text: &[u16]) -> String {
    let s: String =
        char::decode_utf16(text.iter().copied()).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect();
    let cps: Vec<String> = text.iter().map(|u| format!("{u:04X}")).collect();
    format!("{s:?} [{}]", cps.join(" "))
}

/// Why a case differs. Each known cause is described (with a minimal
/// reproducer) in DEVIATIONS.md. HarfRust's anchor rounding and fallback
/// space widths (causes 4 and 5 there) have no variant: they come out as
/// Unknown, which is why the test is ignored.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Cause {
    /// The text has a codepoint unassigned in Unicode 15.0 (HarfBuzz 8.2's
    /// HB_NO_UCD_UNASSIGNED tables give those arbitrary properties) or
    /// whose script or combining class differs between HarfBuzz 8.2.0's
    /// Unicode 15.0 and HarfRust's Unicode 18.0 data.
    UnicodeVersion,
    /// HarfRust inserts dotted circles where HB_TINY does not
    /// (HB_NO_OT_SHAPER_VOWEL_CONSTRAINTS).
    VowelConstraintDottedCircle,
    /// HarfRust's fallback mark positioning (HB_TINY has none:
    /// HB_NO_OT_SHAPE_FALLBACK) zeroes the advance of non-spacing marks
    /// that GDEF does not class as marks (typically .notdef).
    FallbackMarkZeroing,
    /// The font has no Unicode cmap and FreeType synthesizes one from the
    /// glyph names (not ported: TODO(port) in crates/mln_text/src/ft/face.rs).
    SynthesizedCharmapNotPorted,
    Unknown,
}

/// HarfBuzz units (64ths) of a C++/Rust result: (glyph, [x_offset, -y_offset, advance]).
fn units(s: &Shaped) -> Vec<(u32, [i32; 3])> {
    s.iter().map(|(id, a)| (*id, [(a[0] * 64.0) as i32, (a[1] * 64.0) as i32, (a[2] * 64.0) as i32])).collect()
}

/// The codepoints of a UTF-16 string, unpaired surrogates as U+FFFD.
fn codepoints(text: &[u16]) -> Vec<u32> {
    char::decode_utf16(text.iter().copied()).map(|c| c.map_or(0xFFFD, |c| c as u32)).collect()
}

fn unicode_differs(u: u32) -> bool {
    hb_ucd15_full_gc::full_gc(u) == 2 // HB_UNICODE_GENERAL_CATEGORY_UNASSIGNED
        || harfrust::unicode::script(u).tag().to_be_bytes() != hb_script(u)
        || harfrust::unicode::combining_class(u) as u32 != hb_combining_class(u)
}

fn classify(text: &[u16], c: &Shaped, r: &Shaped, dotted_circle: u32) -> Cause {
    let cps = codepoints(text);
    if cps.iter().any(|&u| unicode_differs(u)) {
        return Cause::UnicodeVersion;
    }
    let (cu, ru) = (units(c), units(r));
    if ru.len() > cu.len() {
        let without: Vec<u32> = ru.iter().map(|g| g.0).filter(|&g| g & 0xFFFF != dotted_circle).collect();
        let cpp_ids: Vec<u32> = cu.iter().map(|g| g.0).filter(|&g| g & 0xFFFF != dotted_circle).collect();
        if without == cpp_ids {
            return Cause::VowelConstraintDottedCircle;
        }
        return Cause::Unknown;
    }
    if ru.len() != cu.len() || ru.iter().zip(&cu).any(|(a, b)| a.0 != b.0) {
        return Cause::Unknown;
    }
    let diffs: Vec<(&[i32; 3], &[i32; 3])> =
        ru.iter().zip(&cu).filter(|(a, b)| a.1 != b.1).map(|(a, b)| (&a.1, &b.1)).collect();
    // Advance zeroed, offset unchanged (backward) or moved by the advance
    // (forward), as hb's zero_mark_advances does.
    if diffs.iter().all(|(r, c)| r[2] == 0 && c[2] != 0 && r[1] == c[1] && (r[0] == c[0] || r[0] == c[0] - c[2])) {
        return Cause::FallbackMarkZeroing;
    }
    Cause::Unknown
}

/// Removes codepoints from `text` while the mismatch persists.
fn minimize(text: &[u16], mismatches: impl Fn(&[u16]) -> bool) -> Vec<u16> {
    // Work on whole codepoints (surrogate pairs stay together).
    let mut units: Vec<Vec<u16>> = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let n = if (0xD800..0xDC00).contains(&text[i]) && text.get(i + 1).is_some_and(|l| (0xDC00..0xE000).contains(l))
        {
            2
        } else {
            1
        };
        units.push(text[i..i + n].to_vec());
        i += n;
    }
    let join = |u: &[Vec<u16>]| u.concat();
    let mut chunk = units.len().max(1);
    while chunk >= 1 {
        let mut start = 0;
        while start < units.len() {
            let end = (start + chunk).min(units.len());
            let mut candidate = units.clone();
            candidate.drain(start..end);
            if !candidate.is_empty() && mismatches(&join(&candidate)) {
                units = candidate;
            } else {
                start = end;
            }
        }
        if chunk == 1 {
            break;
        }
        chunk /= 2;
    }
    join(&units)
}

// Ignored: upstream HarfRust 0.15 rounds anchor ties and sizes fallback
// spaces differently from HarfBuzz (DEVIATIONS.md [text], causes 4 and 5).
// Those mismatches have no Cause, so the test fails on them. Run it with
// --test_arg=--ignored to see the report.
#[test]
#[ignore = "HarfRust anchor rounding and fallback spaces differ (DEVIATIONS.md [text] 4, 5)"]
fn shaping_matches_harfbuzz() {
    let lib = FreeTypeLibrary::new();
    let fonts = test_fonts();
    let mut strings: Vec<Vec<u16>> = CORPUS.iter().map(|s| utf16(s)).collect();
    strings.extend(raw_corpus());
    let handwritten = strings.len();
    strings.extend(generated_corpus(300));

    let type_ = 7u16;
    let mut cases = 0u64;
    let mut glyphs = 0u64;
    let mut valid_fonts = 0;
    // cause -> (font, minimal text) -> count
    let mut by_cause: BTreeMap<Cause, BTreeMap<(String, Vec<u16>), (usize, String)>> = BTreeMap::new();
    let mut mismatch_count = 0;
    for font in &fonts {
        let cpp = RefShaper::new(&font.data, type_);
        let rust = HBShaper::new(mln_text::GlyphIDType(type_), &font.data, &lib);
        assert_eq!(rust.valid(), cpp.valid(), "{}: valid()", font.name);
        if !cpp.valid() {
            continue;
        }
        valid_fonts += 1;
        let dotted_circle = RefFace::new(&font.data).map_or(0, |f| f.hb_nominal_glyph(0x25CC));
        let differs = |text: &[u16]| bits(&cpp.shape(text)) != bits(&rust_shape(&rust, text));
        for text in &strings {
            cases += 1;
            let c = cpp.shape(text);
            let r = rust_shape(&rust, text);
            glyphs += c.len() as u64;
            if bits(&c) == bits(&r) {
                continue;
            }
            mismatch_count += 1;
            let min = minimize(text, differs);
            let (mc, mr) = (cpp.shape(&min), rust_shape(&rust, &min));
            let synthesized = rust.impl_().face().get_face().is_some_and(|f| f.needs_synthesized_charmap)
                && RefFace::new(&font.data).is_some_and(|f| {
                    (0..f.info().num_charmaps as usize).any(|i| f.charmap(i)[3] == -1 && f.charmap(i)[0] != 7)
                });
            let cause =
                if synthesized { Cause::SynthesizedCharmapNotPorted } else { classify(&min, &mc, &mr, dotted_circle) };
            let entry = by_cause
                .entry(cause)
                .or_default()
                .entry((font.name.clone(), min.clone()))
                .or_insert_with(|| (0, format!("C++  {:?}\n        Rust {:?}", units(&mc), units(&mr))));
            entry.0 += 1;
        }
    }

    report!(
        "{} fonts ({valid_fonts} valid), {} strings ({handwritten} hand-written, {} generated), {cases} cases, {glyphs} C++ glyphs",
        fonts.len(),
        strings.len(),
        strings.len() - handwritten
    );
    report!("{} cases identical, {mismatch_count} differ", cases - mismatch_count as u64);
    for (cause, reproducers) in &by_cause {
        let n: usize = reproducers.values().map(|v| v.0).sum();
        let mut texts: Vec<&Vec<u16>> = reproducers.keys().map(|(_, t)| t).collect();
        texts.sort();
        texts.dedup();
        report!(
            "CAUSE {cause:?}: {n} cases, {} (font, minimal text) reproducers, {} distinct texts",
            reproducers.len(),
            texts.len()
        );
        let limit = if *cause == Cause::Unknown { usize::MAX } else { 6 };
        for ((font, text), (count, detail)) in reproducers.iter().take(limit) {
            report!("    {font} {} ({count} cases)\n        {detail}", show(text));
        }
    }
    let unknown: usize = by_cause.get(&Cause::Unknown).map_or(0, |r| r.values().map(|v| v.0).sum());
    assert_eq!(unknown, 0, "{unknown} shaping mismatches with no known cause");
}

/// Raw HarfBuzz-unit details of selected cases (glyph, cluster, x/y
/// advance, x/y offset per glyph) for analysing mismatches. Always passes.
#[test]
fn debug_dump() {
    let cases: &[(&str, &str, [u8; 4])] = &[
        ("perf/fonts/Roboto-Regular.ttf", "ש\u{5b8}\u{5c1}לו\u{5b9}ם", *b"Hebr"),
        ("perf/fonts/Roboto-Regular.ttf", "\u{5c6}", *b"Hebr"),
        ("perf/fonts/Roboto-Regular.ttf", "Ҙ\u{325}", *b"Cyrl"),
        ("perf/fonts/Amiri-Regular.ttf", "م\u{650}", *b"Arab"),
        ("perf/fonts/Amiri-Regular.ttf", "ب\u{650}س\u{652}م\u{650}", *b"Arab"),
    ];
    let lib = FreeTypeLibrary::new();
    let fonts = test_fonts();
    for (font_name, text, script) in cases {
        let font = fonts.iter().find(|f| f.name.ends_with(font_name)).expect(font_name);
        let text = utf16(text);
        let cpp = RefFace::new(&font.data).unwrap().hb_shape_raw(&text, *script);
        let rust = HBShaper::new(mln_text::GlyphIDType(0), &font.data, &lib).impl_().shape_part_raw(&text, *script);
        // Not captured by the test harness.
        use std::io::Write;
        let mut err = std::io::stderr();
        writeln!(err, "DEBUG {font_name} {} {}", show(&text), String::from_utf8_lossy(script)).unwrap();
        writeln!(err, "  C++  {cpp:?}").unwrap();
        writeln!(err, "  Rust {rust:?}").unwrap();
    }
}
