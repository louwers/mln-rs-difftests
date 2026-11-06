// Difftest of //vendor/geojson against mapbox::geojson (geojson_impl.hpp,
// geojson_reference.cpp): convert (exact values incl. variant types, or the
// exact error message), parse (inputs whose numbers both JSON parsers read
// the same), stringify (byte-exact; structurally where a property map has
// several keys, whose C++ order is std::unordered_map's) and the number
// formatting of stringify for any double.

use feature::{Feature, Value};
use geojson_canonical::*;
use geometry::{Geometry, Point};
use geometry_difftest_support::run_cases;
use proptest::prelude::*;
use proptest::test_runner::TestCaseResult;
use serde_json::{Value as Json, json};

fn rust_convert(text: &str) -> Json {
    let d: Json = serde_json::from_str(text).unwrap();
    geojson_result(&geojson::convert(&d))
}

fn check_convert(text: &str) -> TestCaseResult {
    let expected = cpp_convert(text);
    let actual = rust_convert(text);
    prop_assert!(actual == expected, "input {text}\nC++  {}\nRust {}", abbreviate(&expected), abbreviate(&actual));
    Ok(())
}

#[test]
fn convert_matches_cpp() {
    let cases = run_cases(4000, &geojson_json(), |(doc, _)| check_convert(&doc.to_string()));
    println!("geojson convert: {cases} cases, 0 mismatches");
}

/// Invalid documents: every error message of geojson_impl.hpp that does not
/// come from a rapidjson precondition (non-number coordinates, non-array
/// line strings, non-string types: UB in C++, see geojson_impl.rs).
fn invalid_document() -> impl Strategy<Value = Json> {
    let coords = prop::sample::select(vec![
        json!(null),
        json!(1),
        json!("x"),
        json!({}),
        json!([]),
        json!([1]),
        json!([1, 2]),
        json!([1, 2, 3]),
        json!([[1, 2]]),
        json!([[1, 2], [3, 4]]),
        json!([[1, 2], [3, 4], [5, 6]]),
        json!([[[1, 2], [3, 4], [5, 6]]]),
        json!([[[1, 2], [3, 4], [5, 6], [1, 2]]]),
        json!([[1, 2, 3, 4]]),
        json!([[[1, 2], [3, 4], [5, 6], [1]]]),
        json!([[[[1, 2], [3, 4], [5, 6], [1, 2]]], [[]]]),
        json!([[[[1, 2], [3, 4], [5, 6], [1, 2]]], [[[], [3, 4], [5, 6], [1, 2]]]]),
        json!([[[[1, 2], [3, 4], [5, 6], [1, 2]]], 5]),
        json!([[[[1, 2], [3, 4], [5, 6], [1, 2]]], [5]]),
        json!([[1, 2], [[3, 4], [5, 6]]]),
        json!([[[1, 2], [3, 4]], [[5, 6]]]),
    ]);
    let geometry_type = prop::sample::select(vec![
        "Point",
        "MultiPoint",
        "LineString",
        "MultiLineString",
        "Polygon",
        "MultiPolygon",
        "Pologon",
        "",
    ]);
    let geometry = (geometry_type, coords, 0u8..4).prop_map(|(t, c, shape)| match shape {
        0 => json!({"type": t, "coordinates": c}),
        1 => json!({"type": t}),
        2 => json!({"coordinates": c}),
        _ => json!({"type": "GeometryCollection", "geometries": [{"type": t, "coordinates": c}]}),
    });
    let odd = prop::sample::select(vec![
        json!(42),
        json!("x"),
        json!([]),
        json!({}),
        json!({"type": "GeometryCollection"}),
        json!({"type": "GeometryCollection", "geometries": {}}),
        json!({"type": "GeometryCollection", "geometries": [5]}),
        json!({"type": "FeatureCollection"}),
        json!({"type": "FeatureCollection", "features": {}}),
        json!({"type": "FeatureCollection", "features": [5]}),
        json!({"type": "FeatureCollection", "features": [{"type": "Feature"}]}),
        json!({"type": "FeatureCollection", "features": [{"type": "Point", "geometry": null}]}),
        json!({"type": "FeatureCollection", "features": [{"geometry": null}]}),
        json!({"type": "Feature"}),
        json!({"type": "Feature", "geometry": 5}),
        json!({"type": "Feature", "geometry": null, "id": null}),
        json!({"type": "Feature", "geometry": null, "id": true}),
        json!({"type": "Feature", "geometry": null, "id": [1]}),
        json!({"type": "Feature", "geometry": null, "properties": 5}),
        json!({"type": "Feature", "geometry": null, "properties": []}),
        json!({"type": "Feature", "geometry": null, "properties": null, "id": 1.5}),
        json!({"type": "Feature", "geometry": null, "id": -3}),
        json!({"type": "Feature", "geometry": null, "id": 18446744073709551615u64}),
        json!({"type": "Feature", "geometry": null, "id": 1e30}),
    ]);
    prop_oneof![
        geometry.clone(),
        geometry.clone().prop_map(|g| json!({"type": "Feature", "geometry": g})),
        geometry.prop_map(|g| json!({"type": "FeatureCollection", "features": [{"type": "Feature", "geometry": g}]})),
        odd,
    ]
}

