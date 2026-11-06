// Difftest of //vendor/cheap-ruler against mapbox::cheap_ruler::CheapRuler
// (misc_reference.cpp): every method, bit-exact, for rulers at random
// latitudes and units (and fromTile), on random points (city-scale and
// across the antimeridian), lines and polygons, plus std::remainder (the
// port's own exact implementation) on random and special doubles.

use cheap_ruler::{Box, CheapRuler, Unit};
use geometry::Point;
use geometry_difftest_support::run_cases;
use proptest::prelude::*;
use serde_json::{Value as Json, json};

unsafe extern "C" {
    fn ref_geojson_free(p: *mut std::ffi::c_char);
    fn ref_cheap_ruler(text: *const u8, len: usize) -> *mut std::ffi::c_char;
}

fn cpp(request: &Json) -> Json {
    let text = request.to_string();
    unsafe {
        let p = ref_cheap_ruler(text.as_ptr(), text.len());
        let s = std::ffi::CStr::from_ptr(p).to_str().unwrap().to_owned();
        ref_geojson_free(p);
        serde_json::from_str(&s).unwrap()
    }
}

const UNITS: [Unit; 7] =
    [Unit::Kilometers, Unit::Miles, Unit::NauticalMiles, Unit::Meters, Unit::Yards, Unit::Feet, Unit::Inches];

fn d(v: f64) -> Json {
    json!(v.to_bits())
}

fn pj(p: Point<f64>) -> Json {
    json!([d(p.x), d(p.y)])
}

fn lj(l: &[Point<f64>]) -> Json {
    Json::Array(l.iter().map(|p| pj(*p)).collect())
}

fn bj(b: Box) -> Json {
    json!([pj(b.min), pj(b.max)])
}

#[derive(Clone, Debug)]
enum Op {
    Distance(Point<f64>, Point<f64>),
    SquareDistance(Point<f64>, Point<f64>),
    Bearing(Point<f64>, Point<f64>),
    Destination(Point<f64>, f64, f64),
    Offset(Point<f64>, f64, f64),
    LineDistance(Vec<Point<f64>>),
    Area(Vec<Vec<Point<f64>>>),
    Along(Vec<Point<f64>>, f64),
    PointToSegmentDistance(Point<f64>, Point<f64>, Point<f64>),
    PointOnLine(Vec<Point<f64>>, Point<f64>),
    LineSlice(Point<f64>, Point<f64>, Vec<Point<f64>>),
    LineSliceAlong(f64, f64, Vec<Point<f64>>),
    BufferPoint(Point<f64>, f64),
    BufferBBox(Box, f64),
    InsideBBox(Point<f64>, Box),
    Interpolate(Point<f64>, Point<f64>, f64),
}

impl Op {
    fn request(&self) -> Json {
        match self {
            Op::Distance(a, b) => json!(["distance", pj(*a), pj(*b)]),
            Op::SquareDistance(a, b) => json!(["squareDistance", pj(*a), pj(*b)]),
            Op::Bearing(a, b) => json!(["bearing", pj(*a), pj(*b)]),
            Op::Destination(o, dist, bearing) => json!(["destination", pj(*o), d(*dist), d(*bearing)]),
            Op::Offset(o, dx, dy) => json!(["offset", pj(*o), d(*dx), d(*dy)]),
            Op::LineDistance(l) => json!(["lineDistance", lj(l)]),
            Op::Area(poly) => json!(["area", Json::Array(poly.iter().map(|r| lj(r)).collect())]),
            Op::Along(l, dist) => json!(["along", lj(l), d(*dist)]),
            Op::PointToSegmentDistance(p, a, b) => json!(["pointToSegmentDistance", pj(*p), pj(*a), pj(*b)]),
            Op::PointOnLine(l, p) => json!(["pointOnLine", lj(l), pj(*p)]),
            Op::LineSlice(a, b, l) => json!(["lineSlice", pj(*a), pj(*b), lj(l)]),
            Op::LineSliceAlong(a, b, l) => json!(["lineSliceAlong", d(*a), d(*b), lj(l)]),
            Op::BufferPoint(p, b) => json!(["bufferPoint", pj(*p), d(*b)]),
            Op::BufferBBox(bx, b) => json!(["bufferBBox", bj(*bx), d(*b)]),
            Op::InsideBBox(p, bx) => json!(["insideBBox", pj(*p), bj(*bx)]),
            Op::Interpolate(a, b, t) => json!(["interpolate", pj(*a), pj(*b), d(*t)]),
        }
    }

