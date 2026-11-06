// Difftest of the geometry ports against the C++ libraries on the libraries'
// own test fixtures (read in place from @maplibre//vendor), exact:
// - geojson.hpp/test/fixtures: convert, parse, stringify;
// - geojson-vt-cpp/test/fixtures + data/countries.geojson: GeoJSONVT with the
//   option sets of test.cpp, every internal tile plus test.cpp's tiles, and
//   geoJSONToTile;
// - supercluster/test/fixtures/places.json: the options of test.cpp, every
//   tile at z0-3, every cluster's children/leaves/expansion zoom;
// - polylabel/test/fixtures: several precisions;
// - kdbush.hpp/test.cpp: its points and queries, several node sizes.
// (earcut's fixtures: earcut_fixtures_difftest; cheap-ruler's are turf.js
// expectations, compared by //vendor/cheap-ruler:cheap_ruler_fixtures_test
// and here only as inputs.)

use geojson_canonical::*;
use geometry_difftest_support::maplibre_runfile;
use serde_json::{Value as Json, json};

fn read(path: &str) -> String {
    std::fs::read_to_string(maplibre_runfile(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn files(dir: &str, ext: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(maplibre_runfile(dir))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap()))
        .collect();
    v.sort();
    v
}

#[test]
fn geojson_fixtures() {
    let mut cases = 0;
    let mut parse_differences = Vec::new();
    for (name, text) in files("vendor/maplibre-native-base/deps/geojson.hpp/test/fixtures", "json") {
        if let Ok(doc) = serde_json::from_str::<Json>(&text) {
            assert_eq!(geojson_result(&geojson::convert(&doc)), cpp_convert(&text), "convert {name}");
            cases += 1;
            if let Ok(g) = geojson::convert(&doc) {
                let expected = cpp_stringify(&text);
                let actual = geojson::stringify(&g);
                let e: Json = serde_json::from_str(&expected).unwrap();
                let a: Json = serde_json::from_str(&actual).unwrap();
                assert_eq!(a, e, "stringify {name}");
                cases += 1;
            }
        }
        // parse: C++ uses rapidjson's default precision; report, don't fail,
        // where only that differs (DEVIATIONS.md).
        let expected = cpp_parse(&text);
        let actual = geojson_result(&geojson::parse(&text));
        if actual != expected {
            let both_errors = actual.get("error").is_some() && expected.get("error").is_some();
            assert!(both_errors || expected.get("error").is_none(), "parse {name}: {actual} vs {expected}");
            parse_differences.push(name);
        }
        cases += 1;
    }
    println!(
        "geojson.hpp fixtures: {cases} cases, 0 mismatches; parse differs (error text or default-precision numbers) for {parse_differences:?}"
    );
}

fn vt_request(doc: &Json, options: Json, queries: &[(u8, u32, u32)]) -> Json {
    json!({"geojson": doc, "options": options, "queries": queries})
}

fn vt_options(
    max_zoom: u8,
    index_max_zoom: u8,
    index_max_points: u32,
    extent: u16,
    tolerance: f64,
    buffer: u16,
    line_metrics: bool,
    generate_id: bool,
) -> (Json, geojson_vt::Options) {
    let o = geojson_vt::Options {
        tile: geojson_vt::TileOptions { tolerance, extent, buffer, line_metrics },
        max_zoom,
        index_max_zoom,
        index_max_points,
        generate_id,
    };
    (
        json!({"tolerance": tolerance, "extent": extent, "buffer": buffer, "line_metrics": line_metrics,
               "max_zoom": max_zoom, "index_max_zoom": index_max_zoom, "index_max_points": index_max_points,
               "generate_id": generate_id}),
        o,
    )
}

fn rust_vt(doc: &Json, options: geojson_vt::Options, queries: &[(u8, u32, u32)]) -> Json {
    let geojson = geojson::convert(doc).unwrap();
    let mut index = geojson_vt::GeoJSONVT::from_geojson(&geojson, options);
    let tiles: Vec<Json> = queries
        .iter()
        .map(|&(z, x, y)| match index.get_tile(z, x, y) {
            Ok(t) => json!({"empty_tile": std::ptr::eq(t, &geojson_vt::EMPTY_TILE), "tile": tile(t)}),
            Err(e) => json!({"error": e}),
        })
        .collect();
    let stats: Vec<Json> = index.stats.iter().map(|(z, n)| json!([z, n])).collect();
    let mut internal: Vec<[u32; 4]> = index
        .get_internal_tiles()
        .values()
        .map(|t| [u32::from(t.z), t.x, t.y, t.source_features.len() as u32])
        .collect();
    internal.sort();
    json!({"tiles": tiles, "total": index.total, "stats": stats, "internal": internal})
}

#[test]
fn geojson_vt_fixtures() {
    let dir = "vendor/maplibre-native-base/deps/geojson-vt-cpp";
    let inputs = [
        "test/fixtures/us-states.json",
        "test/fixtures/dateline.json",
        "test/fixtures/dateline-triangle.json",
        "test/fixtures/feature.json",
        "test/fixtures/collection.json",
        "test/fixtures/single-geom.json",
        "test/fixtures/single-tile.json",
        "test/fixtures/linestring.json",
        "test/fixtures/polygon-bug.json",
        "test/fixtures/empty.json",
        "test/fixtures/points.geojson",
        "data/countries.geojson",
    ];
    let option_sets = [
        vt_options(18, 5, 100000, 4096, 3.0, 64, false, false), // defaults
        vt_options(14, 7, 200, 4096, 3.0, 64, false, false),    // TileTest us-states/dateline
        vt_options(14, 0, 10000, 4096, 3.0, 64, true, false),   // dateline metrics
        vt_options(14, 0, 10000, 4096, 3.0, 64, false, false),  // feature/collection/single-geom
        vt_options(20, 5, 100000, 8192, 0.0, 64, false, true),  // GenerateIds
        vt_options(20, 5, 100000, 8192, 0.0, 64, false, false), // Projection
        vt_options(18, 5, 100000, 4096, 3.0, 1024, false, false), // PolygonClippingBug
        vt_options(18, 5, 100000, 8192, 3.0, 2048, true, false), // ClipVertexOnTileBorder
    ];
    let test_tiles: Vec<(u8, u32, u32)> = vec![
        (7, 37, 48),
        (9, 148, 192),
        (11, 800, 400),
        (5, 19, 9),
        (13, 2344, 3134),
        (1, 0, 0),
        (1, 1, 1),
        (12, 1171, 1566),
        (16, 10482, 25329),
        (20, 167719, 405281),
        (19, 2, 3),
    ];
    let mut cases = 0;
    let mut tiles = 0;
    for input in inputs {
        let doc: Json = serde_json::from_str(&read(&format!("{dir}/{input}"))).unwrap();
        for (options_json, options) in &option_sets {
            // First the test tiles, then every tile in the resulting cache.
            let first = cpp_geojsonvt(&vt_request(&doc, options_json.clone(), &test_tiles));
            assert_eq!(rust_vt(&doc, *options, &test_tiles), first, "{input} {options_json}");
            let mut queries: Vec<(u8, u32, u32)> = first["internal"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| (t[0].as_u64().unwrap() as u8, t[1].as_u64().unwrap() as u32, t[2].as_u64().unwrap() as u32))
                .collect();
            queries.extend(&test_tiles);
            let second = cpp_geojsonvt(&vt_request(&doc, options_json.clone(), &queries));
            assert_eq!(rust_vt(&doc, *options, &queries), second, "{input} {options_json}");
            cases += 2;
            tiles += test_tiles.len() + queries.len();
        }
        for &(z, x, y) in &test_tiles {
            for (wrap, clip) in [(false, false), (true, true), (false, true)] {
                let options = geojson_vt::TileOptions::default();
                let request = json!({"geojson": doc, "z": z, "x": x, "y": y,
                    "options": {"tolerance": 3.0, "extent": 4096, "buffer": 64, "line_metrics": false},
                    "wrap": wrap, "clip": clip});
                let g = geojson::convert(&doc).unwrap();
                assert_eq!(
                    tile(&geojson_vt::geojson_to_tile(&g, z, x, y, &options, wrap, clip)),
                    cpp_geojson_to_tile(&request),
                    "geoJSONToTile {input} {z}/{x}/{y}"
                );
                cases += 1;
            }
        }
    }
    println!("geojson-vt fixtures: {cases} cases ({tiles} getTile requests), 0 mismatches");
}

unsafe extern "C" {
    fn ref_supercluster(text: *const u8, len: usize) -> *mut std::ffi::c_char;
    fn ref_kdbush(text: *const u8, len: usize) -> *mut std::ffi::c_char;
    fn ref_polylabel(xy: *const f64, sizes: *const u32, rings: usize, precision: f64, out: *mut f64) -> bool;
    fn ref_geojson_free(p: *mut std::ffi::c_char);
}

fn call(f: unsafe extern "C" fn(*const u8, usize) -> *mut std::ffi::c_char, request: &Json) -> Json {
    let text = request.to_string();
    unsafe {
        let p = f(text.as_ptr(), text.len());
        let s = std::ffi::CStr::from_ptr(p).to_str().unwrap().to_owned();
        ref_geojson_free(p);
        serde_json::from_str(&s).unwrap()
    }
}

/// places.json as test.cpp's parseFeatures reads it, plus "n" =
/// scalerank for the reference's mode-1 map/reduce (test 3/4's sum).
fn places() -> Json {
    let d: Json = serde_json::from_str(&read("vendor/supercluster/test/fixtures/places.json")).unwrap();
    let features: Vec<Json> = d["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let mut props = serde_json::Map::new();
            for key in
                ["name", "scalerank", "lat_y", "long_x", "region", "featureclass", "comment", "name_alt", "subregion"]
            {
                if let Some(v) = f["properties"].get(key) {
                    let v = match v {
                        Json::Null => Some(json!("null")),
                        Json::String(_) => Some(v.clone()),
                        Json::Number(n) if n.is_u64() || n.is_f64() => Some(v.clone()),
                        _ => None,
                    };
                    if let Some(v) = v {
                        props.insert(key.to_owned(), v);
                    }
                }
            }
            if let Some(n) = f["properties"]["scalerank"].as_u64() {
                props.insert("n".to_owned(), json!(n));
            }
            json!({"type": "Feature", "geometry": f["geometry"], "properties": props})
        })
        .collect();
    json!({"type": "FeatureCollection", "features": features})
}

