// Differential tests of the geometry functions on the MVT path against the
// C++ core: fixupPolygons (and the wagyu clipper under it, for every clip
// and fill type), classifyRings, limitHoles and both convertGeometry
// overloads (tile/geometry_tile_data.cpp).

mod dump;

use std::fmt::Write;

use dump::*;
use geometry::{Geometry, MultiPolygon, Point};
use mln_tile_id::{CanonicalTileID, GeometryCollection, GeometryTileFeature};
use mln_util::geometry::FeatureType;
use proptest::prelude::*;

fn collection(rings: &[Vec<(i16, i16)>]) -> GeometryCollection {
    GeometryCollection::from(
        rings.iter().map(|r| r.iter().map(|&(x, y)| Point::new(x, y)).collect::<Vec<_>>()).collect::<Vec<_>>(),
    )
}

fn rust_fixup(rings: &[Vec<(i16, i16)>]) -> String {
    let mut out = String::new();
    match mln_tile::fixup_polygons(&collection(rings)) {
        Ok(g) => {
            out.push_str("geom ");
            dump::rings(&mut out, &g);
        }
        Err(e) => exception(&mut out, "geom", &e),
    }
    out
}

fn rust_classify(rings: &[Vec<(i16, i16)>]) -> String {
    let mut out = String::new();
    let polygons = mln_tile::classify_rings(&collection(rings));
    let _ = writeln!(out, "polygons {}", polygons.len());
    for p in &polygons {
        out.push_str("geom ");
        dump::rings(&mut out, p);
    }
    out
}

fn rust_limit_holes(rings: &[Vec<(i16, i16)>], max_holes: u32) -> String {
    let mut out = String::new();
    let mut g = collection(rings);
    mln_tile::limit_holes(&mut g, max_holes);
    out.push_str("geom ");
    dump::rings(&mut out, &g);
    out
}

fn rust_wagyu(rings: &[Vec<(i32, i32)>], types: &[u8], clip: u8, sfill: u8, cfill: u8, reverse: bool) -> String {
    use wagyu::{ClipType, FillType, PolygonType, Wagyu};
    let fill = |f: u8| [FillType::EvenOdd, FillType::NonZero, FillType::Positive, FillType::Negative][f as usize];
    let mut out = String::new();
    let mut clipper = Wagyu::new();
    clipper.reverse_rings(reverse);
    let mut added = String::new();
    for (r, &t) in rings.iter().zip(types) {
        let ring: Vec<Point<i32>> = r.iter().map(|&(x, y)| Point::new(x, y)).collect();
        let p_type = if t == 0 { PolygonType::Subject } else { PolygonType::Clip };
        match clipper.add_ring(&ring, p_type) {
            Ok(a) => added.push(if a { '1' } else { '0' }),
            Err(e) => {
                exception(&mut out, "wagyu", &mln_tile::Error::Runtime(e));
                return out;
            }
        }
    }
    let _ = writeln!(out, "added {added}");
    let clip_type = [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::XOr][clip as usize];
    let mut solution: MultiPolygon<i32> = Vec::new();
    match clipper.execute(clip_type, &mut solution, fill(sfill), fill(cfill)) {
        Ok(ok) => {
            let _ = writeln!(out, "{}{}", if ok { "ok " } else { "empty " }, solution.len());
            for polygon in &solution {
                let _ = writeln!(out, "polygon {}", polygon.len());
                for ring in polygon {
                    let _ = write!(out, "r {}", ring.len());
                    for p in ring {
                        let _ = write!(out, " {} {}", p.x, p.y);
                    }
                    out.push('\n');
                }
            }
        }
        Err(e) => {
            out.clear();
            exception(&mut out, "wagyu", &mln_tile::Error::Runtime(e));
        }
    }
    out
}

struct TestFeature {
    type_: FeatureType,
    geometries: GeometryCollection,
}

impl GeometryTileFeature for TestFeature {
    fn get_type(&self) -> FeatureType {
        self.type_
    }
    fn get_value(&self, _key: &str) -> Option<feature::Value> {
        None
    }
    fn get_geometries(&self) -> &GeometryCollection {
        &self.geometries
    }
}

fn latlng(out: &mut String, p: &Point<f64>) {
    out.push(' ');
    bits(out, p.x);
    out.push(' ');
    bits(out, p.y);
}

fn lines(out: &mut String, ls: &[Vec<Point<f64>>]) {
    for l in ls {
        let _ = write!(out, "l {}", l.len());
        for p in l {
            latlng(out, p);
        }
        out.push('\n');
    }
}

