//! Safe wrappers around the C++ reference (reference.cpp).

#![allow(clippy::missing_safety_doc)]

use std::os::raw::c_char;

mod ffi {
    use super::c_char;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct BoundsSpec {
        pub kind: i32,
        pub a_lat: f64,
        pub a_lon: f64,
        pub b_lat: f64,
        pub b_lon: f64,
        pub c_lat: f64,
        pub c_lon: f64,
        pub z: u32,
        pub x: u32,
        pub y: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct BoundsQuery {
        pub sw_lat: f64,
        pub sw_lon: f64,
        pub ne_lat: f64,
        pub ne_lon: f64,
        pub se_lat: f64,
        pub se_lon: f64,
        pub nw_lat: f64,
        pub nw_lon: f64,
        pub center_lat: f64,
        pub center_lon: f64,
        pub constrained_lat: f64,
        pub constrained_lon: f64,
        pub valid: u8,
        pub is_empty: u8,
        pub crosses: u8,
        pub contains_point: u8,
        pub contains_point_wrapped: u8,
        pub contains_area: u8,
        pub contains_area_wrapped: u8,
        pub intersects: u8,
        pub intersects_wrapped: u8,
        pub equal: u8,
    }

    unsafe extern "C" {
        pub fn ref_wrap(v: f64, lo: f64, hi: f64) -> f64;
        pub fn ref_wrapf(v: f32, lo: f32, hi: f32) -> f32;
        pub fn ref_clamp(v: f64, lo: f64, hi: f64) -> f64;
        pub fn ref_clampf(v: f32, lo: f32, hi: f32) -> f32;
        pub fn ref_clamp_i64(v: i64, lo: i64, hi: i64) -> i64;
        pub fn ref_min(a: f64, b: f64) -> f64;
        pub fn ref_max(a: f64, b: f64) -> f64;
        pub fn ref_minf(a: f32, b: f32) -> f32;
        pub fn ref_maxf(a: f32, b: f32) -> f32;
        pub fn ref_ceil_log2(x: u64) -> u32;
        pub fn ref_log2(x: f64) -> f64;
        pub fn ref_angles(d: f64, f: f32, out_d: *mut f64, out_f: *mut f32);
        pub fn ref_unitbezier(p1x: f64, p1y: f64, p2x: f64, p2y: f64, x: f64, eps: f64, out: *mut f64);
        pub fn ref_strtof(s: *const c_char, len: usize, consumed: *mut usize, erange: *mut i32) -> f32;
        pub fn ref_strtod(s: *const c_char, len: usize, consumed: *mut usize, erange: *mut i32) -> f64;
        pub fn ref_strtoll(s: *const c_char, len: usize, base: i32, consumed: *mut usize, erange: *mut i32) -> i64;
        pub fn ref_stof(s: *const c_char, len: usize, out: *mut f32) -> i32;
        pub fn ref_css_parse(s: *const c_char, len: usize, rgb: *mut u8, a: *mut f32) -> i32;
        pub fn ref_css_equal(rgb_a: *const u8, a_a: f32, rgb_b: *const u8, a_b: f32) -> i32;
        pub fn ref_color_parse(s: *const c_char, len: usize, out: *mut f32) -> i32;
        pub fn ref_color_interpolate(from: *const f32, to: *const f32, t: f64, space: i32, out: *mut f32);
        pub fn ref_color_to_array(c: *const f32, out: *mut f64);
        pub fn ref_color_stringify(c: *const f32, buf: *mut c_char, cap: usize) -> usize;
        pub fn ref_to_string_double(v: f64, decimal: i32, buf: *mut c_char, cap: usize) -> usize;
        pub fn ref_to_string_i64(v: i64, buf: *mut c_char, cap: usize) -> usize;
        pub fn ref_to_string_u64(v: u64, buf: *mut c_char, cap: usize) -> usize;
        pub fn ref_writer(prog: *const u8, len: usize, buf: *mut c_char, cap: usize, results: *mut u8) -> usize;
        pub fn ref_interpolate_f_d(a: f32, b: f32, t: f64) -> f32;
        pub fn ref_interpolate_f_f(a: f32, b: f32, t: f32) -> f32;
        pub fn ref_interpolate_d_d(a: f64, b: f64, t: f64) -> f64;
        pub fn ref_interpolate_arr2_d(a: *const f32, b: *const f32, t: f64, out: *mut f32);
        pub fn ref_interpolate_padding(
            a: *const f32,
            b: *const f32,
            td: f64,
            tf: f32,
            out_d: *mut f32,
            out_f: *mut f32,
        );
        pub fn ref_interpolation_factor(base: f32, min: f32, max: f32, z: f32) -> f32;
        pub fn ref_latlng_new(
            lat: f64,
            lon: f64,
            wrapped: i32,
            out: *mut f64,
            err: *mut c_char,
            cap: usize,
            err_len: *mut usize,
        ) -> i32;
        pub fn ref_latlng_unwrap_for_shortest_path(lat: f64, lon: f64, end_lat: f64, end_lon: f64, out: *mut f64);
        pub fn ref_latlng_tile(z: u8, x: u32, y: u32, wrap: i16, out: *mut f64);
        pub fn ref_bounds_query(
            spec: *const BoundsSpec,
            area: *const BoundsSpec,
            p_lat: f64,
            p_lon: f64,
            with_center: i32,
            q: *mut BoundsQuery,
        );
        pub fn ref_edge_insets(
            t: f64,
            l: f64,
            b: f64,
            r: f64,
            w: u16,
            h: u16,
            out: *mut f64,
            err: *mut c_char,
            cap: usize,
            err_len: *mut usize,
        ) -> i32;
    }
}

pub use ffi::{BoundsQuery, BoundsSpec};

/// Calls `f(buf, cap) -> len` with a growing buffer until the string fits.
fn read_string(mut f: impl FnMut(*mut c_char, usize) -> usize) -> String {
    let mut cap = 64;
    loop {
        let mut buf = vec![0u8; cap];
        let len = f(buf.as_mut_ptr() as *mut c_char, cap);
        if len <= cap {
            buf.truncate(len);
            return String::from_utf8(buf).expect("C++ wrote invalid UTF-8");
        }
        cap = len;
    }
}

pub fn wrap(v: f64, lo: f64, hi: f64) -> f64 {
    unsafe { ffi::ref_wrap(v, lo, hi) }
}
pub fn wrapf(v: f32, lo: f32, hi: f32) -> f32 {
    unsafe { ffi::ref_wrapf(v, lo, hi) }
}
pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    unsafe { ffi::ref_clamp(v, lo, hi) }
}
pub fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    unsafe { ffi::ref_clampf(v, lo, hi) }
}
pub fn clamp_i64(v: i64, lo: i64, hi: i64) -> i64 {
    unsafe { ffi::ref_clamp_i64(v, lo, hi) }
}
pub fn min(a: f64, b: f64) -> f64 {
    unsafe { ffi::ref_min(a, b) }
}
pub fn max(a: f64, b: f64) -> f64 {
    unsafe { ffi::ref_max(a, b) }
}
pub fn minf(a: f32, b: f32) -> f32 {
    unsafe { ffi::ref_minf(a, b) }
}
pub fn maxf(a: f32, b: f32) -> f32 {
    unsafe { ffi::ref_maxf(a, b) }
}
pub fn ceil_log2(x: u64) -> u32 {
    unsafe { ffi::ref_ceil_log2(x) }
}
pub fn log2(x: f64) -> f64 {
    unsafe { ffi::ref_log2(x) }
}
/// `([deg2rad(d), rad2deg(d)], [deg2radf(f), rad2degf(f)])`
pub fn angles(d: f64, f: f32) -> ([f64; 2], [f32; 2]) {
    let mut od = [0.0; 2];
    let mut of = [0.0f32; 2];
    unsafe { ffi::ref_angles(d, f, od.as_mut_ptr(), of.as_mut_ptr()) };
    (od, of)
}

