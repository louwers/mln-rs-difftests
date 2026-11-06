// Difftests for //crates/mln_math (include/mln/math) and the numeric parts
// of //crates/mln_util (unitbezier.hpp, interpolate.hpp/.cpp).

use mln_util::interpolate::{Interpolate, interpolation_factor};
use mln_util::range::Range;
use mln_util::{Padding, UnitBezier};
use proptest::prelude::*;
use reference_ffi as cpp;
use reference_ffi::{same_f32, same_f64, special_f64};

fn config(cases: u32) -> ProptestConfig {
    ProptestConfig { cases, failure_persistence: None, ..ProptestConfig::default() }
}

/// Any f64 bit pattern, or a "nice" value.
fn any_f64() -> impl Strategy<Value = f64> {
    prop_oneof![any::<u64>().prop_map(f64::from_bits), prop::sample::select(special_f64()), -1000.0f64..1000.0,]
}

fn any_f32() -> impl Strategy<Value = f32> {
    prop_oneof![
        any::<u32>().prop_map(f32::from_bits),
        prop::sample::select(special_f64()).prop_map(|v| v as f32),
        -1000.0f32..1000.0,
    ]
}

fn check_wrap(v: f64, lo: f64, hi: f64) {
    let r = mln_math::wrap(v, lo, hi);
    let c = cpp::wrap(v, lo, hi);
    assert!(same_f64(r, c), "wrap({v:e}, {lo:e}, {hi:e}): rust {r:e} c++ {c:e}");
    let (vf, lf, hf) = (v as f32, lo as f32, hi as f32);
    let r = mln_math::wrap_f32(vf, lf, hf);
    let c = cpp::wrapf(vf, lf, hf);
    assert!(same_f32(r, c), "wrapf({vf:e}, {lf:e}, {hf:e}): rust {r:e} c++ {c:e}");
}

fn check_clamp(v: f64, lo: f64, hi: f64) {
    let r = mln_math::clamp_f64(v, lo, hi);
    let c = cpp::clamp(v, lo, hi);
    assert!(same_f64(r, c), "clamp({v:e}, {lo:e}, {hi:e}): rust {r:e} c++ {c:e}");
    let (vf, lf, hf) = (v as f32, lo as f32, hi as f32);
    let r = mln_math::clamp_f32(vf, lf, hf);
    let c = cpp::clampf(vf, lf, hf);
    assert!(same_f32(r, c), "clampf({vf:e}, {lf:e}, {hf:e}): rust {r:e} c++ {c:e}");
}

fn check_minmax(a: f64, b: f64) {
    let (r, c) = (mln_math::min_f64(a, b), cpp::min(a, b));
    assert!(same_f64(r, c), "min({a:e}, {b:e}): rust {r:e} c++ {c:e}");
    let (r, c) = (mln_math::max_f64(a, b), cpp::max(a, b));
    assert!(same_f64(r, c), "max({a:e}, {b:e}): rust {r:e} c++ {c:e}");
    let (af, bf) = (a as f32, b as f32);
    let (r, c) = (mln_math::min_f32(af, bf), cpp::minf(af, bf));
    assert!(same_f32(r, c), "minf({af:e}, {bf:e}): rust {r:e} c++ {c:e}");
    let (r, c) = (mln_math::max_f32(af, bf), cpp::maxf(af, bf));
    assert!(same_f32(r, c), "maxf({af:e}, {bf:e}): rust {r:e} c++ {c:e}");
}

#[test]
fn minmax_clamp_wrap_edge_cases() {
    let s = special_f64();
    let mut n = 0;
    for &a in &s {
        for &b in &s {
            check_minmax(a, b);
            n += 1;
            for &v in &[0.0, -0.0, f64::NAN, 1.0, -1.0, 200.0, f64::INFINITY] {
                check_clamp(v, a, b);
                check_clamp(a, v, b);
                n += 2;
            }
            // wrap with finite, ordered ranges (the callers' contract).
            if a.is_finite() && b.is_finite() && a < b {
                for &v in &s {
                    check_wrap(v, a, b);
                    n += 1;
                }
            }
        }
    }
    println!("minmax/clamp/wrap edge cases: {n}");
}