fn dump_geometry(out: &mut String, g: &Geometry<f64>) {
    match g {
        Geometry::Point(p) => {
            out.push_str("point");
            latlng(out, p);
            out.push('\n');
        }
        Geometry::MultiPoint(ps) => {
            let _ = write!(out, "multipoint {}", ps.len());
            for p in ps {
                latlng(out, p);
            }
            out.push('\n');
        }
        Geometry::LineString(ps) => {
            let _ = write!(out, "linestring {}", ps.len());
            for p in ps {
                latlng(out, p);
            }
            out.push('\n');
        }
        Geometry::MultiLineString(ls) => {
            let _ = writeln!(out, "multilinestring {}", ls.len());
            lines(out, ls);
        }
        Geometry::Polygon(pg) => {
            let _ = writeln!(out, "polygon {}", pg.len());
            lines(out, pg);
        }
        Geometry::MultiPolygon(mp) => {
            let _ = writeln!(out, "multipolygon {}", mp.len());
            for pg in mp {
                let _ = writeln!(out, "pg {}", pg.len());
                lines(out, pg);
            }
        }
        _ => out.push_str("other\n"),
    }
}

fn rust_convert_geometry(type_: u8, rings: &[Vec<(i16, i16)>], z: u8, x: u32, y: u32) -> String {
    let mut out = String::new();
    let feature = TestFeature {
        type_: [FeatureType::Unknown, FeatureType::Point, FeatureType::LineString, FeatureType::Polygon]
            [type_ as usize],
        geometries: collection(rings),
    };
    match mln_tile::convert_geometry(&feature, &CanonicalTileID::new(z, x, y)) {
        Ok(g) => dump_geometry(&mut out, &g),
        Err(e) => exception(&mut out, "convert", &e),
    }
    out
}

fn rust_convert_geometry_to_tile(kind: u8, rings: &[Vec<(f64, f64)>], z: u8, x: u32, y: u32) -> String {
    let lines: Vec<Vec<Point<f64>>> =
        rings.iter().map(|r| r.iter().map(|&(x, y)| Point::new(x, y)).collect()).collect();
    let g = match kind {
        1 => Geometry::Point(lines[0][0]),
        2 => Geometry::MultiPoint(lines[0].clone()),
        3 => Geometry::LineString(lines[0].clone()),
        4 => Geometry::Polygon(lines.clone()),
        5 => Geometry::MultiPolygon(vec![lines.clone()]),
        _ => Geometry::MultiLineString(lines.clone()),
    };
    let mut out = String::from("geom ");
    dump::rings(&mut out, &mln_tile::convert_geometry_to_tile(&g, &CanonicalTileID::new(z, x, y)));
    out
}

// ---- strategies -----------------------------------------------------------

/// Rings of i16 points: small grids (many duplicates, collinear points and
/// intersections), tile-sized and extreme coordinates.
fn rings_strategy(max_rings: usize, max_points: usize) -> impl Strategy<Value = Vec<Vec<(i16, i16)>>> {
    let coord = prop_oneof![
        4 => (-4i16..5, -4i16..5),
        3 => (-20i16..21, -20i16..21),
        2 => (-200i16..8400, -200i16..8400),
        1 => (any::<i16>(), any::<i16>()),
    ];
    let ring = proptest::collection::vec(coord, 0..max_points).prop_map(|mut r| {
        // Mostly closed rings, as the decoder produces.
        if r.len() > 2 && r.len() % 3 != 0 {
            let first = r[0];
            r.push(first);
        }
        r
    });
    proptest::collection::vec(ring, 0..max_rings)
}

fn rings32_strategy() -> impl Strategy<Value = (Vec<Vec<(i32, i32)>>, Vec<u8>)> {
    rings_strategy(6, 12).prop_flat_map(|rings| {
        let n = rings.len();
        let rings32: Vec<Vec<(i32, i32)>> =
            rings.into_iter().map(|r| r.into_iter().map(|(x, y)| (i32::from(x), i32::from(y))).collect()).collect();
        (Just(rings32), proptest::collection::vec(0u8..2, n))
    })
}

fn config(cases: u32) -> ProptestConfig {
    ProptestConfig { cases, failure_persistence: None, ..ProptestConfig::default() }
}

proptest! {
    #![proptest_config(config(20000))]

    #[test]
    fn fixup_polygons_matches(rings in rings_strategy(6, 14)) {
        prop_assert_eq!(rust_fixup(&rings), reference_fixup_polygons(&rings));
    }
}

