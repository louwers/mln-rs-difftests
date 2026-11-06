// Difftest for mln_util::Color (src/mln/util/color.cpp).

use mln_util::{Color, ColorSpace};
use proptest::prelude::*;
use reference_ffi as cpp;
use reference_ffi::{same_f32, same_f64};

fn config(cases: u32) -> ProptestConfig {
    ProptestConfig { cases, failure_persistence: None, ..ProptestConfig::default() }
}

fn arr(c: &Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

fn same4(a: [f32; 4], b: [f32; 4]) -> bool {
    (0..4).all(|i| same_f32(a[i], b[i]))
}

fn check_parse(s: &str) -> bool {
    // NaN alpha trips the C++ constructor's assert (and the Rust
    // debug_assert): skipped.
    let Ok(c) = cpp::color_parse(s) else {
        return false;
    };
    let r = Color::parse(s).map(|c| arr(&c));
    let same = match (r, c) {
        (None, None) => true,
        (Some(a), Some(b)) => same4(a, b),
        _ => false,
    };
    assert!(same, "Color::parse({s:?}): rust {r:?} c++ {c:?}");
    true
}

fn check_output(c: [f32; 4]) {
    let color = Color::new_unchecked(c[0], c[1], c[2], c[3]);
    let r = color.to_array();
    let k = cpp::color_to_array(c);
    assert!((0..4).all(|i| same_f64(r[i], k[i])), "toArray({c:?}): rust {r:?} c++ {k:?}");
    let r = color.stringify();
    let k = cpp::color_stringify(c);
    assert_eq!(r, k, "stringify({c:?})");
}

fn check_interpolate(from: [f32; 4], to: [f32; 4], t: f64) {
    let f = Color::new(from[0], from[1], from[2], from[3]);
    let g = Color::new(to[0], to[1], to[2], to[3]);
    for (space, id) in [(ColorSpace::Rgb, 0), (ColorSpace::Lab, 1), (ColorSpace::Hcl, 2)] {
        let r = arr(&Color::interpolate(&f, &g, t, space));
        let c = cpp::color_interpolate(from, to, t, id);
        assert!(same4(r, c), "interpolate({from:?}, {to:?}, {t}, {space:?}): rust {r:?} c++ {c:?}");
    }
}

const PARSE_CASES: &[&str] = &[
    "red",
    "white",
    "black",
    "transparent",
    "#fff",
    "#0000",
    "#12345678",
    "rgba(255, 0, 0, 0.5)",
    "rgba(255, 51, 0, 1)",
    "rgba(1, 2, 3, 0.1)",
    "rgba(254, 253, 252, 0.333)",
    "rgba(255,255,255,-0)",
    "hsla(120, 50%, 50%, 0.25)",
    "rgba(0,0,0,nan)",
    "rgba(10,20,30,1e-45)",
    "not-a-color",
    "",
];

#[test]
fn parse_edge_cases() {
    let mut skipped = 0;
    for s in PARSE_CASES {
        if !check_parse(s) {
            skipped += 1;
        }
    }
    println!("Color::parse edge cases: {} ({skipped} skipped: NaN alpha)", PARSE_CASES.len());
}

fn unit_f32() -> impl Strategy<Value = f32> {
    prop_oneof![
        0.0f32..=1.0,
        Just(0.0f32),
        Just(-0.0f32),
        Just(1.0f32),
        Just(0.5f32),
        Just(1e-45f32),
        (0u32..=255).prop_map(|b| b as f32 / 255.0),
    ]
}

fn unit_f64() -> impl Strategy<Value = f64> {
    prop_oneof![0.0f64..=1.0, Just(0.0), Just(1.0), Just(0.5), Just(-0.0), Just(1e-300)]
}

proptest! {
    #![proptest_config(config(50_000))]

    #[test]
    fn parse_rgba(r in 0u32..=255, g in 0u32..=255, b in 0u32..=255, a in "(0|1|0?\\.[0-9]{1,4}|[0-9]{1,3}%|-?[0-9]e-?[0-9]{1,2})") {
        check_parse(&format!("rgba({r}, {g}, {b}, {a})"));
        check_parse(&format!("#{r:02x}{g:02x}{b:02x}{:02x}", (r * 7 + g) % 256));
        check_parse(&format!("hsla({r}, {}%, {}%, {a})", g % 101, b % 101));
    }

    #[test]
    fn parse_random(s in "[ #(),.%+\\-0-9a-fhlsrgbn]{0,20}") {
        check_parse(&s);
    }

    #[test]
    fn output_any_bits(r in any::<u32>(), g in any::<u32>(), b in any::<u32>(), a in any::<u32>()) {
        check_output([f32::from_bits(r), f32::from_bits(g), f32::from_bits(b), f32::from_bits(a)]);
    }

    #[test]
    fn output_valid(r in unit_f32(), g in unit_f32(), b in unit_f32(), a in unit_f32()) {
        // Premultiplied, as Color normally holds.
        check_output([r * a, g * a, b * a, a]);
        check_output([r, g, b, a]);
    }

    #[test]
    fn interpolate(f in prop::array::uniform4(unit_f32()), g in prop::array::uniform4(unit_f32()), t in unit_f64(), premultiply in any::<bool>()) {
        let (f, g) = if premultiply {
            ([f[0] * f[3], f[1] * f[3], f[2] * f[3], f[3]], [g[0] * g[3], g[1] * g[3], g[2] * g[3], g[3]])
        } else {
            // Not premultiplied (r > a): keep r / a moderate, or LAB/HCL
            // overflow to NaN, which the C++ Color constructor asserts on.
            ([f[0], f[1], f[2], 0.05 + 0.95 * f[3]], [g[0], g[1], g[2], 0.05 + 0.95 * g[3]])
        };
        check_interpolate(f, g, t);
    }
}

#[test]
fn interpolate_edge_cases() {
    let colors: Vec<[f32; 4]> = [
        "black",
        "white",
        "red",
        "green",
        "blue",
        "transparent",
        "rgba(255,0,0,0.5)",
        "rgba(0,0,0,0)",
        "rgba(128,128,128,1)",
        "rgba(10,200,30,0.01)",
        "#ff00ff",
        "hsl(200, 50%, 50%)",
    ]
    .iter()
    .map(|s| arr(&Color::parse(s).unwrap()))
    .collect();
    let mut n = 0;
    for &a in &colors {
        for &b in &colors {
            for t in [0.0, 0.25, 0.5, 0.75, 1.0, 1e-9, 1.0 - 1e-9] {
                check_interpolate(a, b, t);
                n += 1;
            }
        }
    }
    println!("Color::interpolate edge cases: {n} (x3 color spaces)");
}