proptest! {
    #![proptest_config(config(50_000))]

    #[test]
    fn minmax_clamp(a in any_f64(), b in any_f64(), v in any_f64()) {
        check_minmax(a, b);
        check_clamp(v, a, b);
        let (ai, bi, vi) = (a as i64, b as i64, v as i64);
        if ai <= bi {
            prop_assert_eq!(mln_math::clamp(vi, ai, bi), cpp::clamp_i64(vi, ai, bi));
        }
    }

    #[test]
    fn wrap(v in any_f64(), lo in -1e6f64..1e6, width in 1e-3f64..1e6) {
        check_wrap(v, lo, lo + width);
        check_wrap(v, -180.0, 180.0);
        check_wrap(v, 0.0, 360.0);
    }

    #[test]
    fn ceil_log2(x in any::<u64>(), shift in 0u32..64) {
        prop_assert_eq!(mln_math::ceil_log2(x), cpp::ceil_log2(x));
        let y = x >> shift;
        prop_assert_eq!(mln_math::ceil_log2(y), cpp::ceil_log2(y));
        let p = 1u64 << shift;
        for z in [p.wrapping_sub(1), p, p.wrapping_add(1)] {
            prop_assert_eq!(mln_math::ceil_log2(z), cpp::ceil_log2(z));
        }
    }

    #[test]
    fn log2_and_angles(d in any_f64(), f in any_f32()) {
        prop_assert!(same_f64(mln_math::log2(d), cpp::log2(d)));
        let (od, of) = cpp::angles(d, f);
        prop_assert!(same_f64(mln_math::deg2rad(d), od[0]));
        prop_assert!(same_f64(mln_math::rad2deg(d), od[1]));
        prop_assert!(same_f32(mln_math::deg2radf(f), of[0]));
        prop_assert!(same_f32(mln_math::rad2degf(f), of[1]));
    }
}

#[test]
fn ceil_log2_edge_cases() {
    for x in [0u64, 1, 2, 3, 4, 5, 7, 8, 9, u64::MAX, u64::MAX - 1, 1 << 63, (1 << 63) + 1] {
        assert_eq!(mln_math::ceil_log2(x), cpp::ceil_log2(x), "{x}");
    }
}

fn check_unitbezier(p1x: f64, p1y: f64, p2x: f64, p2y: f64, x: f64, eps: f64) {
    let b = UnitBezier::new(p1x, p1y, p2x, p2y);
    let r = [
        b.solve(x, eps),
        b.solve_curve_x(x, eps),
        b.sample_curve_x(x),
        b.sample_curve_y(x),
        b.sample_curve_derivative_x(x),
        b.get_p1().0,
        b.get_p1().1,
        b.get_p2().0,
        b.get_p2().1,
    ];
    let c = cpp::unitbezier(p1x, p1y, p2x, p2y, x, eps);
    for i in 0..9 {
        assert!(
            same_f64(r[i], c[i]),
            "UnitBezier({p1x}, {p1y}, {p2x}, {p2y}) x={x} eps={eps} [{i}]: rust {:e} c++ {:e}",
            r[i],
            c[i]
        );
    }
}

proptest! {
    #![proptest_config(config(20_000))]

    // x control points in [0, 1] (the style spec's constraint; outside it the
    // C++ bisection can loop forever); y control points and x anywhere.
    #[test]
    fn unitbezier(
        p1x in 0.0f64..=1.0,
        p1y in -10.0f64..10.0,
        p2x in 0.0f64..=1.0,
        p2y in -10.0f64..10.0,
        x in prop_oneof![-0.5f64..1.5, Just(0.0), Just(1.0), Just(0.5), Just(f64::NAN)],
        eps in prop_oneof![Just(1e-6), Just(1e-9), Just(0.01), 1e-12f64..0.1],
    ) {
        check_unitbezier(p1x, p1y, p2x, p2y, x, eps);
    }
}