proptest! {
    #![proptest_config(config(10000))]

    #[test]
    fn wagyu_matches(
        (rings, types) in rings32_strategy(),
        clip in 0u8..4,
        sfill in 0u8..4,
        cfill in 0u8..4,
        reverse in any::<bool>(),
    ) {
        prop_assert_eq!(
            rust_wagyu(&rings, &types, clip, sfill, cfill, reverse),
            reference_wagyu(&rings, &types, clip, sfill, cfill, reverse)
        );
    }

    #[test]
    fn classify_rings_matches(rings in rings_strategy(8, 10)) {
        prop_assert_eq!(rust_classify(&rings), reference_classify_rings(&rings));
    }

    #[test]
    fn limit_holes_matches(rings in rings_strategy(40, 8), max_holes in 0u32..12) {
        prop_assert_eq!(rust_limit_holes(&rings, max_holes), reference_limit_holes(&rings, max_holes));
    }

    #[test]
    fn convert_geometry_matches(
        type_ in 1u8..4,
        rings in rings_strategy(4, 8),
        z in 0u8..23,
        fx in 0.0f64..1.0,
        fy in 0.0f64..1.0,
    ) {
        let n = 1u64 << z;
        let x = ((fx * n as f64) as u64).min(n - 1) as u32;
        let y = ((fy * n as f64) as u64).min(n - 1) as u32;
        prop_assert_eq!(rust_convert_geometry(type_, &rings, z, x, y), reference_convert_geometry(type_, &rings, z, x, y));
    }

    #[test]
    fn convert_geometry_to_tile_matches(
        kind in 0u8..6,
        rings in proptest::collection::vec(
            proptest::collection::vec(
                prop_oneof![
                    3 => (-180.0f64..180.0, -85.06f64..85.06),
                    1 => (-400.0f64..400.0, -95.0f64..95.0),
                    1 => (any::<f64>(), any::<f64>()),
                ],
                1..6,
            ),
            1..4,
        ),
        z in 0u8..23,
        fx in 0.0f64..1.0,
        fy in 0.0f64..1.0,
    ) {
        let n = 1u64 << z;
        let x = ((fx * n as f64) as u64).min(n - 1) as u32;
        let y = ((fy * n as f64) as u64).min(n - 1) as u32;
        prop_assert_eq!(
            rust_convert_geometry_to_tile(kind, &rings, z, x, y),
            reference_convert_geometry_to_tile(kind, &rings, z, x, y)
        );
    }
}

#[test]
fn edge_cases() {
    let cases: Vec<Vec<Vec<(i16, i16)>>> = vec![
        vec![],
        vec![vec![]],
        vec![vec![(0, 0)]],
        vec![vec![(0, 0), (0, 0), (0, 0), (0, 0)]],
        vec![vec![(0, 0), (10, 0), (10, 10), (0, 10), (0, 0)]],
        vec![vec![(0, 0), (10, 10), (10, 0), (0, 10), (0, 0)]],
        vec![vec![(0, 0), (10, 0), (10, 10), (0, 10), (0, 0)], vec![(2, 2), (8, 2), (8, 8), (2, 8), (2, 2)]],
        vec![vec![(0, 0), (10, 0), (10, 10), (0, 10), (0, 0)], vec![(0, 0), (10, 0), (10, 10), (0, 10), (0, 0)]],
        vec![vec![(-32768, -32768), (32767, -32768), (32767, 32767), (-32768, 32767), (-32768, -32768)]],
        vec![vec![(-32768, 0), (32767, 1), (-32768, 2), (32767, 3), (-32768, 0)]],
        vec![vec![(0, 0), (5, 0), (5, 5), (0, 5), (0, 0), (5, 5), (10, 5), (10, 10), (5, 10), (5, 5)]],
    ];
    for rings in &cases {
        assert_eq!(rust_fixup(rings), reference_fixup_polygons(rings), "fixup {rings:?}");
        assert_eq!(rust_classify(rings), reference_classify_rings(rings), "classify {rings:?}");
        for max_holes in 0..3 {
            assert_eq!(rust_limit_holes(rings, max_holes), reference_limit_holes(rings, max_holes), "limit {rings:?}");
        }
        for type_ in 1..4 {
            for (z, x, y) in [(0, 0, 0), (14, 8802, 5374), (22, 4194303, 0)] {
                assert_eq!(
                    rust_convert_geometry(type_, rings, z, x, y),
                    reference_convert_geometry(type_, rings, z, x, y),
                    "convert {type_} {rings:?}"
                );
            }
        }
    }
    let nan = vec![vec![(f64::NAN, 0.0), (0.0, f64::NAN), (f64::INFINITY, -f64::INFINITY), (1e300, -1e300)]];
    for kind in 0..6 {
        assert_eq!(
            rust_convert_geometry_to_tile(kind, &nan, 3, 2, 1),
            reference_convert_geometry_to_tile(kind, &nan, 3, 2, 1)
        );
    }
}

/// How much of wagyu the random inputs reach (printed with --nocapture).
#[test]
fn coverage_report() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    let strategy = rings_strategy(6, 14);
    let (mut nonempty, mut multi, mut holes, mut errors) = (0, 0, 0, 0);
    let n = 5000;
    for _ in 0..n {
        let rings = strategy.new_tree(&mut runner).unwrap().current();
        let cpp = reference_fixup_polygons(&rings);
        assert_eq!(rust_fixup(&rings), cpp);
        if cpp.starts_with("geom!") {
            errors += 1;
            continue;
        }
        let rings_out: usize = cpp.lines().next().unwrap()[5..].parse().unwrap();
        if rings_out > 0 {
            nonempty += 1;
        }
        let polygons = mln_tile::classify_rings(&mln_tile::fixup_polygons(&collection(&rings)).unwrap());
        if polygons.len() > 1 {
            multi += 1;
        }
        if polygons.iter().any(|p| p.len() > 1) {
            holes += 1;
        }
    }
    eprintln!(
        "fixupPolygons coverage: {n} cases, {nonempty} with output rings, {multi} with several polygons, \
         {holes} with holes, {errors} wagyu exceptions"
    );
    assert!(nonempty > n / 4);
}