/// solve, solveCurveX, sampleCurveX, sampleCurveY, sampleCurveDerivativeX,
/// p1.x, p1.y, p2.x, p2.y
pub fn unitbezier(p1x: f64, p1y: f64, p2x: f64, p2y: f64, x: f64, eps: f64) -> [f64; 9] {
    let mut out = [0.0; 9];
    unsafe { ffi::ref_unitbezier(p1x, p1y, p2x, p2y, x, eps, out.as_mut_ptr()) };
    out
}

pub fn strtof(s: &[u8]) -> (f32, usize, bool) {
    let mut consumed = 0;
    let mut erange = 0;
    let v = unsafe { ffi::ref_strtof(s.as_ptr() as *const c_char, s.len(), &mut consumed, &mut erange) };
    (v, consumed, erange != 0)
}

pub fn strtod(s: &[u8]) -> (f64, usize, bool) {
    let mut consumed = 0;
    let mut erange = 0;
    let v = unsafe { ffi::ref_strtod(s.as_ptr() as *const c_char, s.len(), &mut consumed, &mut erange) };
    (v, consumed, erange != 0)
}

pub fn strtoll(s: &[u8], base: u32) -> (i64, usize, bool) {
    let mut consumed = 0;
    let mut erange = 0;
    let v = unsafe { ffi::ref_strtoll(s.as_ptr() as *const c_char, s.len(), base as i32, &mut consumed, &mut erange) };
    (v, consumed, erange != 0)
}

