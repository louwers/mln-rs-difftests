// Difftest for mln_util::geo (include/mln/util/geo.hpp, src/mln/util/geo.cpp).

use mln_util::geo::{EdgeInsets, LatLng, LatLngBounds, WrapMode};
use proptest::prelude::*;
use reference_ffi as cpp;
use reference_ffi::{BoundsQuery, BoundsSpec, same_f64, special_f64};

fn config(cases: u32) -> ProptestConfig {
    ProptestConfig { cases, failure_persistence: None, ..ProptestConfig::default() }
}

fn check_latlng(lat: f64, lon: f64) {
    for wrapped in [false, true] {
        let mode = if wrapped { WrapMode::Wrapped } else { WrapMode::Unwrapped };
        let r = LatLng::with_mode(lat, lon, mode).map(|l| (l.latitude(), l.longitude()));
        let c = cpp::latlng_new(lat, lon, wrapped);
        let same = match (&r, &c) {
            (Ok(a), Ok(b)) => same_f64(a.0, b.0) && same_f64(a.1, b.1),
            (Err(a), Err(b)) => a == b,
            _ => false,
        };
        assert!(same, "LatLng({lat:e}, {lon:e}, {mode:?}): rust {r:?} c++ {c:?}");
    }
}

#[test]
fn latlng_edge_cases() {
    let s = special_f64();
    for &lat in &s {
        for &lon in &s {
            check_latlng(lat, lon);
        }
    }
    println!("LatLng edge cases: {}", s.len() * s.len());
}

fn lat() -> impl Strategy<Value = f64> {
    prop_oneof![-90.0f64..=90.0, Just(90.0), Just(-90.0), Just(0.0), Just(-0.0)]
}

fn lon() -> impl Strategy<Value = f64> {
    prop_oneof![
        -180.0f64..=180.0,
        -1000.0f64..1000.0,
        prop::sample::select(vec![180.0, -180.0, 0.0, -0.0, 360.0, -360.0, 540.0, -540.0, 179.99999999999997]),
        -1e6f64..1e6,
    ]
}

proptest! {
    #![proptest_config(config(50_000))]

    #[test]
    fn latlng(lat in prop_oneof![lat(), any::<u64>().prop_map(f64::from_bits)], lon in prop_oneof![lon(), any::<u64>().prop_map(f64::from_bits)]) {
        check_latlng(lat, lon);
    }

    #[test]
    fn unwrap_for_shortest_path(a in lat(), b in lon(), c in lat(), d in lon()) {
        let mut l = LatLng::new(a, b).unwrap();
        l.unwrap_for_shortest_path(&LatLng::new(c, d).unwrap());
        let k = cpp::latlng_unwrap_for_shortest_path(a, b, c, d);
        prop_assert!(same_f64(l.latitude(), k.0) && same_f64(l.longitude(), k.1));
    }

    #[test]
    fn tile(z in 0u8..=25, x in any::<u32>(), y in any::<u32>(), wrap in any::<i16>()) {
        // CanonicalTileID asserts x, y < 2^z.
        let n = 1u64 << z;
        let (x, y) = ((x as u64 % n) as u32, (y as u64 % n) as u32);
        let a = LatLng::from_canonical_tile(z, x, y);
        let b = LatLng::from_unwrapped_tile(z, x, y, wrap);
        let c = cpp::latlng_tile(z, x, y, wrap);
        let r = [a.latitude(), a.longitude(), b.latitude(), b.longitude()];
        prop_assert!((0..4).all(|i| same_f64(r[i], c[i])), "tile {z}/{x}/{y} wrap {wrap}: rust {r:?} c++ {c:?}");
    }
}

fn make_bounds(s: &BoundsSpec) -> LatLngBounds {
    let a = LatLng::new(s.a_lat, s.a_lon).unwrap();
    let b = LatLng::new(s.b_lat, s.b_lon).unwrap();
    let c = LatLng::new(s.c_lat, s.c_lon).unwrap();
    match s.kind {
        0 => LatLngBounds::world(),
        1 => LatLngBounds::empty(),
        2 => LatLngBounds::default(),
        3 => LatLngBounds::singleton(a),
        4 => LatLngBounds::hull(a, b),
        5 => {
            let mut r = LatLngBounds::hull(a, b);
            r.extend(&c);
            r
        }
        6 => LatLngBounds::from_canonical_tile(s.z as u8, s.x, s.y),
        _ => {
            let mut r = LatLngBounds::empty();
            r.extend(&a);
            r.extend_bounds(&LatLngBounds::singleton(b));
            r
        }
    }
}

fn query(s: &BoundsSpec, area_s: &BoundsSpec, p_lat: f64, p_lon: f64, with_center: bool) -> BoundsQuery {
    let bounds = make_bounds(s);
    let area = make_bounds(area_s);
    let p = LatLng::new(p_lat, p_lon).unwrap();
    let mut q = BoundsQuery {
        sw_lat: bounds.southwest().latitude(),
        sw_lon: bounds.southwest().longitude(),
        ne_lat: bounds.northeast().latitude(),
        ne_lon: bounds.northeast().longitude(),
        se_lat: bounds.southeast().latitude(),
        se_lon: bounds.southeast().longitude(),
        nw_lat: bounds.northwest().latitude(),
        nw_lon: bounds.northwest().longitude(),
        ..BoundsQuery::default()
    };
    if with_center {
        q.center_lat = bounds.center().latitude();
        q.center_lon = bounds.center().longitude();
    }
    let constrained = bounds.constrain(&p);
    q.constrained_lat = constrained.latitude();
    q.constrained_lon = constrained.longitude();
    q.valid = bounds.valid() as u8;
    q.is_empty = bounds.is_empty() as u8;
    q.crosses = bounds.crosses_antimeridian() as u8;
    q.contains_point = bounds.contains(&p) as u8;
    q.contains_point_wrapped = bounds.contains_with_mode(&p, WrapMode::Wrapped) as u8;
    q.contains_area = bounds.contains_bounds(&area, WrapMode::Unwrapped) as u8;
    q.contains_area_wrapped = bounds.contains_bounds(&area, WrapMode::Wrapped) as u8;
    q.intersects = bounds.intersects(area, WrapMode::Unwrapped) as u8;
    q.intersects_wrapped = bounds.intersects(area, WrapMode::Wrapped) as u8;
    q.equal = (bounds == area) as u8;
    q
}

