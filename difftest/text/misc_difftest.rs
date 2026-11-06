// Difftests of the small text helpers against the C++ core:
// FontStackHasher (libstdc++ std::hash<std::string> + hash_combine) and
// LanguageTag::fromBCP47 / toBCP47 (Boost.Spirit grammar) on hand-written
// and generated tags.

use mln_text::language_tag::LanguageTag;
use mln_text::tagged_string::font_stack_hash;
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, TestRng, TestRunner};
use text_reference::*;

macro_rules! report {
    ($($arg:tt)*) => {{
        use std::io::Write;
        writeln!(std::io::stderr(), $($arg)*).unwrap();
    }};
}

fn runner() -> TestRunner {
    TestRunner::new_with_rng(Config::default(), TestRng::deterministic_rng(Default::default()))
}

#[test]
fn font_stack_hash_matches() {
    let mut cases: Vec<Vec<String>> = vec![
        vec![],
        vec!["Open Sans Regular".into()],
        vec!["Open Sans Regular".into(), "Arial Unicode MS Regular".into()],
        vec!["a".into()],
        vec!["12345678".into(), "123456789".into(), "Noto Sans CJK JP Bold".into()],
        vec!["Ünïcödé".into(), "中文".into()],
    ];
    let mut r = runner();
    let strategy = proptest::collection::vec("[ -~]{1,40}", 0..5);
    for _ in 0..2000 {
        cases.push(strategy.new_tree(&mut r).unwrap().current());
    }
    for c in &cases {
        assert_eq!(font_stack_hash(c), ref_font_stack_hash(c), "{c:?}");
    }
    report!("FontStackHasher: {} font stacks identical", cases.len());
}

fn rust_language_tag(tag: &str) -> String {
    let t = LanguageTag::from_bcp47(tag);
    format!(
        "{}|{}|{}|{}",
        t.language.as_deref().unwrap_or("-"),
        t.script.as_deref().unwrap_or("-"),
        t.region.as_deref().unwrap_or("-"),
        t.to_bcp47()
    )
}

#[test]
fn language_tags_match() {
    let mut cases: Vec<String> = [
        "",
        "de",
        "de-hans",
        "de-US",
        "de-ext-two-Hans-US-variant-x-private",
        "zh-Hant-TW",
        "en-GB-oed",
        "i-klingon",
        "x-private",
        "zh-min-nan",
        "sgn-BE-FR",
        "abcd",
        "abcde",
        "abcdefghi",
        "a",
        "en-",
        "-en",
        "en--US",
        "EN-us",
        "es-419",
        "sr-Latn-RS-1234",
        "de-CH-1901",
        "en-a-bbb-x-a-ccc",
        "en-x",
        "en-US-u-ca-gregory",
        "zh-yue-HK",
        "tlh",
        "ar-arb-SA",
        "ab-cde-fgh-ijk-lmn",
        "é",
        "de-hans-",
        "1234",
        "art-lojban",
        "no-bok",
        "ab-X-1",
        "ab-U-51",
        "X-private",
        "I-KLINGON",
        "Art-Lojban",
        "ab-z-12",
        "ab-0-12",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut r = runner();
    let subtag = proptest::prop_oneof![
        "[a-zA-Z]{1,9}",
        "[0-9]{1,4}",
        "[a-zA-Z0-9]{1,9}",
        Just("x".to_string()),
        Just("u".to_string())
    ];
    let strategy = proptest::collection::vec(subtag, 1..7).prop_map(|v| v.join("-"));
    for _ in 0..5000 {
        cases.push(strategy.new_tree(&mut r).unwrap().current());
    }
    for c in &cases {
        assert_eq!(rust_language_tag(c), ref_language_tag(c), "{c:?}");
    }
    report!("LanguageTag: {} tags identical", cases.len());
}

use proptest::strategy::Just;