    fn rust(&self, ruler: &CheapRuler) -> Json {
        match self {
            Op::Distance(a, b) => d(ruler.distance(*a, *b)),
            Op::SquareDistance(a, b) => d(ruler.square_distance(*a, *b)),
            Op::Bearing(a, b) => d(ruler.bearing(*a, *b)),
            Op::Destination(o, dist, bearing) => pj(ruler.destination(*o, *dist, *bearing)),
            Op::Offset(o, dx, dy) => pj(ruler.offset(*o, *dx, *dy)),
            Op::LineDistance(l) => d(ruler.line_distance(l)),
            Op::Area(poly) => d(ruler.area(poly)),
            Op::Along(l, dist) => pj(ruler.along(l, *dist)),
            Op::PointToSegmentDistance(p, a, b) => d(ruler.point_to_segment_distance(p, a, b)),
            Op::PointOnLine(l, p) => {
                let (q, i, t) = ruler.point_on_line(l, *p);
                json!([pj(q), i, d(t)])
            }
            Op::LineSlice(a, b, l) => lj(&ruler.line_slice(*a, *b, l)),
            Op::LineSliceAlong(a, b, l) => lj(&ruler.line_slice_along(*a, *b, l)),
            Op::BufferPoint(p, b) => bj(ruler.buffer_point(*p, *b)),
            Op::BufferBBox(bx, b) => bj(ruler.buffer_bbox(*bx, *b)),
            Op::InsideBBox(p, bx) => json!(CheapRuler::inside_bbox(*p, *bx)),
            Op::Interpolate(a, b, t) => pj(CheapRuler::interpolate(*a, *b, *t)),
        }
    }
}

/// Points near `(cx, cy)`: city-scale offsets, exact duplicates, and the
/// other side of the antimeridian.
fn point(cx: f64, cy: f64) -> impl Strategy<Value = Point<f64>> {
    prop_oneof![
        6 => (-0.05f64..0.05, -0.05f64..0.05).prop_map(move |(dx, dy)| Point::new(cx + dx, cy + dy)),
        1 => Just(Point::new(cx, cy)),
        1 => (-0.05f64..0.05, prop::sample::select(vec![360.0, -360.0, 180.0])).prop_map(move |(dy, s)| Point::new(cx + s, cy + dy)),
        1 => (-180.0f64..180.0, -85.0f64..85.0).prop_map(|(x, y)| Point::new(x, y)),
    ]
}

fn line(cx: f64, cy: f64, min: usize) -> impl Strategy<Value = Vec<Point<f64>>> {
    prop::collection::vec(point(cx, cy), min..=12)
}

fn op(cx: f64, cy: f64) -> impl Strategy<Value = Op> {
    let p = move || point(cx, cy);
    let dist = prop_oneof![-1.0f64..20.0, Just(0.0), Just(-0.0)];
    let bx = move || (p(), p()).prop_map(|(a, b)| Box::new(a, b));
    prop_oneof![
        (p(), p()).prop_map(|(a, b)| Op::Distance(a, b)),
        (p(), p()).prop_map(|(a, b)| Op::SquareDistance(a, b)),
        (p(), p()).prop_map(|(a, b)| Op::Bearing(a, b)),
        (p(), -10.0f64..10.0, -400.0f64..400.0).prop_map(|(o, d, b)| Op::Destination(o, d, b)),
        (p(), -10.0f64..10.0, -10.0f64..10.0).prop_map(|(o, x, y)| Op::Offset(o, x, y)),
        line(cx, cy, 0).prop_map(Op::LineDistance),
        prop::collection::vec(line(cx, cy, 0), 0..=3).prop_map(Op::Area),
        (line(cx, cy, 0), dist.clone()).prop_map(|(l, d)| Op::Along(l, d)),
        (p(), p(), p()).prop_map(|(q, a, b)| Op::PointToSegmentDistance(q, a, b)),
        (line(cx, cy, 0), p()).prop_map(|(l, q)| Op::PointOnLine(l, q)),
        (p(), p(), line(cx, cy, 2)).prop_map(|(a, b, l)| Op::LineSlice(a, b, l)),
        (dist.clone(), dist.clone(), line(cx, cy, 0)).prop_map(|(a, b, l)| Op::LineSliceAlong(a, b, l)),
        (p(), dist.clone()).prop_map(|(q, d)| Op::BufferPoint(q, d)),
        (bx(), dist).prop_map(|(b, d)| Op::BufferBBox(b, d)),
        (p(), bx()).prop_map(|(q, b)| Op::InsideBBox(q, b)),
        (p(), p(), -0.5f64..1.5).prop_map(|(a, b, t)| Op::Interpolate(a, b, t)),
    ]
}

