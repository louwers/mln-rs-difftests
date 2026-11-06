// Difftests for //vendor/csscolorparser's cstdlib.rs (glibc strtof, strtod,
// strtoll, ERANGE included) and mln_util::string::stof (std::stof).

use proptest::prelude::*;
use reference_ffi as cpp;
use reference_ffi::{same_f32, same_f64};

fn config(cases: u32) -> ProptestConfig {
    ProptestConfig { cases, failure_persistence: None, ..ProptestConfig::default() }
}

fn check_float(s: &[u8]) {
    let r = csscolorparser::cstdlib::strtof(s);
    let c = cpp::strtof(s);
    assert!(
        same_f32(r.0, c.0) && r.1 == c.1 && r.2 == c.2,
        "strtof({:?}): rust {:?} ({:#x}) c++ {:?} ({:#x})",
        String::from_utf8_lossy(s),
        r,
        r.0.to_bits(),
        c,
        c.0.to_bits()
    );
    let r = csscolorparser::cstdlib::strtod(s);
    let c = cpp::strtod(s);
    assert!(
        same_f64(r.0, c.0) && r.1 == c.1 && r.2 == c.2,
        "strtod({:?}): rust {:?} ({:#x}) c++ {:?} ({:#x})",
        String::from_utf8_lossy(s),
        r,
        r.0.to_bits(),
        c,
        c.0.to_bits()
    );
    if let Ok(st) = std::str::from_utf8(s) {
        let r = mln_util::string::stof(st);
        let (code, v) = cpp::stof(st);
        match (r, code) {
            (Ok(x), 0) => assert!(same_f32(x, v), "stof({st:?}): rust {x:e} c++ {v:e}"),
            (Err(mln_util::string::StofError::InvalidArgument), 1) => {}
            (Err(mln_util::string::StofError::OutOfRange), 2) => {}
            _ => panic!("stof({st:?}): rust {r:?} c++ code {code} value {v:e}"),
        }
    }
}

fn check_int(s: &[u8], base: u32) {
    let r = csscolorparser::cstdlib::strtoll(s, base);
    let c = cpp::strtoll(s, base);
    assert_eq!(r, c, "strtoll({:?}, {base})", String::from_utf8_lossy(s));
}

const FLOAT_EDGE_CASES: &[&str] = &[
    "",
    " ",
    "-",
    "+",
    ".",
    "-.",
    "+.5",
    ".5",
    "5.",
    "-0",
    "+0",
    "0",
    "00",
    "0.",
    "0.e5",
    ".e5",
    "e5",
    "1e",
    "1e+",
    "1e-",
    "1e+5",
    "1E5",
    "1e-5x",
    "0x",
    "0X",
    "-0x",
    "0x.",
    "0x.p1",
    "0x0.p1",
    "0xp1",
    "0x0p1",
    "0x1",
    "0x1p",
    "0x1p+",
    "0x1.8p1",
    "0x.8",
    "0X1P-149",
    "0x1p-150",
    "0x1.000001p-150",
    "0x1p-151",
    "0x1p-126",
    "0x1.fffffep-127",
    "0x1.fffffcp-127",
    "0x0.ffffffp-126",
    "0x1.fffffffp-127",
    "0x1.ffffffp-127",
    "0x1.fffffe0000001p-127",
    "0x1p127",
    "0x1p128",
    "0x1.fffffep127",
    "0x1.ffffffp127",
    "0x1.fffffefp127",
    "inf",
    "INF",
    "-inf",
    "infinity",
    "-Infinity",
    "infinit",
    "infx",
    "in",
    "nan",
    "NaN",
    "-nan",
    "nan(",
    "nan()",
    "nan(123)",
    "nan(abc_X9)",
    "nan(a-b)",
    "nanx",
    "na",
    "\t\n\x0b\x0c\r 1",
    "\x00 1",
    "1\x002",
    "1.5%",
    "50%",
    "1e39",
    "3.4028235e38",
    "3.40282356e38",
    "3.4028235677973366e38",
    "3.40282357e38",
    "1e-38",
    "1.17549435e-38",
    "1.1754942e-38",
    "1e-45",
    "1.4e-45",
    "7e-46",
    "7.006e-46",
    "7.0064923216240854e-46",
    "7.0064923216240862e-46",
    "1e-46",
    "1e-50",
    "1e-400",
    "1e400",
    "-1e400",
    "0e999999999999",
    "0.000e-9999",
    "1e99999999999999999999",
    "1e-99999999999999999999",
    "0.1",
    "0.2",
    "0.3",
    "1.000000059604644775390625",
    "1.0000000596046447753906250000000001",
    "1.00000005960464477539062499999999",
    "16777217",
    "16777216.5",
    "1.7976931348623157e308",
    "1.7976931348623158e308",
    "1.797693134862315807e308",
    "1.8e308",
    "4.9e-324",
    "2.4703282292062327e-324",
    "2.4703282292062328e-324",
    "2.2250738585072011e-308",
    "2.2250738585072012e-308",
    "123456789012345678901234567890",
    "0.000000000000000000000000000000000000000000001",
    "- 1",
    "+-1",
    "--1",
    "1,5",
    "1 5",
    "é",
    "1é",
];

