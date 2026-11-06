// Difftest of //vendor/earcut against mapbox::earcut on earcut.hpp's own
// fixtures (test/fixtures/*.cpp): exact indices for each fixture as the
// core's point<int16_t> (short fixtures), point<float> and
// std::pair<double, double>.

use earcut_test_fixtures::{CoordType, load_all_in};
use geometry::Point;

/// earcut.hpp's fixtures, from the C++ module in the runfiles
/// (@maplibre//vendor:earcut_fixtures).
fn earcut_fixtures() -> Vec<earcut_test_fixtures::Fixture> {
    let root =
        std::env::var("RUNFILES_DIR").or_else(|_| std::env::var("TEST_SRCDIR")).expect("RUNFILES_DIR or TEST_SRCDIR");
    load_all_in(&std::path::Path::new(&root).join("maplibre+/vendor/earcut.hpp/test/fixtures"))
}

unsafe extern "C" {
    fn ref_earcut_i16(xy: *const i16, sizes: *const u32, rings: usize, out: *mut u32, cap: usize) -> usize;
    fn ref_earcut_f32(xy: *const f32, sizes: *const u32, rings: usize, out: *mut u32, cap: usize) -> usize;
    fn ref_earcut_f64(xy: *const f64, sizes: *const u32, rings: usize, out: *mut u32, cap: usize) -> usize;
}

type RefEarcut<T> = unsafe extern "C" fn(*const T, *const u32, usize, *mut u32, usize) -> usize;

fn reference<T: Copy>(f: RefEarcut<T>, polygon: &[Vec<(T, T)>]) -> Vec<u32> {
    let mut xy = Vec::new();
    let mut sizes = Vec::new();
    for ring in polygon {
        sizes.push(ring.len() as u32);
        for &(x, y) in ring {
            xy.push(x);
            xy.push(y);
        }
    }
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

fn points<T: Copy>(polygon: &[Vec<(T, T)>]) -> Vec<Vec<Point<T>>> {
    polygon.iter().map(|r| r.iter().map(|&(x, y)| Point::new(x, y)).collect()).collect()
}

#[test]
fn fixtures_match_cpp() {
    let fixtures = earcut_fixtures();
    let mut cases = 0;
    for fixture in &fixtures {
        if fixture.coord_type == CoordType::Short {
            let polygon = fixture.short_polygon();
            let actual: Vec<u32> = earcut::earcut(&points(&polygon));
            assert_eq!(actual, reference(ref_earcut_i16, &polygon), "{} as point<int16_t>", fixture.name);
            cases += 1;
        }

        let polygon: Vec<Vec<(f32, f32)>> =
            fixture.polygon.iter().map(|r| r.iter().map(|&(x, y)| (x as f32, y as f32)).collect()).collect();
        let actual: Vec<u32> = earcut::earcut(&points(&polygon));
        assert_eq!(actual, reference(ref_earcut_f32, &polygon), "{} as point<float>", fixture.name);
        cases += 1;

        let actual: Vec<u32> = earcut::earcut(&fixture.polygon);
        assert_eq!(actual, reference(ref_earcut_f64, &fixture.polygon), "{} as pair<double>", fixture.name);
        cases += 1;
    }
    println!("earcut fixtures: {} fixtures, {cases} cases, 0 mismatches", fixtures.len());
}
