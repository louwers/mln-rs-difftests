// Difftest for //vendor/csscolorparser against vendor/csscolorparser.

use proptest::prelude::*;
use reference_ffi as cpp;
use reference_ffi::same_f32;

fn config(cases: u32) -> ProptestConfig {
    ProptestConfig { cases, failure_persistence: None, ..ProptestConfig::default() }
}

fn check(s: &str) {
    let r = csscolorparser::parse(s).map(|c| ([c.r, c.g, c.b], c.a));
    let c = cpp::css_parse(s);
    let same = match (r, c) {
        (None, None) => true,
        (Some((rr, ra)), Some((cr, ca))) => rr == cr && same_f32(ra, ca),
        _ => false,
    };
    assert!(same, "parse({s:?}): rust {r:?} c++ {c:?}");
    // The tolerant operator==.
    if let (Some(a), Some(b)) = (r, cpp::css_parse("rgba(10, 20, 30, 0.5)")) {
        let ra = csscolorparser::Color::new(a.0[0], a.0[1], a.0[2], a.1);
        let rb = csscolorparser::Color::new(b.0[0], b.0[1], b.0[2], b.1);
        assert_eq!(ra == rb, cpp::css_equal(a, b), "operator== {a:?} {b:?}");
    }
}

const EDGE_CASES: &[&str] = &[
    "",
    " ",
    "red",
    "RED",
    " r e d ",
    "Red\t",
    "transparent",
    "rebeccapurple",
    "#",
    "#f",
    "#ff",
    "#fff",
    "#ffff",
    "#fffff",
    "#ffffff",
    "#fffffff",
    "#ffffffff",
    "#fffffffff",
    "#FFF",
    "#-ff",
    "#-fff",
    "#+fff",
    "#0x1",
    "#0x12",
    "#0x123",
    "#0xfff",
    "#\tfff",
    "#\nfffff",
    "# fff",
    "#ggg",
    "#12g",
    "#1g2",
    "#123456789",
    "#00000000",
    "#ff001100",
    "#80808080",
    "rgb(0,0,0)",
    "rgb(255,255,255)",
    "rgb(256,-1,300)",
    "rgb( 1 , 2 , 3 )",
    "rgb(,,,)",
    "rgb(,,)",
    "rgb(1,2)",
    "rgb(1,2,3,4)",
    "rgba(1,2,3)",
    "rgba(1,2,3,0.5)",
    "rgba(1,2,3,.5)",
    "rgba(1,2,3,5e-1)",
    "rgba(1,2,3,50%)",
    "rgba(1,2,3,-0)",
    "rgba(1,2,3,-0.0)",
    "rgba(1,2,3,nan)",
    "rgba(1,2,3,inf)",
    "rgba(1,2,3,-inf)",
    "rgba(1,2,3,1e400)",
    "rgba(1,2,3,0x1p-1)",
    "rgba(1,2,3,0x.8)",
    "rgba(1,2,3,)",
    "rgba(1,2,3,,)",
    "rgba(1,2,3,4,)",
    "rgb(50%,50%,50%)",
    "rgb(100%,0%,-5%)",
    "rgb(nan%,inf%,-inf%)",
    "rgb(1e2,1e3,1e-3)",
    "rgb(0x10,0x20,0x30)",
    "rgb(+10,-0,+0)",
    "rgb(10.9,10.5,10.4)",
    "rgb(254.5%,0.5%,99.9%)",
    "rgb(9223372036854775807,0,0)",
    "rgb(9223372036854775806,0,0)",
    "rgb(9223372036854775295,0,0)",
    "rgb(9223372036854775296,0,0)",
    "rgb(99999999999999999999,0,0)",
    "rgb(-99999999999999999999,0,0)",
    "rgb(1,2,3))",
    "rgb((1,2,3)",
    "rgb(1,2,3",
    "rgb1,2,3)",
    "(1,2,3)",
    "rgb()",
    "rgb(1;2;3)",
    "hsl(0,100%,50%)",
    "hsl(120,100%,50%)",
    "hsl(240,100%,50%)",
    "hsl(360,100%,50%)",
    "hsl(-120,100%,50%)",
    "hsl(720.5,50%,50%)",
    "hsl(inf,50%,50%)",
    "hsl(-inf,50%,50%)",
    "hsl(nan,50%,50%)",
    "hsl(0,nan,50%)",
    "hsl(0,50%,nan)",
    "hsl(0,50%,nan%)",
    "hsl(1e40,50%,50%)",
    "hsl(-0,50%,50%)",
    "hsl(0,0,0)",
    "hsl(0,1,1)",
    "hsl(0,1,0.5)",
    "hsl(0,1.5,0.5)",
    "hsl(30,100%,50%,0.5)",
    "hsla(30,100%,50%)",
    "hsla(30,100%,50%,0.5)",
    "hsla(30,100%,50%,0.5,1)",
    "hsl(9999999999999999999,0,0)",
    "hsl(45,100%,50%)",
    "hsl(-315,100%,50%)",
    "hsl(-675,100%,50%)",
    "hsla(900,15%,90%,0.5)",
    "HSL(45,100%,50%)",
    "hsv(0,0,0)",
    "rgb(é,1,2)",
    "#ééé",
    "rgb(1,2,3)\u{0}",
    "rgb(\u{0}1,2,3)",
    "\u{0}red",
    "rgb(1\u{0},2,3)",
    "rgb(\t1,\n2,\x0b3)",
    "rgb(1%%,2,3)",
    "rgb(%,%,%)",
    "rgba(0,0,0,%)",
    "rgb(1e,2e+,3e-)",
];

