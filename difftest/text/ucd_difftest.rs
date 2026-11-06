// Difftest of crates/mln_text/src/hb_ucd.rs (the HB_TINY Unicode 15.0 tables
// of the C++ core's HarfBuzz) against hb_unicode_general_category /
// hb_unicode_script of the C++ core, for every codepoint and beyond; and a
// report of where HarfRust's own Unicode data (18.0) differs from them.

use mln_text::hb_ucd;
use text_reference::*;

/// Like println!, but not captured by the test harness, so the summary is
/// in test.log of passing runs too.
macro_rules! report {
    ($($arg:tt)*) => {{
        use std::io::Write;
        writeln!(std::io::stderr(), $($arg)*).unwrap();
    }};
}

#[test]
fn general_category_and_script_every_codepoint() {
    let mut n = 0;
    for u in (0..=0x11_0010u32).chain([0x7FFF_FFFF, 0xFFFF_FFFF]) {
        assert_eq!(hb_ucd::general_category(u) as u32, hb_general_category(u), "gc({u:#x})");
        assert_eq!(hb_ucd::script(u), hb_script(u), "script({u:#x})");
        n += 1;
    }
    report!("{n} codepoints: general category and script identical");
}

/// Not an equality test: counts the codepoints whose script or canonical
/// combining class differ between HarfBuzz 8.2.0 (C++) and HarfRust 0.15,
/// which shape with their own tables. Strings with them are the expected
/// Unicode-version differences of the shaping difftest.
#[test]
fn unicode_version_differences_report() {
    let mut script = 0;
    let mut ccc = 0;
    let mut first = Vec::new();
    for u in 0..=0x10_FFFFu32 {
        let s = harfrust::unicode::script(u).tag().to_be_bytes();
        let c = harfrust::unicode::combining_class(u) as u32;
        let ds = s != hb_script(u);
        let dc = c != hb_combining_class(u);
        script += ds as u32;
        ccc += dc as u32;
        if (ds || dc) && first.len() < 20 {
            first.push(format!("{u:#x}"));
        }
    }
    report!(
        "HarfRust vs HarfBuzz 8.2.0: {script} codepoints differ in script, {ccc} in combining class; first: {first:?}"
    );
}
