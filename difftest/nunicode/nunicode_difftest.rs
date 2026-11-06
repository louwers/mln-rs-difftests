// Bit-exact difftest of //vendor/nunicode against the C nunicode of the C++
// core (reference.cpp): DUCET weights (every codepoint, every contraction
// state), the casemap/unaccent tables (every codepoint), the string
// functions of the default platform (uppercase/lowercase/unaccent) and
// nu_strcoll/nu_strcasecoll (exact return values).

use std::path::PathBuf;

use nunicode::casemap::{nu_tolower, nu_tounaccent, nu_toupper};
use nunicode::ducet::nu_ducet_weight;
use proptest::prelude::*;

unsafe extern "C" {
    fn ref_nu_strcoll(a: *const u8, alen: usize, b: *const u8, blen: usize) -> i32;
    fn ref_nu_strcasecoll(a: *const u8, alen: usize, b: *const u8, blen: usize) -> i32;
    fn ref_nu_ducet_weight(codepoint: u32) -> i32;
    fn ref_nu_ducet_weight_reentry(codepoint: u32, state: i32, rollback: *mut i32) -> i32;
    fn ref_nu_toupper(codepoint: u32, out: *mut u8, cap: usize) -> std::ffi::c_long;
    fn ref_nu_tolower(codepoint: u32, out: *mut u8, cap: usize) -> std::ffi::c_long;
    fn ref_nu_tounaccent(codepoint: u32, out: *mut u8, cap: usize) -> std::ffi::c_long;
    fn ref_uppercase(s: *const u8, len: usize, out: *mut u8, cap: usize) -> usize;
    fn ref_lowercase(s: *const u8, len: usize, out: *mut u8, cap: usize) -> usize;
    fn ref_unaccent(s: *const u8, len: usize, out: *mut u8, cap: usize) -> usize;
}

/// The highest codepoint tried (past U+10FFFF, to cover the "undefined
/// weight" arithmetic and out-of-range lookups).
const MAX_CODEPOINT: u32 = 0x11_0100;

type RefString = unsafe extern "C" fn(*const u8, usize, *mut u8, usize) -> usize;
type RefMap = unsafe extern "C" fn(u32, *mut u8, usize) -> std::ffi::c_long;

fn ref_string(f: RefString, s: &[u8]) -> Vec<u8> {
    let mut cap = s.len() * 4 + 16;
    loop {
        let mut buf = vec![0u8; cap];
        let len = unsafe { f(s.as_ptr(), s.len(), buf.as_mut_ptr(), cap) };
        if len <= cap {
            buf.truncate(len);
            return buf;
        }
        cap = len;
    }
}

fn ref_map(f: RefMap, codepoint: u32) -> Option<Vec<u8>> {
    let mut buf = [0u8; 64];
    let len = unsafe { f(codepoint, buf.as_mut_ptr(), buf.len()) };
    if len < 0 {
        return None;
    }
    assert!((len as usize) <= buf.len());
    Some(buf[..len as usize].to_vec())
}

/// A Rust transformation as the bytes up to its 0 terminator.
fn rust_map(t: Option<&'static [u8]>) -> Option<Vec<u8>> {
    t.map(|m| m[..m.iter().position(|b| *b == 0).expect("0-terminated")].to_vec())
}

fn ref_strcoll(a: &[u8], b: &[u8]) -> i32 {
    unsafe { ref_nu_strcoll(a.as_ptr(), a.len(), b.as_ptr(), b.len()) }
}

fn ref_strcasecoll(a: &[u8], b: &[u8]) -> i32 {
    unsafe { ref_nu_strcasecoll(a.as_ptr(), a.len(), b.as_ptr(), b.len()) }
}

#[test]
fn ducet_weights_every_codepoint() {
    for cp in 0..=MAX_CODEPOINT {
        let cpp = unsafe { ref_nu_ducet_weight(cp) };
        assert_eq!(nu_ducet_weight(cp, None), cpp, "nu_ducet_weight({cp:#x})");
    }
    println!("nu_ducet_weight: {} codepoints compared", MAX_CODEPOINT + 1);
}