#[test]
fn edge_cases() {
    for s in EDGE_CASES {
        check(s);
    }
    println!("csscolorparser edge cases: {}", EDGE_CASES.len());
}

fn number() -> impl Strategy<Value = String> {
    prop_oneof![
        "[+\\-]?[0-9]{0,4}(\\.[0-9]{0,3})?%?",
        "[+\\-]?[0-9]{1,25}",
        "[+\\-]?[0-9]{0,3}(\\.[0-9]{0,3})?[eE][+\\-]?[0-9]{1,3}%?",
        "[+\\-]?0[xX][0-9a-fA-F]{0,6}(\\.[0-9a-fA-F]{0,3})?([pP][+\\-]?[0-9]{1,3})?%?",
        prop::sample::select(vec!["nan", "inf", "-inf", "infinity", "nan(1)", "", " ", "\t", "%", "1e400", "-0"])
            .prop_map(str::to_owned),
        "[ \t0-9.eE%+\\-xX]{0,8}",
    ]
}

fn functional() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec!["rgb", "rgba", "hsl", "hsla", "RGB", " rgba ", "Hsla", "rgbx", ""]),
        prop::collection::vec(number(), 0..6),
        prop::sample::select(vec![",", ",", ",", ", ", ";"]),
        prop::sample::select(vec![")", ")", ")", "", "))", ") ", ")x"]),
    )
        .prop_map(|(f, params, sep, close)| format!("{f}({}{close}", params.join(sep)))
}

fn hex() -> impl Strategy<Value = String> {
    prop_oneof!["#[0-9a-fA-F]{0,9}", "#[0-9a-fA-F \t+\\-xXg]{0,9}", "#(0[xX])?[0-9a-f]{1,7}",]
}

proptest! {
    #![proptest_config(config(100_000))]

    #[test]
    fn functional_notation(s in functional()) {
        check(&s);
    }

    #[test]
    fn hex_notation(s in hex()) {
        check(&s);
    }

    #[test]
    fn random_strings(s in "[ \t#(),.%+\\-0-9a-fA-Fhlsrgbxyznpi\u{e9}\u{0}]{0,20}") {
        check(&s);
    }

    #[test]
    fn exact_rgba(r in 0u32..300, g in 0u32..300, b in 0u32..300, a in -0.5f64..1.5, pct in any::<bool>()) {
        let s = if pct {
            format!("rgba({}%, {}%, {}%, {a})", r as f64 / 3.0, g as f64 / 2.0, b, )
        } else {
            format!("rgba({r}, {g}, {b}, {a})")
        };
        check(&s);
        check(&format!("hsla({r}, {}%, {}%, {a})", g % 101, b % 101));
        check(&format!("hsl({}, {}, {})", r as f64 * 1.7 - 200.0, a, a * 0.5));
    }
}

/// C++ quirks the port reproduces.
#[test]
fn observed_quirks() {
    // double -> int64_t overflow (UB) is cvttsd2si's INT64_MIN on x86-64,
    // which clamps to 0, not 255.
    assert_eq!(cpp::css_parse("rgb(9223372036854775807,0,0)"), Some(([0, 0, 0], 1.0)));
    assert_eq!(cpp::css_parse("rgb(9223372036854775295,0,0)"), Some(([255, 0, 0], 1.0)));
    // std::getline drops the trailing empty field.
    assert_eq!(cpp::css_parse("rgb(,,,)"), Some(([0, 0, 0], 1.0)));
    // NaN alpha passes the clamp.
    assert!(cpp::css_parse("rgba(0,0,0,nan)").unwrap().1.is_nan());
}