#[test]
fn float_edge_cases() {
    for s in FLOAT_EDGE_CASES {
        check_float(s.as_bytes());
    }
    // Long inputs beyond the 800-digit cut-off.
    let long = format!("1.{}1e-40", "0".repeat(2000));
    check_float(long.as_bytes());
    let long = format!("0.{}", "9".repeat(1500));
    check_float(long.as_bytes());
    let long = format!("0x1.{}1p-127", "0".repeat(400));
    check_float(long.as_bytes());
    println!("float edge cases: {}", FLOAT_EDGE_CASES.len() + 3);
}

/// Exact decimal and hex spellings of f32 rounding boundaries (midpoints
/// between neighbours, and the neighbours themselves), and their neighbours.
fn f32_boundaries(bits: u32) -> Vec<String> {
    let x = f32::from_bits(bits & 0x7fff_ffff);
    if !x.is_finite() {
        return vec![];
    }
    let next = f32::from_bits(x.to_bits() + 1);
    let mut out = vec![format!("{x:e}"), format!("{:.120e}", x as f64)];
    {
        // The midpoint is exact in f64 (25 significant bits).
        // Past FLT_MAX, the next value would be 2^128.
        let next_d = if next.is_infinite() { 2f64.powi(128) } else { next as f64 };
        let mid = (x as f64 + next_d) / 2.0;
        let exact = format!("{mid:.160e}");
        out.push(exact.clone());
        // Just above and below the midpoint.
        let (mantissa, exp) = exact.split_once('e').unwrap();
        let trimmed = mantissa.trim_end_matches('0');
        out.push(format!("{trimmed}000001e{exp}"));
        let below = f64::from_bits(mid.to_bits() - 1);
        out.push(format!("{below:.160e}"));
        // Hex: mid = m * 2^e
        let (m, e) = {
            let b = mid.to_bits();
            let exp = ((b >> 52) & 0x7ff) as i64;
            let frac = b & ((1 << 52) - 1);
            if exp == 0 { (frac, -1074) } else { (frac | (1 << 52), exp - 1075) }
        };
        out.push(format!("0x{m:x}p{e}"));
        out.push(format!("0x{:x}p{}", m * 16 + 1, e - 4));
        out.push(format!("-0x{:x}p{}", m * 16 - 1, e - 4));
    }
    out
}

fn decimal_strategy() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec!["", "", "-", "+", " ", "\t-"]),
        "[0-9]{0,30}",
        prop::option::of("[0-9]{0,30}"),
        prop::option::of((prop::sample::select(vec!["e", "E", "e-", "e+", "E-"]), "[0-9]{0,4}")),
        prop::sample::select(vec!["", "", "x", "%", ")", ",", "e", ".", "0"]),
    )
        .prop_map(|(sign, int, frac, exp, tail)| {
            let mut s = format!("{sign}{int}");
            if let Some(f) = frac {
                s.push('.');
                s.push_str(&f);
            }
            if let Some((e, d)) = exp {
                s.push_str(e);
                s.push_str(&d);
            }
            s.push_str(tail);
            s
        })
}

fn hex_strategy() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec!["", "-", "+", " "]),
        prop::sample::select(vec!["0x", "0X"]),
        "[0-9a-fA-F]{0,20}",
        prop::option::of("[0-9a-fA-F]{0,20}"),
        prop::option::of((prop::sample::select(vec!["p", "P", "p-", "p+"]), "[0-9]{0,4}")),
        prop::sample::select(vec!["", "x", "g", "."]),
    )
        .prop_map(|(sign, x, int, frac, exp, tail)| {
            let mut s = format!("{sign}{x}{int}");
            if let Some(f) = frac {
                s.push('.');
                s.push_str(&f);
            }
            if let Some((e, d)) = exp {
                s.push_str(e);
                s.push_str(&d);
            }
            s.push_str(tail);
            s
        })
}