#[test]
fn ducet_contractions_every_state_and_codepoint() {
    // The contraction states are the negative first-call weights.
    let mut states: Vec<i32> = (0..=0x10FFFF).map(|cp| nu_ducet_weight(cp, None)).filter(|w| *w < 0).collect();
    states.sort();
    states.dedup();
    assert_eq!(states.len(), 34);
    let mut count = 0u64;
    for &state in &states {
        for cp in 0..=MAX_CODEPOINT {
            let mut cpp_rollback = 0;
            let cpp = unsafe { ref_nu_ducet_weight_reentry(cp, state, &mut cpp_rollback) };
            let mut rollback = state;
            let rust = nu_ducet_weight(cp, Some(&mut rollback));
            assert_eq!((rust, rollback), (cpp, cpp_rollback), "state {state}, codepoint {cp:#x}");
            count += 1;
        }
    }
    println!("nu_ducet_weight re-entry: {} states x {} codepoints = {count} compared", states.len(), MAX_CODEPOINT + 1);
}

#[test]
fn casemap_tables_every_codepoint() {
    for cp in 0..=MAX_CODEPOINT {
        assert_eq!(rust_map(nu_toupper(cp)), ref_map(ref_nu_toupper, cp), "nu_toupper({cp:#x})");
        assert_eq!(rust_map(nu_tolower(cp)), ref_map(ref_nu_tolower, cp), "nu_tolower({cp:#x})");
        assert_eq!(rust_map(nu_tounaccent(cp)), ref_map(ref_nu_tounaccent, cp), "nu_tounaccent({cp:#x})");
    }
    println!("nu_toupper/nu_tolower/nu_tounaccent: {} codepoints x 3 compared", MAX_CODEPOINT + 1);
}

fn check_strings(s: &str) {
    let b = s.as_bytes();
    for (name, rust_bytes, rust_string, cpp) in [
        ("uppercase", nunicode::uppercase_bytes(b), nunicode::uppercase(s), ref_string(ref_uppercase, b)),
        ("lowercase", nunicode::lowercase_bytes(b), nunicode::lowercase(s), ref_string(ref_lowercase, b)),
        ("unaccent", nunicode::unaccent_bytes(b), nunicode::unaccent(s), ref_string(ref_unaccent, b)),
    ] {
        // The bytes are exact; the String API is the C++ bytes, lossily
        // decoded where they are not UTF-8 (the b4_utf8 quirk).
        assert_eq!(rust_bytes, cpp, "{name}({s:?})");
        assert_eq!(rust_string, String::from_utf8_lossy(&cpp), "{name}({s:?}) as String");
        if std::str::from_utf8(&cpp).is_err() {
            INVALID_UTF8.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

static INVALID_UTF8: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[test]
fn string_functions_every_scalar_value() {
    let mut count = 0;
    for c in (0..=0x10FFFF).filter_map(char::from_u32) {
        // alone, at the start, in the middle and before a NUL (final sigma
        // and the like look at the following codepoint)
        for s in [format!("{c}"), format!("{c}a"), format!("a{c}b"), format!("{c}\0x")] {
            check_strings(&s);
            count += 1;
        }
    }
    println!(
        "uppercase/lowercase/unaccent: {count} strings x 3 compared ({} C++ results not UTF-8, so far in this process)",
        INVALID_UTF8.load(std::sync::atomic::Ordering::Relaxed)
    );
}

fn check_collation(a: &str, b: &str) {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    assert_eq!(nunicode::strcoll(a, b), ref_strcoll(ab, bb), "strcoll({a:?}, {b:?})");
    assert_eq!(nunicode::strcasecoll(a, b), ref_strcasecoll(ab, bb), "strcasecoll({a:?}, {b:?})");
    // collator.cpp's diacritic-insensitive modes
    let (ua, ub) = (nunicode::unaccent(a), nunicode::unaccent(b));
    assert_eq!(
        nunicode::strcoll(&ua, &ub),
        ref_strcoll(&ref_string(ref_unaccent, ab), &ref_string(ref_unaccent, bb)),
        "strcoll(unaccent({a:?}), unaccent({b:?}))"
    );
    assert_eq!(
        nunicode::strcasecoll(&ua, &ub),
        ref_strcasecoll(&ref_string(ref_unaccent, ab), &ref_string(ref_unaccent, bb)),
        "strcasecoll(unaccent({a:?}), unaccent({b:?}))"
    );
    // collator.cpp's Impl::compare, all four modes, on the exact bytes
    let (cua, cub) = (ref_string(ref_unaccent, ab), ref_string(ref_unaccent, bb));
    for (cs, ds, cpp) in [
        (true, true, ref_strcoll(ab, bb)),
        (false, true, ref_strcasecoll(ab, bb)),
        (true, false, ref_strcoll(&cua, &cub)),
        (false, false, ref_strcasecoll(&cua, &cub)),
    ] {
        assert_eq!(nunicode::collator_compare(a, b, cs, ds), cpp, "collator_compare({a:?}, {b:?}, {cs}, {ds})");
    }
}

/// Roots of the DUCET contractions (_ducet_switch.c) and codepoints that
/// complete some of them.
const CONTRACTION_CHARS: &[char] = &[
    'L', 'l', '·', '·', 'И', 'и', '\u{0306}', 'ا', 'و', 'ي', '\u{0653}', '\u{0654}', '\u{0655}', 'ஒ', '\u{0BD7}', 'เ',
    'แ', 'โ', 'ใ', 'ไ', 'ก', 'ข', 'ฮ', 'ເ', 'ແ', 'ໂ', 'ໃ', 'ໄ', 'ກ', 'ຂ', 'ຮ', 'ဥ', '\u{102E}', 'ᦵ', 'ᦶ', 'ᦷ', 'ᦺ',
    'ᦀ', 'ᦁ', 'ᦫ', 'ᬅ', 'ᬇ', 'ᬉ', 'ᬋ', 'ᬍ', 'ᬑ', '\u{1B35}', 'ꪵ', 'ꪶ', 'ꪹ', 'ꪻ', 'ꪼ', 'ꪀ', 'ꪁ', 'ꪯ', 'a', 'A', 'ä',
    'Ä', 'ß', 'Σ', 'σ', 'ς', 'ǅ', 'ﬃ', '\0',
];

fn contraction_string() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(CONTRACTION_CHARS), 0..8).prop_map(|v| v.into_iter().collect())
}

fn latin_string() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            prop::char::range('a', 'z'),
            prop::char::range('A', 'Z'),
            prop::char::range('\u{00C0}', '\u{017F}'),
            prop::char::range('\u{0300}', '\u{036F}'),
            prop::char::range('\u{1E00}', '\u{1EFF}'),
            prop::char::range('\u{0370}', '\u{03FF}'),
            Just(' '),
        ],
        0..10,
    )
    .prop_map(|v| v.into_iter().collect())
}

