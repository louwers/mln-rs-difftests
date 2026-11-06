// Difftest of //vendor/polylabel against mapbox::polylabel (misc_reference.cpp):
// the exact pole of inaccessibility (bit patterns) for star polygons with
// holes, degenerate grid polygons, and tile-sized polygons at the core's
// precision (EXTENT / tileSize = 16) and others.

use geometry::Point;
use geometry_difftest_support::{Polygon, flatten, grid_polygon, run_cases, star_polygon};
use proptest::prelude::*;
use proptest::test_runner::TestCaseResult;

unsafe extern "C" {
    fn ref_polylabel(xy: *const f64, sizes: *const u32, rings: usize, precision: f64, out: *mut f64) -> bool;
}

fn cpp(polygon: &Polygon<f64>, precision: f64) -> Option<(u64, u64)> {
    let (xy, sizes) = flatten(polygon);
    let mut out = [0.0f64; 2];
    let ok = unsafe { ref_polylabel(xy.as_ptr(), sizes.as_ptr(), sizes.len(), precision, out.as_mut_ptr()) };
    ok.then(|| (out[0].to_bits(), out[1].to_bits()))
}

fn rust(polygon: &Polygon<f64>, precision: f64) -> Option<(u64, u64)> {
    let polygon: geometry::Polygon<f64> =
        polygon.iter().map(|r| r.iter().map(|&(x, y)| Point::new(x, y)).collect()).collect();
    polylabel::polylabel(&polygon, precision).ok().map(|p| (p.x.to_bits(), p.y.to_bits()))
}

fn check(polygon: &Polygon<f64>, precision: f64) -> TestCaseResult {
    prop_assert_eq!(rust(polygon, precision), cpp(polygon, precision));
    Ok(())
}

#[test]
fn star_polygons() {
    let strategy = (star_polygon(3..=60, 4, 4096.0), prop::sample::select(vec![16.0, 4.0, 50.0]));
    let cases = run_cases(3000, &strategy, |(p, precision)| check(&p, precision));
    println!("polylabel star polygons: {cases} cases, 0 mismatches");
}

#[test]
fn grid_polygons() {
    let strategy = (grid_polygon(4), prop::sample::select(vec![1.0, 0.5, 16.0, 100.0]));
    let cases = run_cases(10_000, &strategy, |(p, precision)| check(&p, precision));
    println!("polylabel grid polygons: {cases} cases, 0 mismatches");
}

#[test]
fn edge_cases() {
    let cases: Vec<Polygon<f64>> = vec![
        vec![],
        vec![vec![]],
        vec![vec![(1.0, 2.0)]],
        vec![vec![(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (0.0, 0.0)]],
        vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)]],
        vec![vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]],
        vec![vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)], vec![]],
    ];
    for polygon in &cases {
        for precision in [1.0, 16.0] {
            check(polygon, precision).unwrap();
        }
    }
    println!("polylabel edge cases: {} cases, 0 mismatches", cases.len() * 2);
}