#[derive(Clone, Debug)]
enum Ruler {
    New(f64, usize),
    FromTile(u32, u32),
}

fn ruler() -> impl Strategy<Value = Ruler> {
    prop_oneof![
        4 => (-90.0f64..90.0, 0usize..7).prop_map(|(lat, u)| Ruler::New(lat, u)),
        1 => (0u32..=20).prop_flat_map(|z| ((0u32..(1u32 << z)), Just(z))).prop_map(|(y, z)| Ruler::FromTile(y, z)),
    ]
}

#[test]
fn cheap_ruler_matches_cpp() {
    let strategy = (ruler(), -180.0f64..180.0, -80.0f64..80.0)
        .prop_flat_map(|(r, cx, cy)| (Just(r), prop::collection::vec(op(cx, cy), 1..=20)));
    let ops = std::cell::Cell::new(0);
    let cases = run_cases(3000, &strategy, |(r, list)| {
        let (ruler, request_ruler) = match r {
            Ruler::New(lat, u) => (CheapRuler::new(lat, UNITS[u]), json!(["new", d(lat), u])),
            Ruler::FromTile(y, z) => (CheapRuler::from_tile(y, z), json!(["fromTile", y, z])),
        };
        let request = json!({"ruler": request_ruler, "ops": list.iter().map(Op::request).collect::<Vec<_>>()});
        let expected = cpp(&request);
        let actual = Json::Array(list.iter().map(|o| o.rust(&ruler)).collect());
        ops.set(ops.get() + list.len());
        prop_assert_eq!(actual, expected, "{:?}", list);
        Ok(())
    });
    println!("CheapRuler: {cases} cases ({} method calls), 0 mismatches", ops.get());
}

#[test]
fn point_on_line_signed_zero() {
    // t = -0.0 (p on a segment start, segment pointing down-left):
    // ::fmax(0., ::fmin(1., -0.)) picks glibc's zero.
    let ruler = CheapRuler::new(10.0, Unit::Kilometers);
    let line = vec![Point::new(1.0, 1.0), Point::new(0.5, 0.5)];
    let op = Op::PointOnLine(line, Point::new(1.0, 1.0));
    let request = json!({"ruler": ["new", d(10.0), 0], "ops": [op.request()]});
    assert_eq!(Json::Array(vec![op.rust(&ruler)]), cpp(&request));
}

fn remainder_request(pairs: &[(f64, f64)]) -> Json {
    json!({
        "ruler": ["new", d(0.0), 0],
        "ops": pairs.iter().map(|&(x, y)| json!(["remainder", d(x), d(y)])).collect::<Vec<_>>(),
    })
}

#[test]
fn remainder_matches_libm() {
    let specials = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        180.0,
        -180.0,
        360.0,
        540.0,
        -540.0,
        720.0,
        900.0,
        1e300,
        -1e300,
        f64::MAX,
        f64::MIN_POSITIVE,
        5e-324,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        0.1,
        179.99999999999997,
    ];
    let mut pairs = Vec::new();
    for &x in &specials {
        for &y in &specials {
            pairs.push((x, y));
        }
    }
    let expected = cpp(&remainder_request(&pairs));
    let actual = Json::Array(pairs.iter().map(|&(x, y)| d(cheap_ruler::remainder(x, y))).collect());
    // NaN payloads may differ; compare NaN as NaN.
    let canon = |j: &Json| -> Vec<u64> {
        j.as_array()
            .unwrap()
            .iter()
            .map(|v| {
                let b = v.as_u64().unwrap();
                if f64::from_bits(b).is_nan() { u64::MAX } else { b }
            })
            .collect()
    };
    assert_eq!(canon(&actual), canon(&expected));
    let mut cases = pairs.len() as u32;

    let strategy = prop::collection::vec(
        prop_oneof![
            (-2000.0f64..2000.0, Just(360.0)),
            ((-1000i32..1000).prop_map(|k| f64::from(k) * 180.0), Just(360.0)),
            (any::<f64>(), any::<f64>()),
            (any::<f64>(), Just(360.0)),
        ],
        1..=50,
    );
    cases += run_cases(5000, &strategy, |pairs| {
        let expected = cpp(&remainder_request(&pairs));
        let actual = Json::Array(pairs.iter().map(|&(x, y)| d(cheap_ruler::remainder(x, y))).collect());
        prop_assert_eq!(canon(&actual), canon(&expected));
        Ok(())
    }) * 25;
    println!("std::remainder: ~{cases} values, 0 mismatches");
}
