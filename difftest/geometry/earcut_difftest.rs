// Difftest of //vendor/earcut against mapbox::earcut (earcut_reference.cpp):
// the exact triangle indices, in order, for the point types the core uses
// (point<int16_t> read as int64_t, point<float>) and std::pair<double,
// double>, on proptest-generated polygons (grid rings with collinear and
// duplicate points and self-intersections, star polygons with holes,
// polygons large enough for z-order hashing, extreme coordinates).
// earcut_fixtures_difftest.rs covers earcut.hpp's own fixtures.

use geometry::Point;
use geometry_difftest_support::{Polygon, flatten, grid_polygon, run_cases, star_polygon, to_f32, to_i16};
use proptest::prelude::*;
use proptest::test_runner::TestCaseResult;

unsafe extern "C" {
    fn ref_earcut_i16(xy: *const i16, sizes: *const u32, rings: usize, out: *mut u32, cap: usize) -> usize;
    fn ref_earcut_f32(xy: *const f32, sizes: *const u32, rings: usize, out: *mut u32, cap: usize) -> usize;
    fn ref_earcut_f64(xy: *const f64, sizes: *const u32, rings: usize, out: *mut u32, cap: usize) -> usize;
}

type RefEarcut<T> = unsafe extern "C" fn(*const T, *const u32, usize, *mut u32, usize) -> usize;

fn reference<T: Copy>(f: RefEarcut<T>, polygon: &Polygon<T>) -> Vec<u32> {
    let (xy, sizes) = flatten(polygon);
    let mut out = vec![0u32; 3 * (xy.len() + 4 * sizes.len()) + 16];
    loop {
        let n = unsafe { f(xy.as_ptr(), sizes.as_ptr(), sizes.len(), out.as_mut_ptr(), out.len()) };
        if n <= out.len() {
            out.truncate(n);
            return out;
        }
        out.resize(n, 0);
    }
}

fn points<T: Copy>(polygon: &Polygon<T>) -> Vec<Vec<Point<T>>> {
    polygon.iter().map(|r| r.iter().map(|&(x, y)| Point::new(x, y)).collect()).collect()
}

fn check_i16(polygon: &Polygon<i16>) -> TestCaseResult {
    let expected = reference(ref_earcut_i16, polygon);
    let actual: Vec<u32> = earcut::earcut(&points(polygon));
    prop_assert_eq!(actual, expected);
    Ok(())
}

fn check_f32(polygon: &Polygon<f32>) -> TestCaseResult {
    let expected = reference(ref_earcut_f32, polygon);
    let actual: Vec<u32> = earcut::earcut(&points(polygon));
    prop_assert_eq!(actual, expected);
    Ok(())
}

fn check_f64(polygon: &Polygon<f64>) -> TestCaseResult {
    let expected = reference(ref_earcut_f64, polygon);
    let actual: Vec<u32> = earcut::earcut(polygon);
    prop_assert_eq!(actual, expected);
    Ok(())
}

/// Every representation of the same polygon.
fn check_all(polygon: &Polygon<f64>) -> TestCaseResult {
    check_i16(&to_i16(polygon))?;
    check_f32(&to_f32(polygon))?;
    check_f64(polygon)
}

/// Moves ring `r` to `x * 8 + r` (and `y * 8 + 3r mod 8`), so that no point
/// occurs in two different rings while the rings keep overlapping.
///
/// Holes that share points with other rings can make earcut.hpp loop
/// forever: `eliminateHoles` passes `outerNode` to `filterPoints` after the
/// previous bridge's `filterPoints` may already have unlinked it (C++ and
/// the port both hang). The core only triangulates wagyu-fixed polygons
/// (`fixupPolygons`), which never have such holes; single-ring inputs and
/// the fixtures cover shared points otherwise.
fn separate_rings(polygon: Polygon<f64>) -> Polygon<f64> {
    polygon
        .into_iter()
        .enumerate()
        .map(|(r, ring)| {
            let (dx, dy) = ((r % 8) as f64, ((3 * r) % 8) as f64);
            ring.into_iter().map(|(x, y)| (x * 8.0 + dx, y * 8.0 + dy)).collect()
        })
        .collect()
}

#[test]
fn grid_rings() {
    let cases = run_cases(20_000, &grid_polygon(1), |p| check_all(&p));
    println!("earcut grid rings: {cases} cases x 3 point types, 0 mismatches");
}

#[test]
fn grid_polygons() {
    let strategy = grid_polygon(6).prop_map(separate_rings);
    let cases = run_cases(10_000, &strategy, |p| check_all(&p));
    println!("earcut grid polygons (disjoint rings): {cases} cases x 3 point types, 0 mismatches");
}

#[test]
fn star_polygons_with_holes() {
    let cases = run_cases(5_000, &star_polygon(3..=60, 6, 4096.0), |p| check_all(&p));
    println!("earcut star polygons: {cases} cases x 3 point types, 0 mismatches");
}

#[test]
fn hashed_polygons() {
    // More than 80 points: the z-order hashed ear test.
    let cases = run_cases(1_500, &star_polygon(81..=1200, 12, 8192.0), |p| check_all(&p));
    println!("earcut hashed polygons: {cases} cases x 3 point types, 0 mismatches");
}

#[test]
fn hashed_grid_polygons() {
    // Hashing over degenerate grid rings.
    let strategy = prop::collection::vec(grid_polygon(1), 4..=12)
        .prop_map(|parts| {
            let mut polygon: Polygon<f64> = Vec::new();
            for part in parts {
                polygon.extend(part);
            }
            polygon
        })
        .prop_map(separate_rings);
    let cases = run_cases(5_000, &strategy, |p| check_all(&p));
    println!("earcut hashed grid polygons: {cases} cases x 3 point types, 0 mismatches");
}

#[test]
fn extreme_coordinates() {
    let strategy = prop_oneof![
        star_polygon(3..=200, 4, 32767.0),
        grid_polygon(4)
            .prop_map(|p| {
                p.into_iter().map(|r| r.into_iter().map(|(x, y)| (x * 300.0, y * 300.0)).collect()).collect()
            })
            .prop_map(separate_rings),
    ];
    let cases = run_cases(5_000, &strategy, |p| check_all(&p));
    println!("earcut extreme coordinates: {cases} cases x 3 point types, 0 mismatches");
}

#[test]
fn edge_cases() {
    let cases: Vec<Polygon<f64>> = vec![
        vec![],
        vec![vec![]],
        vec![vec![(0.0, 0.0)]],
        vec![vec![(0.0, 0.0), (1.0, 1.0)]],
        vec![vec![(0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]],
        vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]],
        vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], vec![]],
        vec![vec![], vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]],
        vec![vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], vec![(5.0, 5.0)]],
        vec![
            vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
            vec![(2.0, 2.0), (4.0, 2.0), (4.0, 4.0)],
            vec![(2.0, 2.0), (4.0, 2.0), (4.0, 4.0)],
        ],
        vec![vec![(-0.0, 0.0), (0.0, -0.0), (1.0, 0.0), (0.0, 1.0)]],
    ];
    for polygon in &cases {
        check_all(polygon).unwrap();
    }
    println!("earcut edge cases: {} cases x 3 point types, 0 mismatches", cases.len());
}