proptest! {
    #![proptest_config(config(100_000))]

    #[test]
    fn float_random_strings(s in "[ \t+\\-0-9.eExXpPa-fAFinItyNA()_%,]{0,24}") {
        check_float(s.as_bytes());
    }

    #[test]
    fn float_decimal(s in decimal_strategy()) {
        check_float(s.as_bytes());
    }

    #[test]
    fn float_hex(s in hex_strategy()) {
        check_float(s.as_bytes());
    }

    #[test]
    fn float_exponent_sweep(m in "[1-9][0-9]{0,25}", e in -70i32..60) {
        check_float(format!("{m}e{e}").as_bytes());
        check_float(format!("0.{m}e{e}").as_bytes());
        check_float(format!("0x{m}p{}", e * 3).as_bytes());
    }
}

proptest! {
    #![proptest_config(config(30_000))]

    #[test]
    fn float_f32_boundaries(bits in prop_oneof![any::<u32>(), 0u32..0x0100_0000, 0x7f00_0000u32..0x7f80_0000]) {
        for s in f32_boundaries(bits) {
            check_float(s.as_bytes());
        }
    }

    #[test]
    fn float_f64_shortest(bits in any::<u64>()) {
        let x = f64::from_bits(bits);
        check_float(format!("{x:e}").as_bytes());
        check_float(format!("{x:.25e}").as_bytes());
    }
}

const INT_EDGE_CASES: &[&str] = &[
    "",
    " ",
    "-",
    "+",
    "0",
    "-0",
    "0x",
    "0X",
    "-0x",
    "0x0",
    "0xg",
    "0xx",
    "0b",
    "0b1",
    "0b2",
    "0B11",
    "00",
    "07",
    "08",
    "0o7",
    "x",
    "12abc",
    "  \t\n42",
    "9223372036854775807",
    "9223372036854775808",
    "-9223372036854775808",
    "-9223372036854775809",
    "18446744073709551615",
    "18446744073709551616",
    "99999999999999999999999999",
    "-99999999999999999999999999",
    "0x7fffffffffffffff",
    "0x8000000000000000",
    "-0x8000000000000000",
    "0xffffffffffffffff",
    "0x10000000000000000",
    "zz",
    "ZZ",
    "1\x002",
    "é1",
    "+-1",
    "fff",
    "FFF",
    "#fff",
];

#[test]
fn int_edge_cases() {
    let mut n = 0;
    for s in INT_EDGE_CASES {
        for base in [0, 2, 8, 10, 16, 36] {
            check_int(s.as_bytes(), base);
            n += 1;
        }
    }
    println!("int edge cases: {n}");
}

proptest! {
    #![proptest_config(config(100_000))]

    #[test]
    fn int_random(s in "[ \t+\\-0-9a-zA-ZxXbB]{0,24}", base in prop::sample::select(vec![0u32, 2, 8, 10, 16, 36])) {
        check_int(s.as_bytes(), base);
    }

    #[test]
    fn int_values(v in any::<i128>(), base in prop::sample::select(vec![10u32, 16, 0])) {
        let s = match base {
            16 => if v < 0 { format!("-0x{:x}", v.unsigned_abs()) } else { format!("0x{v:x}") },
            _ => v.to_string(),
        };
        check_int(s.as_bytes(), base);
        let small = (v as i64).to_string();
        check_int(small.as_bytes(), 10);
    }
}

/// glibc's ERANGE rules (tininess after rounding, inexact only).
#[test]
fn observed_erange() {
    assert_eq!(cpp::strtof(b"0x1p-149"), (f32::from_bits(1), 8, false)); // exact subnormal
    assert_eq!(cpp::strtof(b"0x1p-150"), (0.0, 8, true));
    assert!(cpp::strtof(b"0x1.fffffep-127").2); // tiny, rounds up to FLT_MIN
    assert!(!cpp::strtof(b"0x1.ffffffp-127").2); // rounds to FLT_MIN at 24 bits too
    assert_eq!(cpp::strtof(b"1e39"), (f32::INFINITY, 4, true));
    assert_eq!(cpp::strtof(b"inf"), (f32::INFINITY, 3, false));
    assert_eq!(cpp::strtoll(b"0b11", 0), (3, 4, false)); // C23 strtoll
}
