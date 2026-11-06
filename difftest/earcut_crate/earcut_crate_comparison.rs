// Compares the georust `earcut` crate (`earcut_crate`) with //vendor/earcut
// (`earcut`, the exact port of the C++ earcut.hpp) on the same polygons.
// It reports three levels of agreement instead of failing on the first
// difference: identical index lists, the same set of triangles in any order,
// and the same triangulated area.

use earcut_test_fixtures::load_all_in;

/// earcut.hpp's fixtures, from the C++ module in the runfiles
/// (@maplibre//vendor:earcut_fixtures).
fn earcut_fixtures() -> Vec<earcut_test_fixtures::Fixture> {
    let root =
        std::env::var("RUNFILES_DIR").or_else(|_| std::env::var("TEST_SRCDIR")).expect("RUNFILES_DIR or TEST_SRCDIR");
    load_all_in(&std::path::Path::new(&root).join("maplibre+/vendor/earcut.hpp/test/fixtures"))
}

type Rings = Vec<Vec<(f64, f64)>>;

fn port(rings: &Rings) -> Vec<u32> {
    earcut::earcut::<u32, _, _>(rings)
}

fn krate(rings: &Rings) -> Vec<u32> {
    let mut holes = Vec::new();
    let mut n = 0u32;
    for (i, ring) in rings.iter().enumerate() {
        if i > 0 {
            holes.push(n);
        }
        n += ring.len() as u32;
    }
    let data = rings.iter().flatten().map(|&(x, y)| [x, y]);
    let mut out = Vec::new();
    earcut_crate::Earcut::<f64>::new().earcut(data, &holes, &mut out);
    out
}

fn triangle_set(indices: &[u32]) -> Vec<[u32; 3]> {
    let mut tris: Vec<[u32; 3]> = indices
        .chunks_exact(3)
        .map(|t| {
            // Same triangle regardless of starting vertex (winding kept).
            let r = (0..3).min_by_key(|&k| t[k]).unwrap();
            [t[r], t[(r + 1) % 3], t[(r + 2) % 3]]
        })
        .collect();
    tris.sort();
    tris
}

fn area(rings: &Rings, indices: &[u32]) -> f64 {
    let v: Vec<(f64, f64)> = rings.iter().flatten().copied().collect();
    indices
        .chunks_exact(3)
        .map(|t| {
            let (a, b, c) = (v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]);
            ((a.0 - c.0) * (b.1 - a.1) - (a.0 - b.0) * (c.1 - a.1)).abs() / 2.0
        })
        .sum()
}

#[derive(Default)]
struct Stats {
    cases: usize,
    identical: usize,
    same_triangles: usize,
    same_area: usize,
    first_differences: Vec<String>,
}

impl Stats {
    fn add(&mut self, name: &str, rings: &Rings) {
        let (p, k) = (port(rings), krate(rings));
        self.cases += 1;
        if p == k {
            self.identical += 1;
        } else if self.first_differences.len() < 5 {
            self.first_differences.push(format!(
                "{name}: port {} triangles, crate {} triangles",
                p.len() / 3,
                k.len() / 3
            ));
        }
        if triangle_set(&p) == triangle_set(&k) {
            self.same_triangles += 1;
        }
        let (ap, ak) = (area(rings, &p), area(rings, &k));
        if (ap - ak).abs() <= 1e-9 * ap.abs().max(1.0) {
            self.same_area += 1;
        }
    }

    fn report(&self, label: &str) {
        println!(
            "{label}: {} cases, identical {}, same triangle set {}, same area {}",
            self.cases, self.identical, self.same_triangles, self.same_area
        );
        for d in &self.first_differences {
            println!("  e.g. {d}");
        }
    }
}

// xorshift64*: deterministic inputs without extra dependencies.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

// A star-shaped ring around (cx, cy): realistic, simple polygons.
fn star(rng: &mut Rng, cx: f64, cy: f64, r_min: f64, r_max: f64, n: usize, clockwise: bool) -> Vec<(f64, f64)> {
    let mut angles: Vec<f64> = (0..n).map(|_| rng.range(0.0, std::f64::consts::TAU)).collect();
    angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if clockwise {
        angles.reverse();
    }
    angles
        .into_iter()
        .map(|a| {
            let r = rng.range(r_min, r_max);
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

#[test]
fn compare() {
    let mut fixtures = Stats::default();
    for f in earcut_fixtures() {
        fixtures.add(&f.name, &f.polygon);
    }
    fixtures.report("earcut.hpp fixtures");

    let mut simple = Stats::default();
    let mut rng = Rng(0x9E3779B97F4A7C15);
    for i in 0..20_000 {
        let n = 3 + (rng.next() * 60.0) as usize;
        let mut rings = vec![star(&mut rng, 0.0, 0.0, 400.0, 1000.0, n, false)];
        let holes = (rng.next() * 4.0) as usize;
        for h in 0..holes {
            let a = h as f64 * 1.7;
            let m = 3 + (rng.next() * 12.0) as usize;
            rings.push(star(&mut rng, 200.0 * a.cos(), 200.0 * a.sin(), 20.0, 90.0, m, true));
        }
        simple.add(&format!("star #{i}"), &rings);
    }
    simple.report("generated simple polygons with holes");

    let mut messy = Stats::default();
    for i in 0..20_000 {
        let n = 3 + (rng.next() * 40.0) as usize;
        let ring: Vec<(f64, f64)> = (0..n).map(|_| (rng.range(-1000.0, 1000.0), rng.range(-1000.0, 1000.0))).collect();
        messy.add(&format!("random ring #{i}"), &vec![ring]);
    }
    messy.report("generated self-intersecting rings");
}