/// False if converting `doc` could reach a rapidjson precondition
/// (`GetDouble()` on a non-number at a point position, `GetArray()` on a
/// non-array line string of a MultiLineString): UB in C++, not compared.
fn precondition_free(doc: &Json) -> bool {
    fn points_ok(coords: &Json, depth: usize) -> bool {
        let Some(a) = coords.as_array() else { return true };
        if depth == 0 {
            return a.len() < 2 || (a[0].is_number() && a[1].is_number());
        }
        a.iter().all(|e| points_ok(e, depth - 1))
    }
    fn geometry_ok(g: &Json) -> bool {
        let Some(o) = g.as_object() else { return true };
        if let Some(geometries) = o.get("geometries").and_then(Json::as_array) {
            return geometries.iter().all(geometry_ok);
        }
        let coords = o.get("coordinates").unwrap_or(&Json::Null);
        match o.get("type").and_then(Json::as_str) {
            Some("Point") => points_ok(coords, 0),
            Some("MultiPoint" | "LineString") => points_ok(coords, 1),
            Some("MultiLineString") => {
                coords.as_array().is_none_or(|a| a.iter().all(Json::is_array)) && points_ok(coords, 2)
            }
            Some("Polygon") => points_ok(coords, 2),
            Some("MultiPolygon") => points_ok(coords, 3),
            _ => true,
        }
    }
    fn feature_ok(f: &Json) -> bool {
        f.get("geometry").is_none_or(geometry_ok)
    }
    match doc.get("type").and_then(Json::as_str) {
        Some("FeatureCollection") => {
            doc.get("features").and_then(Json::as_array).is_none_or(|f| f.iter().all(feature_ok))
        }
        Some("Feature") => feature_ok(doc),
        _ => geometry_ok(doc),
    }
}

#[test]
fn convert_errors_match_cpp() {
    let skipped = std::cell::Cell::new(0);
    let cases = run_cases(3000, &invalid_document(), |doc| {
        if !precondition_free(&doc) {
            skipped.set(skipped.get() + 1);
            return Ok(());
        }
        check_convert(&doc.to_string())
    });
    let cases = cases - skipped.get();
    println!(
        "geojson convert (invalid documents): {cases} cases, 0 mismatches ({} skipped: C++ precondition)",
        skipped.get()
    );
}