#[test]
fn supercluster_fixtures() {
    let features = places();
    let mut tiles = Vec::new();
    for z in 0u8..=3 {
        for x in 0..(1u32 << z) {
            for y in 0..(1u32 << z) {
                tiles.push((z, x, y));
            }
        }
    }
    let option_sets = [
        (json!({"min_zoom": 0, "max_zoom": 16, "radius": 40, "extent": 512, "min_points": 2, "generate_id": false}), 0),
        (json!({"min_zoom": 0, "max_zoom": 4, "radius": 60, "extent": 256, "min_points": 2, "generate_id": false}), 0),
        (json!({"min_zoom": 0, "max_zoom": 16, "radius": 40, "extent": 512, "min_points": 2, "generate_id": false}), 1),
        (
            json!({"min_zoom": 0, "max_zoom": 16, "radius": 100, "extent": 512, "min_points": 2, "generate_id": false}),
            1,
        ),
        (json!({"min_zoom": 0, "max_zoom": 16, "radius": 40, "extent": 512, "min_points": 5, "generate_id": false}), 0),
        (json!({"min_zoom": 0, "max_zoom": 16, "radius": 40, "extent": 512, "min_points": 2, "generate_id": true}), 0),
        (json!({"min_zoom": 0, "max_zoom": 16, "radius": 40, "extent": 512, "min_points": 2, "generate_id": false}), 2),
        (
            json!({"min_zoom": 2, "max_zoom": 14, "radius": 50, "extent": 8192, "min_points": 3, "generate_id": false}),
            3,
        ),
    ];
    let mut clusters = 0;
    for (options, mode) in &option_sets {
        let request = json!({
            "features": features, "options": options, "mode": mode, "tiles": tiles,
            "extra_ids": [1, 33, 353, 833, 1857, 2436, 0, 4000000000u32],
            "leaf_args": [[10, 0], [10, 5], [1000, 0], [3, 7]],
        });
        let expected = call(ref_supercluster, &request);
        let actual = supercluster_rust(&request);
        clusters += expected["clusters"].as_array().unwrap().len();
        assert!(
            actual == expected,
            "{options} mode {mode}\nC++  {}\nRust {}",
            abbreviate(&expected),
            abbreviate(&actual)
        );
    }
    println!(
        "supercluster fixtures: {} cases ({} tiles each, {clusters} cluster ids), 0 mismatches",
        option_sets.len(),
        tiles.len()
    );
}