/// 0: ok, 1: invalid_argument, 2: out_of_range
pub fn stof(s: &str) -> (i32, f32) {
    let mut out = 0.0f32;
    let r = unsafe { ffi::ref_stof(s.as_ptr() as *const c_char, s.len(), &mut out) };
    (r, out)
}

/// The raw CSSColorParser result.
pub fn css_parse(s: &str) -> Option<([u8; 3], f32)> {
    let mut rgb = [0u8; 3];
    let mut a = 0.0f32;
    let r = unsafe { ffi::ref_css_parse(s.as_ptr() as *const c_char, s.len(), rgb.as_mut_ptr(), &mut a) };
    (r != 0).then_some((rgb, a))
}

pub fn css_equal(a: ([u8; 3], f32), b: ([u8; 3], f32)) -> bool {
    unsafe { ffi::ref_css_equal(a.0.as_ptr(), a.1, b.0.as_ptr(), b.1) != 0 }
}

/// `Err(())` when skipped (NaN alpha would trip the C++ assert).
pub fn color_parse(s: &str) -> Result<Option<[f32; 4]>, ()> {
    let mut out = [0.0f32; 4];
    match unsafe { ffi::ref_color_parse(s.as_ptr() as *const c_char, s.len(), out.as_mut_ptr()) } {
        0 => Ok(None),
        1 => Ok(Some(out)),
        _ => Err(()),
    }
}

pub fn color_interpolate(from: [f32; 4], to: [f32; 4], t: f64, space: i32) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    unsafe { ffi::ref_color_interpolate(from.as_ptr(), to.as_ptr(), t, space, out.as_mut_ptr()) };
    out
}

pub fn color_to_array(c: [f32; 4]) -> [f64; 4] {
    let mut out = [0.0; 4];
    unsafe { ffi::ref_color_to_array(c.as_ptr(), out.as_mut_ptr()) };
    out
}

pub fn color_stringify(c: [f32; 4]) -> String {
    read_string(|buf, cap| unsafe { ffi::ref_color_stringify(c.as_ptr(), buf, cap) })
}

pub fn to_string_double(v: f64, decimal: bool) -> String {
    read_string(|buf, cap| unsafe { ffi::ref_to_string_double(v, decimal as i32, buf, cap) })
}

pub fn to_string_i64(v: i64) -> String {
    read_string(|buf, cap| unsafe { ffi::ref_to_string_i64(v, buf, cap) })
}

pub fn to_string_u64(v: u64) -> String {
    read_string(|buf, cap| unsafe { ffi::ref_to_string_u64(v, buf, cap) })
}

/// Runs a Writer program (see reference.cpp); returns the output and each
/// op's return value.
pub fn writer(prog: &[u8], ops: usize) -> (String, Vec<bool>) {
    let mut results = vec![0u8; ops];
    let s =
        read_string(|buf, cap| unsafe { ffi::ref_writer(prog.as_ptr(), prog.len(), buf, cap, results.as_mut_ptr()) });
    (s, results.into_iter().map(|r| r != 0).collect())
}