#[test]
fn unitbezier_edge_cases() {
    let mut n = 0;
    for (p1x, p1y, p2x, p2y) in [
        (0.0, 0.0, 0.25, 1.0),
        (0.25, 0.1, 0.25, 1.0),
        (0.42, 0.0, 1.0, 1.0),
        (0.0, 0.0, 0.58, 1.0),
        (0.42, 0.0, 0.58, 1.0),
        (0.0, 0.0, 1.0, 1.0),
        (1.0, 0.0, 0.0, 1.0),
        (0.0, 1.0, 1.0, 0.0),
    ] {
        for x in [0.0, -0.0, 1e-300, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0, -0.1, 1.1, f64::NAN] {
            check_unitbezier(p1x, p1y, p2x, p2y, x, 1e-6);
            n += 1;
        }
    }
    println!("unitbezier edge cases: {n}");
}

proptest! {
    #![proptest_config(config(50_000))]

    #[test]
    fn interpolate(a in any_f32(), b in any_f32(), td in prop_oneof![0.0f64..=1.0, any_f64()], tf in prop_oneof![0.0f32..=1.0, any_f32()]) {
        prop_assert!(same_f32(a.interpolate(&b, td), cpp::interpolate_f_d(a, b, td)));
        prop_assert!(same_f32(a.interpolate_f32(&b, tf), cpp::interpolate_f_f(a, b, tf)));
        let (ad, bd) = (a as f64 * 1.37, b as f64 / 3.0);
        prop_assert!(same_f64(ad.interpolate(&bd, td), cpp::interpolate_d_d(ad, bd, td)));
        let r = [a, b].interpolate(&[b, a], td);
        let c = cpp::interpolate_arr2_d([a, b], [b, a], td);
        prop_assert!(same_f32(r[0], c[0]) && same_f32(r[1], c[1]), "array<float, 2> {:?} {:?}", r, c);
        let pa = [a, b, a * 0.5, b * 2.0];
        let pb = [b, a, 1.0, -1.0];
        let (cd, cf) = cpp::interpolate_padding(pa, pb, td, tf);
        let ppa = Padding::new(pa[0], pa[1], pa[2], pa[3]);
        let ppb = Padding::new(pb[0], pb[1], pb[2], pb[3]);
        let rd = ppa.interpolate(&ppb, td).to_float_array();
        let rf = ppa.interpolate_f32(&ppb, tf).to_float_array();
        for i in 0..4 {
            prop_assert!(same_f32(rd[i], cd[i]) && same_f32(rf[i], cf[i]), "padding {:?} {:?} / {:?} {:?}", rd, cd, rf, cf);
        }
    }

    #[test]
    fn interpolation_factor_matches(
        base in prop_oneof![Just(1.0f32), 0.0f32..5.0, any_f32()],
        min in -5.0f32..30.0,
        width in prop_oneof![Just(0.0f32), 0.0f32..30.0],
        z in prop_oneof![-5.0f32..40.0, any_f32()],
    ) {
        let max = min + width;
        let r = interpolation_factor(base, Range::new(min, max), z);
        let c = cpp::interpolation_factor(base, min, max, z);
        prop_assert!(same_f32(r, c), "interpolationFactor({base:e}, [{min:e}, {max:e}], {z:e}): rust {r:e} c++ {c:e}");
    }
}

/// What the oracle does on the platform's libm and compiler (documents the
/// DEVIATIONS.md entry on fmin/fmax).
#[test]
fn observed_signed_zero_behaviour() {
    assert_eq!(cpp::min(0.0, -0.0).to_bits(), 0.0f64.to_bits());
    assert_eq!(cpp::min(-0.0, 0.0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(cpp::max(0.0, -0.0).to_bits(), 0.0f64.to_bits());
    assert_eq!(cpp::max(-0.0, 0.0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(cpp::clamp(f64::NAN, 0.0, 1.0), 1.0);
}
