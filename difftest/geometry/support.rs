// Shared by the //difftest/geometry tests: a deterministic proptest runner
// that counts cases, and polygon strategies (random rings with holes and
// degeneracies) used by the earcut and wagyu difftests.

use std::cell::Cell;

use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseResult, TestRng, TestRunner};

/// Runs `test` on `cases` inputs from `strategy` with a fixed seed (so case
/// counts and failures reproduce) and returns the number of cases run.
/// Panics with proptest's minimized input on the first mismatch.
pub fn run_cases<S: Strategy>(cases: u32, strategy: &S, test: impl Fn(S::Value) -> TestCaseResult) -> u32
where
    S::Value: std::fmt::Debug,
{
    let count = Cell::new(0u32);
    let mut runner = TestRunner::new_with_rng(
        Config { cases, failure_persistence: None, max_shrink_iters: 4096, ..Config::default() },
        TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    );
    let result = runner.run(strategy, |value| {
        count.set(count.get() + 1);
        test(value)
    });
    if let Err(e) = result {
        panic!("{e}");
    }
    count.get()
}

/// Directory of `@maplibre` files in the test's runfiles.
pub fn maplibre_runfile(path: &str) -> std::path::PathBuf {
    let root = std::env::var_os("RUNFILES_DIR")
        .or_else(|| std::env::var_os("TEST_SRCDIR"))
        .expect("RUNFILES_DIR or TEST_SRCDIR must be set");
    std::path::Path::new(&root).join("maplibre+").join(path)
}

pub type Ring<T> = Vec<(T, T)>;
pub type Polygon<T> = Vec<Ring<T>>;

/// Rings of up to 24 points on a small grid: collinear runs, duplicate
/// points, self-intersections, zero-area rings, touching holes.
pub fn grid_polygon(max_rings: usize) -> impl Strategy<Value = Polygon<f64>> {
    (1i32..=12, prop::sample::select(vec![1.0f64, 3.0, 128.0, 0.5]), -64i32..=64).prop_flat_map(
        move |(size, scale, offset)| {
            let point = (0..=size, 0..=size)
                .prop_map(move |(x, y)| (f64::from(x + offset) * scale, f64::from(y + offset) * scale));
            let ring = (prop::collection::vec(point, 0..=24), any::<bool>()).prop_map(|(mut ring, close)| {
                if close && !ring.is_empty() {
                    ring.push(ring[0]);
                }
                ring
            });
            prop::collection::vec(ring, 1..=max_rings)
        },
    )
}

/// A star-shaped ring (simple unless radii are degenerate) with `n` points
/// around `(cx, cy)`.
fn star_ring(n: std::ops::RangeInclusive<usize>, max_radius: f64) -> impl Strategy<Value = Ring<f64>> {
    (prop::collection::vec((0.0f64..std::f64::consts::TAU, 0.05f64..=1.0), n), any::<bool>(), any::<bool>()).prop_map(
        move |(mut spokes, clockwise, close)| {
            spokes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            if clockwise {
                spokes.reverse();
            }
            let mut ring: Ring<f64> =
                spokes.iter().map(|(angle, r)| (angle.cos() * r * max_radius, angle.sin() * r * max_radius)).collect();
            if close && !ring.is_empty() {
                ring.push(ring[0]);
            }
            ring
        },
    )
}

/// Star-shaped outer rings with up to `max_holes` star-shaped holes near the
/// centre (holes may overlap each other or the outer ring), `points` per
/// outer ring. Coordinates are around `(cx, cy)` within `radius`.
pub fn star_polygon(
    points: std::ops::RangeInclusive<usize>,
    max_holes: usize,
    radius: f64,
) -> impl Strategy<Value = Polygon<f64>> {
    let hole = (star_ring(3..=24, radius * 0.3), -0.4f64..0.4, -0.4f64..0.4).prop_map(move |(ring, dx, dy)| {
        ring.into_iter().map(|(x, y)| (x + dx * radius, y + dy * radius)).collect::<Ring<f64>>()
    });
    (star_ring(points, radius), prop::collection::vec(hole, 0..=max_holes), -1.0f64..1.0, -1.0f64..1.0).prop_map(
        move |(outer, holes, cx, cy)| {
            let mut polygon = vec![outer];
            polygon.extend(holes);
            for ring in &mut polygon {
                for p in ring.iter_mut() {
                    p.0 += cx * radius;
                    p.1 += cy * radius;
                }
            }
            polygon
        },
    )
}

/// Rounds a polygon to `i16` coordinates (clamped), as tile geometry is.
pub fn to_i16(polygon: &Polygon<f64>) -> Polygon<i16> {
    polygon
        .iter()
        .map(|r| {
            r.iter()
                .map(|&(x, y)| (x.round().clamp(-32768.0, 32767.0) as i16, y.round().clamp(-32768.0, 32767.0) as i16))
                .collect()
        })
        .collect()
}

pub fn to_f32(polygon: &Polygon<f64>) -> Polygon<f32> {
    polygon.iter().map(|r| r.iter().map(|&(x, y)| (x as f32, y as f32)).collect()).collect()
}

/// `(flat xy, ring sizes)` for the C++ side.
pub fn flatten<T: Copy>(polygon: &Polygon<T>) -> (Vec<T>, Vec<u32>) {
    let mut xy = Vec::new();
    let mut sizes = Vec::new();
    for ring in polygon {
        sizes.push(ring.len() as u32);
        for &(x, y) in ring {
            xy.push(x);
            xy.push(y);
        }
    }
    (xy, sizes)
}