fn any_text() -> impl Strategy<Value = String> {
    prop_oneof![any::<String>(), latin_string(), contraction_string()]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 20000, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn string_functions_generated(s in any_text()) {
        check_strings(&s);
    }

    #[test]
    fn collation_generated(a in any_text(), b in any_text()) {
        check_collation(&a, &b);
        check_collation(&a, &a);
        check_collation(&format!("{a}{b}"), &a);
    }

    #[test]
    fn collation_contractions(a in contraction_string(), b in contraction_string()) {
        check_collation(&a, &b);
    }
}

#[test]
fn collation_edge_cases() {
    let cases = [
        ("", ""),
        ("", "a"),
        ("a", ""),
        ("a", "a\0b"),
        ("a\0b", "a\0c"),
        ("\0", ""),
        ("abc", "abd"),
        ("abc", "ab"),
        ("L·", "L"),
        ("L·a", "La"),
        ("l·", "L·"),
        ("เก", "เข"),
        ("เ", "เก"),
        ("ไก", "ก"),
        ("ᦵᦀ", "ᦵ"),
        ("И\u{0306}", "Й"),
        ("ß", "SS"),
        ("ß", "ss"),
        ("ﬃ", "FFI"),
        ("Σ", "σ"),
        ("ς", "Σ"),
    ];
    for (a, b) in cases {
        check_collation(a, b);
        check_collation(b, a);
    }
    println!("collation edge cases: {} pairs x 2 orders compared", cases.len());
}

fn corpus_root() -> PathBuf {
    let base = PathBuf::from(std::env::var("TEST_SRCDIR").unwrap());
    for repo in ["maplibre+", "maplibre"] {
        let p = base.join(repo).join("metrics/integration/expression-tests/collator");
        if p.exists() {
            return p;
        }
    }
    panic!("expression-test-files not in the runfiles");
}

#[test]
fn collation_corpus_inputs() {
    let mut count = 0;
    for entry in std::fs::read_dir(corpus_root()).unwrap() {
        let path = entry.unwrap().path().join("test.json");
        let test: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for input in test["inputs"].as_array().into_iter().flatten() {
            let props = &input[1]["properties"];
            let (Some(lhs), Some(rhs)) = (props["lhs"].as_str(), props["rhs"].as_str()) else {
                continue;
            };
            check_collation(lhs, rhs);
            check_collation(rhs, lhs);
            check_strings(lhs);
            check_strings(rhs);
            count += 1;
        }
    }
    assert!(count > 0);
    println!("collator corpus inputs: {count} pairs compared");
}
