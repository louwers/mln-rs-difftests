// Difftest of //vendor/kdbush against kdbush::KDBush (supercluster_reference.cpp):
// the exact ids, in visiting order, of range and within queries, for
// KDBush<pair<double, double>, uint32_t> (supercluster's arithmetic) and
// KDBush<pair<int, int>, size_t> (kdbush's own test), over random point
// sets (duplicates, collinear runs, > 600 points for Floyd-Rivest's
// sampling step) and node sizes.

use geometry_difftest_support::run_cases;
use kdbush::KDBush;
use proptest::prelude::*;
use serde_json::{Value as Json, json};

unsafe extern "C" {
    fn ref_geojson_free(p: *mut std::ffi::c_char);
    fn ref_kdbush(text: *const u8, len: usize) -> *mut std::ffi::c_char;
}

fn cpp(request: &Json) -> Json {
    let text = request.to_string();
    unsafe {
        let p = ref_kdbush(text.as_ptr(), text.len());
        let s = std::ffi::CStr::from_ptr(p).to_str().unwrap().to_owned();
        ref_geojson_free(p);
        serde_json::from_str(&s).unwrap()
    }
}

#[derive(Clone, Debug)]
enum Query<T> {
    Range(T, T, T, T),
    Within(T, T, T),
}

fn query_json<T: Into<Json> + Copy>(q: &Query<T>) -> Json {
    match *q {
        Query::Range(a, b, c, d) => json!(["range", a.into(), b.into(), c.into(), d.into()]),
        Query::Within(a, b, c) => json!(["within", a.into(), b.into(), c.into()]),
    }
}

fn points_f64() -> impl Strategy<Value = Vec<(f64, f64)>> {
    let point = prop_oneof![
        4 => (0.0f64..1.0, 0.0f64..1.0),
        1 => (0u8..8, 0u8..8).prop_map(|(x, y)| (f64::from(x) / 8.0, f64::from(y) / 8.0)),
        1 => (0.0f64..1.0).prop_map(|x| (x, 0.5)),
    ];
    prop_oneof![prop::collection::vec(point.clone(), 0..=200), prop::collection::vec(point, 601..=3000),]
}

fn queries_f64() -> impl Strategy<Value = Vec<Query<f64>>> {
    let q = prop_oneof![
        (0.0f64..1.0, 0.0f64..1.0, 0.0f64..0.5, 0.0f64..0.5).prop_map(|(x, y, w, h)| Query::Range(x, y, x + w, y + h)),
        (0.0f64..1.0, 0.0f64..1.0, 0.0f64..0.3).prop_map(|(x, y, r)| Query::Within(x, y, r)),
        (0u8..8, 0u8..8, 0u8..3)
            .prop_map(|(x, y, r)| { Query::Within(f64::from(x) / 8.0, f64::from(y) / 8.0, f64::from(r) / 8.0) }),
    ];
    prop::collection::vec(q, 1..=10)
}

fn node_size() -> impl Strategy<Value = u8> {
    prop::sample::select(vec![1u8, 2, 10, 64, 255])
}

#[test]
fn kdbush_f64_u32_matches_cpp() {
    let cases = run_cases(1500, &(points_f64(), node_size(), queries_f64()), |(points, node_size, queries)| {
        let request = json!({
            "type": "f64", "node_size": node_size, "points": points,
            "queries": queries.iter().map(query_json).collect::<Vec<_>>(),
        });
        let expected = cpp(&request);
        let index: KDBush<f64, u32> = KDBush::from_points(&points, node_size);
        let results: Vec<Vec<u32>> = queries
            .iter()
            .map(|q| {
                let mut r = Vec::new();
                match *q {
                    Query::Range(a, b, c, d) => index.range(a, b, c, d, &mut |id| r.push(id)),
                    Query::Within(a, b, c) => index.within(a, b, c, &mut |id| r.push(id)),
                }
                r
            })
            .collect();
        prop_assert_eq!(json!({ "results": results }), expected);
        Ok(())
    });
    println!("KDBush<pair<double>, uint32_t>: {cases} cases, 0 mismatches");
}

#[test]
fn kdbush_i32_usize_matches_cpp() {
    let point = (0i32..100, 0i32..100);
    let points = prop_oneof![prop::collection::vec(point.clone(), 0..=200), prop::collection::vec(point, 601..=2000),];
    let q = prop_oneof![
        (0i32..100, 0i32..100, 0i32..50, 0i32..50).prop_map(|(x, y, w, h)| Query::Range(x, y, x + w, y + h)),
        (0i32..100, 0i32..100, 0i32..30).prop_map(|(x, y, r)| Query::Within(x, y, r)),
    ];
    let strategy = (points, node_size(), prop::collection::vec(q, 1..=10));
    let cases = run_cases(1500, &strategy, |(points, node_size, queries)| {
        let request = json!({
            "type": "i32", "node_size": node_size, "points": points,
            "queries": queries.iter().map(query_json).collect::<Vec<_>>(),
        });
        let expected = cpp(&request);
        let index: KDBush<i32, usize> = KDBush::from_points(&points, node_size);
        let results: Vec<Vec<usize>> = queries
            .iter()
            .map(|q| {
                let mut r = Vec::new();
                match *q {
                    Query::Range(a, b, c, d) => index.range(a, b, c, d, &mut |id| r.push(id)),
                    Query::Within(a, b, c) => index.within(a, b, c, &mut |id| r.push(id)),
                }
                r
            })
            .collect();
        prop_assert_eq!(json!({ "results": results }), expected);
        Ok(())
    });
    println!("KDBush<pair<int>, size_t>: {cases} cases, 0 mismatches");
}