/// Rounds every number to 6 decimals, where rapidjson's default-precision
/// parse is exact (fast path) and agrees with serde_json.
fn round_numbers(j: &mut Json) {
    match j {
        Json::Number(n) => {
            if let Some(f) = n.as_f64()
                && !n.is_u64()
                && !n.is_i64()
            {
                *j = json!((f * 1e6).round() / 1e6);
            }
        }
        Json::Array(a) => a.iter_mut().for_each(round_numbers),
        Json::Object(o) => o.values_mut().for_each(round_numbers),
        _ => {}
    }
}

#[test]
fn parse_matches_cpp() {
    let cases = run_cases(3000, &geojson_json(), |(mut doc, _)| {
        round_numbers(&mut doc);
        let text = doc.to_string();
        let expected = cpp_parse(&text);
        let actual = geojson_result(&geojson::parse(&text));
        prop_assert!(actual == expected, "input {text}\nC++  {}\nRust {}", abbreviate(&expected), abbreviate(&actual));
        Ok(())
    });
    println!("geojson parse: {cases} cases, 0 mismatches");
}

/// The largest property map (or object inside one), whose member order is
/// std::unordered_map's in C++.
fn max_object_len(j: &Json) -> usize {
    fn all_objects(j: &Json) -> usize {
        match j {
            Json::Array(a) => a.iter().map(all_objects).max().unwrap_or(0),
            Json::Object(o) => o.len().max(o.values().map(all_objects).max().unwrap_or(0)),
            _ => 0,
        }
    }
    match j {
        Json::Array(a) => a.iter().map(max_object_len).max().unwrap_or(0),
        Json::Object(o) => {
            o.iter().map(|(k, v)| if k == "properties" { all_objects(v) } else { max_object_len(v) }).max().unwrap_or(0)
        }
        _ => 0,
    }
}

#[test]
fn stringify_matches_cpp() {
    let exact = std::cell::Cell::new(0);
    let cases = run_cases(4000, &geojson_json(), |(doc, _)| {
        let text = doc.to_string();
        let expected = cpp_stringify(&text);
        let actual = geojson::stringify(&geojson::convert(&doc).unwrap());
        if max_object_len(&doc) <= 1 {
            prop_assert_eq!(&actual, &expected, "input {}", text);
            exact.set(exact.get() + 1);
        } else {
            let e: Json = serde_json::from_str(&expected).unwrap();
            let a: Json = serde_json::from_str(&actual).unwrap();
            prop_assert!(a == e, "input {text}\nC++  {expected}\nRust {actual}");
        }
        Ok(())
    });
    println!(
        "geojson stringify: {cases} cases ({} byte-exact, {} as unordered JSON), 0 mismatches",
        exact.get(),
        cases - exact.get()
    );
}

fn rust_stringify_doubles(x: f64, y: f64, property: f64) -> String {
    let mut f = Feature::new(Geometry::Point(Point::new(x, y)));
    f.properties.insert("v".to_owned(), Value::Double(property));
    format!("{}|{}", geojson::stringify_geometry(&Geometry::Point(Point::new(x, y))), geojson::stringify_feature(&f))
}

#[test]
fn stringify_doubles_match_cpp() {
    let specials = [
        0.0,
        -0.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MIN_POSITIVE,
        5e-324,
        f64::MAX,
        f64::MIN,
        1e21,
        1e-7,
        123456789012345680000.0,
        0.1,
        1.0 / 3.0,
    ];
    let mut cases = 0;
    for &x in &specials {
        for &y in &specials {
            for &p in &specials {
                assert_eq!(rust_stringify_doubles(x, y, p), cpp_stringify_doubles(x, y, p), "{x} {y} {p}");
                cases += 1;
            }
        }
    }
    cases += run_cases(20_000, &(any::<f64>(), any::<f64>(), any::<f64>()), |(x, y, p)| {
        prop_assert_eq!(rust_stringify_doubles(x, y, p), cpp_stringify_doubles(x, y, p));
        Ok(())
    });
    println!("geojson stringify doubles: {cases} cases, 0 mismatches");
}