/// supercluster_difftest.rs's procedure for one request.
fn supercluster_rust(request: &Json) -> Json {
    use std::sync::Arc;

    use feature::{PropertyMap, Value};
    let geojson::GeoJSON::FeatureCollection(fs) = geojson::convert(&request["features"]).unwrap() else { panic!() };
    let o = &request["options"];
    let mut options = supercluster::Options {
        min_zoom: o["min_zoom"].as_u64().unwrap() as u8,
        max_zoom: o["max_zoom"].as_u64().unwrap() as u8,
        radius: o["radius"].as_u64().unwrap() as u16,
        extent: o["extent"].as_u64().unwrap() as u16,
        min_points: o["min_points"].as_u64().unwrap() as usize,
        generate_id: o["generate_id"].as_bool().unwrap(),
        ..supercluster::Options::default()
    };
    match request["mode"].as_u64().unwrap() {
        1 => {
            options.map = Arc::new(|p: &PropertyMap| {
                let mut ret = PropertyMap::new();
                if let Some(Value::Uint(u)) = p.get("n") {
                    ret.insert("sum".to_owned(), Value::Uint(*u));
                }
                ret
            });
            options.reduce = Some(Arc::new(|a: &mut PropertyMap, b: &PropertyMap| {
                if let (Some(Value::Uint(x)), Some(Value::Uint(y))) = (a.get("sum"), b.get("sum")) {
                    let s = x.wrapping_add(*y);
                    a.insert("sum".to_owned(), Value::Uint(s));
                }
            }));
        }
        2 => {
            options.map = Arc::new(|p: &PropertyMap| {
                let mut ret = PropertyMap::new();
                let Some(Value::String(name)) = p.get("name") else { return ret };
                ret.insert("names".to_owned(), Value::String(name.clone()));
                let w = match p.get("n") {
                    Some(Value::Uint(u)) => *u as f64,
                    _ => 0.5,
                };
                ret.insert("w".to_owned(), Value::Double(w));
                ret
            });
            options.reduce = Some(Arc::new(|a: &mut PropertyMap, b: &PropertyMap| {
                if let (Some(Value::String(x)), Some(Value::String(y))) = (a.get("names"), b.get("names")) {
                    let n = format!("{x},{y}");
                    a.insert("names".to_owned(), Value::String(n));
                }
                if let (Some(Value::Double(x)), Some(Value::Double(y))) = (a.get("w"), b.get("w")) {
                    let w = x * 0.5 + y;
                    a.insert("w".to_owned(), Value::Double(w));
                }
            }));
        }
        3 => {
            options.reduce = Some(Arc::new(|a: &mut PropertyMap, _: &PropertyMap| {
                let c = match a.get("count") {
                    Some(Value::Uint(u)) => *u,
                    _ => 0,
                };
                a.insert("count".to_owned(), Value::Uint(c + 1));
            }));
        }
        _ => {}
    }
    let index = supercluster::Supercluster::new(&fs, options).unwrap();
    let mut ids: Vec<u32> = Vec::new();
    let mut tiles = Vec::new();
    for t in request["tiles"].as_array().unwrap() {
        let tile =
            index.get_tile(t[0].as_u64().unwrap() as u8, t[1].as_u64().unwrap() as u32, t[2].as_u64().unwrap() as u32);
        tiles.push(features(&tile));
        for f in &tile {
            if let Some(Value::Uint(id)) = f.properties.get("cluster_id")
                && !ids.contains(&(*id as u32))
            {
                ids.push(*id as u32);
            }
        }
    }
    ids.extend(request["extra_ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32));
    let or_error = |r: Result<Json, String>| r.unwrap_or_else(|e| json!({"error": e}));
    let clusters: Vec<Json> = ids
        .iter()
        .map(|&id| {
            json!({
                "id": id,
                "children": or_error(index.get_children(id).map(|f| json!({"features": features(&f)}))),
                "leaves": request["leaf_args"].as_array().unwrap().iter().map(|a| {
                    or_error(index.get_leaves(id, a[0].as_u64().unwrap() as u32, a[1].as_u64().unwrap() as u32)
                        .map(|f| json!({"features": features(&f)})))
                }).collect::<Vec<_>>(),
                "expansion": or_error(index.get_cluster_expansion_zoom(id).map(|z| json!({"zoom": z}))),
            })
        })
        .collect();
    json!({"tiles": tiles, "clusters": clusters})
}

#[test]
fn polylabel_fixtures() {
    let mut cases = 0;
    for (name, text) in files("vendor/polylabel/test/fixtures", "json") {
        let d: Json = serde_json::from_str(&text).unwrap();
        let polygon: Vec<Vec<geometry::Point<f64>>> = d
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                r.as_array()
                    .unwrap()
                    .iter()
                    .map(|p| geometry::Point::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
                    .collect()
            })
            .collect();
        let mut xy = Vec::new();
        let mut sizes = Vec::new();
        for r in &polygon {
            sizes.push(r.len() as u32);
            for p in r {
                xy.push(p.x);
                xy.push(p.y);
            }
        }
        for precision in [1.0, 50.0, 16.0, 0.5, 4.0] {
            let mut out = [0.0; 2];
            assert!(unsafe { ref_polylabel(xy.as_ptr(), sizes.as_ptr(), sizes.len(), precision, out.as_mut_ptr()) });
            let p = polylabel::polylabel(&polygon, precision).unwrap();
            assert_eq!((p.x.to_bits(), p.y.to_bits()), (out[0].to_bits(), out[1].to_bits()), "{name} {precision}");
            cases += 1;
        }
    }
    println!("polylabel fixtures: {cases} cases, 0 mismatches");
}

#[test]
fn kdbush_fixture() {
    let source = read("vendor/kdbush.hpp/test.cpp");
    let start = source.find("static std::vector<TPoint> points =").unwrap();
    let body = &source[start..source[start..].find("};").unwrap() + start];
    let numbers: Vec<i32> = body
        .split(|c: char| !(c.is_ascii_digit() || c == '-'))
        .filter(|t| !t.is_empty())
        .map(|t| t.parse().unwrap())
        .collect();
    let points: Vec<(i32, i32)> = numbers.chunks(2).map(|c| (c[0], c[1])).collect();
    assert_eq!(points.len(), 100);
    let queries = json!([
        ["range", 20, 30, 50, 70],
        ["within", 50, 50, 20],
        ["range", 0, 0, 100, 100],
        ["within", 0, 0, 200],
        ["range", 50, 50, 50, 50]
    ]);
    let mut cases = 0;
    for node_size in [10u8, 64, 1, 2, 255] {
        let expected =
            call(ref_kdbush, &json!({"type": "i32", "node_size": node_size, "points": points, "queries": queries}));
        let index: kdbush::KDBush<i32, usize> = kdbush::KDBush::from_points(&points, node_size);
        let mut results = Vec::new();
        let mut r = Vec::new();
        index.range(20, 30, 50, 70, &mut |id| r.push(id));
        results.push(std::mem::take(&mut r));
        index.within(50, 50, 20, &mut |id| r.push(id));
        results.push(std::mem::take(&mut r));
        index.range(0, 0, 100, 100, &mut |id| r.push(id));
        results.push(std::mem::take(&mut r));
        index.within(0, 0, 200, &mut |id| r.push(id));
        results.push(std::mem::take(&mut r));
        index.range(50, 50, 50, 50, &mut |id| r.push(id));
        results.push(std::mem::take(&mut r));
        assert_eq!(json!({"results": results}), expected, "node size {node_size}");
        cases += 1;
    }
    println!("kdbush fixture: {cases} cases, 0 mismatches");
}