pub fn interpolate_f_d(a: f32, b: f32, t: f64) -> f32 {
    unsafe { ffi::ref_interpolate_f_d(a, b, t) }
}
pub fn interpolate_f_f(a: f32, b: f32, t: f32) -> f32 {
    unsafe { ffi::ref_interpolate_f_f(a, b, t) }
}
pub fn interpolate_d_d(a: f64, b: f64, t: f64) -> f64 {
    unsafe { ffi::ref_interpolate_d_d(a, b, t) }
}
pub fn interpolate_arr2_d(a: [f32; 2], b: [f32; 2], t: f64) -> [f32; 2] {
    let mut out = [0.0f32; 2];
    unsafe { ffi::ref_interpolate_arr2_d(a.as_ptr(), b.as_ptr(), t, out.as_mut_ptr()) };
    out
}
/// (double-t result, float-t result)
pub fn interpolate_padding(a: [f32; 4], b: [f32; 4], td: f64, tf: f32) -> ([f32; 4], [f32; 4]) {
    let mut od = [0.0f32; 4];
    let mut of = [0.0f32; 4];
    unsafe { ffi::ref_interpolate_padding(a.as_ptr(), b.as_ptr(), td, tf, od.as_mut_ptr(), of.as_mut_ptr()) };
    (od, of)
}
pub fn interpolation_factor(base: f32, min: f32, max: f32, z: f32) -> f32 {
    unsafe { ffi::ref_interpolation_factor(base, min, max, z) }
}

pub fn latlng_new(lat: f64, lon: f64, wrapped: bool) -> Result<(f64, f64), String> {
    let mut out = [0.0; 2];
    let mut err_len = 0usize;
    let mut err = vec![0u8; 256];
    let r = unsafe {
        ffi::ref_latlng_new(
            lat,
            lon,
            wrapped as i32,
            out.as_mut_ptr(),
            err.as_mut_ptr() as *mut c_char,
            err.len(),
            &mut err_len,
        )
    };
    if r == 0 {
        Ok((out[0], out[1]))
    } else {
        err.truncate(err_len);
        Err(String::from_utf8(err).unwrap())
    }
}

pub fn latlng_unwrap_for_shortest_path(lat: f64, lon: f64, end_lat: f64, end_lon: f64) -> (f64, f64) {
    let mut out = [0.0; 2];
    unsafe { ffi::ref_latlng_unwrap_for_shortest_path(lat, lon, end_lat, end_lon, out.as_mut_ptr()) };
    (out[0], out[1])
}

/// (LatLng(canonical), LatLng(unwrapped)) as [lat, lon, lat, lon]
pub fn latlng_tile(z: u8, x: u32, y: u32, wrap: i16) -> [f64; 4] {
    let mut out = [0.0; 4];
    unsafe { ffi::ref_latlng_tile(z, x, y, wrap, out.as_mut_ptr()) };
    out
}

pub fn bounds_query(spec: &BoundsSpec, area: &BoundsSpec, p_lat: f64, p_lon: f64, with_center: bool) -> BoundsQuery {
    let mut q = BoundsQuery::default();
    unsafe { ffi::ref_bounds_query(spec, area, p_lat, p_lon, with_center as i32, &mut q) };
    q
}

/// Ok((values, is_flush)) or the error message.
pub fn edge_insets(t: f64, l: f64, b: f64, r: f64, w: u16, h: u16) -> Result<([f64; 6], bool), String> {
    let mut out = [0.0; 6];
    let mut err_len = 0usize;
    let mut err = vec![0u8; 256];
    let res = unsafe {
        ffi::ref_edge_insets(
            t,
            l,
            b,
            r,
            w,
            h,
            out.as_mut_ptr(),
            err.as_mut_ptr() as *mut c_char,
            err.len(),
            &mut err_len,
        )
    };
    match res {
        1 => {
            err.truncate(err_len);
            Err(String::from_utf8(err).unwrap())
        }
        r => Ok((out, r == 2)),
    }
}

/// Bit-exact float equality where any NaN equals any NaN.
pub fn same_f64(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

pub fn same_f32(a: f32, b: f32) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

/// Bit patterns that exercise the edge cases of f64 code.
pub fn special_f64() -> Vec<f64> {
    let mut v = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        f64::NAN,
        -f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        f64::MAX,
        f64::MIN,
        f64::EPSILON,
        5e-324,
        -5e-324,
        2.225073858507201e-308,
        0.1,
        0.2,
        0.3,
        1.0 / 3.0,
        2.0 / 3.0,
        180.0,
        -180.0,
        360.0,
        -360.0,
        90.0,
        -90.0,
        540.0,
        1e21,
        1e22,
        1e-7,
        1e-6,
        123456789012345678901.0,
        255.0,
        254.99999999999997,
        f32::MAX as f64,
        f32::MIN_POSITIVE as f64,
    ];
    for e in -30..=30 {
        v.push(10f64.powi(e));
        v.push(-(10f64.powi(e)));
        v.push(2f64.powi(e));
    }
    v
}