fn same_query(a: &BoundsQuery, b: &BoundsQuery) -> bool {
    let fa = [
        a.sw_lat,
        a.sw_lon,
        a.ne_lat,
        a.ne_lon,
        a.se_lat,
        a.se_lon,
        a.nw_lat,
        a.nw_lon,
        a.center_lat,
        a.center_lon,
        a.constrained_lat,
        a.constrained_lon,
    ];
    let fb = [
        b.sw_lat,
        b.sw_lon,
        b.ne_lat,
        b.ne_lon,
        b.se_lat,
        b.se_lon,
        b.nw_lat,
        b.nw_lon,
        b.center_lat,
        b.center_lon,
        b.constrained_lat,
        b.constrained_lon,
    ];
    (0..fa.len()).all(|i| same_f64(fa[i], fb[i]))
        && (a.valid, a.is_empty, a.crosses, a.contains_point, a.contains_point_wrapped)
            == (b.valid, b.is_empty, b.crosses, b.contains_point, b.contains_point_wrapped)
        && (a.contains_area, a.contains_area_wrapped, a.intersects, a.intersects_wrapped, a.equal)
            == (b.contains_area, b.contains_area_wrapped, b.intersects, b.intersects_wrapped, b.equal)
}

fn spec() -> impl Strategy<Value = BoundsSpec> {
    (0i32..8, lat(), lon(), lat(), lon(), lat(), lon(), 0u32..=20, any::<u32>(), any::<u32>()).prop_map(
        |(kind, a_lat, a_lon, b_lat, b_lon, c_lat, c_lon, z, x, y)| {
            let n = 1u64 << z;
            BoundsSpec {
                kind,
                a_lat,
                a_lon,
                b_lat,
                b_lon,
                c_lat,
                c_lon,
                z,
                x: (x as u64 % n) as u32,
                y: (y as u64 % n) as u32,
            }
        },
    )
}

proptest! {
    #![proptest_config(config(50_000))]

    #[test]
    fn bounds(s in spec(), area in spec(), p_lat in lat(), p_lon in lon()) {
        let r = query(&s, &area, p_lat, p_lon, true);
        let c = cpp::bounds_query(&s, &area, p_lat, p_lon, true);
        prop_assert!(same_query(&r, &c), "bounds {s:?} area {area:?} p ({p_lat}, {p_lon}):\nrust {r:?}\nc++  {c:?}");
    }

    #[test]
    fn edge_insets(
        t in prop_oneof![any::<u64>().prop_map(f64::from_bits), -100.0f64..100.0, Just(0.0)],
        l in prop_oneof![any::<u64>().prop_map(f64::from_bits), -100.0f64..100.0, Just(0.0)],
        b in prop_oneof![any::<u64>().prop_map(f64::from_bits), -100.0f64..100.0, Just(0.0)],
        r in prop_oneof![any::<u64>().prop_map(f64::from_bits), -100.0f64..100.0, Just(0.0)],
        w in any::<u16>(),
        h in any::<u16>(),
    ) {
        let rust = EdgeInsets::new(t, l, b, r).map(|e| {
            let c = e.get_center(w, h);
            ([e.top(), e.left(), e.bottom(), e.right(), c.x, c.y], e.is_flush())
        });
        let c = cpp::edge_insets(t, l, b, r, w, h);
        let same = match (&rust, &c) {
            (Ok((a, fa)), Ok((b, fb))) => fa == fb && (0..6).all(|i| same_f64(a[i], b[i])),
            (Err(a), Err(b)) => a == b,
            _ => false,
        };
        prop_assert!(same, "EdgeInsets({t}, {l}, {b}, {r}) {w}x{h}: rust {rust:?} c++ {c:?}");
    }
}

#[test]
fn bounds_edge_cases() {
    let lats = [-90.0, -45.0, -0.0, 0.0, 45.0, 90.0];
    let lons = [-540.0, -190.0, -180.0, -170.0, -0.0, 0.0, 170.0, 180.0, 190.0, 540.0];
    let mut specs = vec![];
    for kind in 0..8 {
        for &a in &lons {
            for &b in &[-180.0, 0.0, 190.0] {
                specs.push(BoundsSpec {
                    kind,
                    a_lat: -45.0,
                    a_lon: a,
                    b_lat: 45.0,
                    b_lon: b,
                    c_lat: 0.0,
                    c_lon: -0.0,
                    z: 1,
                    x: 1,
                    y: 0,
                });
            }
        }
    }
    let mut n = 0;
    for s in &specs {
        for area in specs.iter().step_by(7) {
            for &p_lat in &lats {
                for &p_lon in &lons {
                    let r = query(s, area, p_lat, p_lon, true);
                    let c = cpp::bounds_query(s, area, p_lat, p_lon, true);
                    assert!(
                        same_query(&r, &c),
                        "bounds {s:?} area {area:?} p ({p_lat}, {p_lon}):\nrust {r:?}\nc++  {c:?}"
                    );
                    n += 1;
                }
            }
        }
    }
    println!("LatLngBounds edge cases: {n}");
}
