// Difftest of //vendor/supercluster against mapbox::supercluster
// (supercluster_reference.cpp): for random point sets (clusters, exact
// duplicates, points on the antimeridian and beyond the Mercator limits)
// with properties and ids, random options and map/reduce functions (none,
// supercluster's own sum, an order-sensitive one, a counting one on the
// identity map), getTile at many zooms and tile coordinates, then
// getChildren, getLeaves (several limits and offsets) and
// getClusterExpansionZoom for every cluster id seen and some invalid ids
// must give exactly the same features (coordinates, properties incl.
// point_count_abbreviated, ids) or errors. Also every
// point_count_abbreviated text.

use std::sync::Arc;

use feature::{PropertyMap, Value};
use geojson_canonical::{abbreviate, features};
use geometry_difftest_support::run_cases;
use proptest::prelude::*;
use serde_json::{Value as Json, json};
use supercluster::{Options, Supercluster};

unsafe extern "C" {
    fn ref_geojson_free(p: *mut std::ffi::c_char);
    fn ref_supercluster(text: *const u8, len: usize) -> *mut std::ffi::c_char;
    fn ref_supercluster_abbreviation(num_points: u32) -> *mut std::ffi::c_char;
}

fn take(p: *mut std::ffi::c_char) -> String {
    unsafe {
        let s = std::ffi::CStr::from_ptr(p).to_str().unwrap().to_owned();
        ref_geojson_free(p);
        s
    }
}

fn cpp(request: &Json) -> Json {
    let text = request.to_string();
    serde_json::from_str(&take(unsafe { ref_supercluster(text.as_ptr(), text.len()) })).unwrap()
}

/// The map/reduce options of supercluster_reference.cpp `setMapReduce`.
fn set_map_reduce(options: &mut Options, mode: u32) {
    match mode {
        1 => {
            options.map = Arc::new(|properties: &PropertyMap| {
                let mut ret = PropertyMap::new();
                if let Some(v) = properties.get("n") {
                    let Value::Uint(u) = v else { panic!("bad_variant_access") };
                    ret.insert("sum".to_owned(), Value::Uint(*u));
                }
                ret
            });
            options.reduce = Some(Arc::new(|to_update: &mut PropertyMap, to_fill: &PropertyMap| {
                if let (Some(Value::Uint(a)), Some(Value::Uint(b))) = (to_update.get("sum"), to_fill.get("sum")) {
                    let sum = a.wrapping_add(*b);
                    to_update.insert("sum".to_owned(), Value::Uint(sum));
                }
            }));
        }
        2 => {
            options.map = Arc::new(|properties: &PropertyMap| {
                let mut ret = PropertyMap::new();
                let Some(name) = properties.get("name") else { return ret };
                let Value::String(name) = name else { panic!("bad_variant_access") };
                ret.insert("names".to_owned(), Value::String(name.clone()));
                let w = match properties.get("n") {
                    Some(Value::Uint(u)) => *u as f64,
                    Some(_) => panic!("bad_variant_access"),
                    None => 0.5,
                };
                ret.insert("w".to_owned(), Value::Double(w));
                ret
            });
            options.reduce = Some(Arc::new(|to_update: &mut PropertyMap, to_fill: &PropertyMap| {
                if let (Some(Value::String(a)), Some(Value::String(b))) = (to_update.get("names"), to_fill.get("names"))
                {
                    let names = format!("{a},{b}");
                    to_update.insert("names".to_owned(), Value::String(names));
                }
                if let (Some(Value::Double(a)), Some(Value::Double(b))) = (to_update.get("w"), to_fill.get("w")) {
                    let w = a * 0.5 + b;
                    to_update.insert("w".to_owned(), Value::Double(w));
                }
            }));
        }
        3 => {
            options.reduce = Some(Arc::new(|to_update: &mut PropertyMap, _: &PropertyMap| {
                let count = match to_update.get("count") {
                    Some(Value::Uint(u)) => *u,
                    Some(_) => panic!("bad_variant_access"),
                    None => 0,
                };
                to_update.insert("count".to_owned(), Value::Uint(count + 1));
            }));
        }
        _ => {}
    }
}

#[derive(Clone, Debug)]
struct Case {
    features: Json,
    min_zoom: u8,
    max_zoom: u8,
    radius: u16,
    extent: u16,
    min_points: usize,
    generate_id: bool,
    mode: u32,
    tiles: Vec<(u8, u32, u32)>,
    extra_ids: Vec<u32>,
    leaf_args: Vec<(u32, u32)>,
}

fn feature() -> impl Strategy<Value = (f64, f64, Json, Option<Json>)> {
    let properties = (
        prop::option::of(0u64..100),
        prop::option::of("[a-z]{1,3}"),
        prop::option::of(prop_oneof![any::<i64>().prop_map(|i| json!(i)), "[a-z]{0,3}".prop_map(|s| json!(s))]),
    )
        .prop_map(|(n, name, x)| {
            let mut p = serde_json::Map::new();
            if let Some(n) = n {
                p.insert("n".to_owned(), json!(n));
            }
            if let Some(name) = name {
                p.insert("name".to_owned(), json!(name));
            }
            if let Some(x) = x {
                p.insert("x".to_owned(), x);
            }
            Json::Object(p)
        });
    let id =
        prop_oneof![Just(None), any::<u32>().prop_map(|u| Some(json!(u))), "[a-z]{1,3}".prop_map(|s| Some(json!(s))),];
    (Just(0.0), Just(0.0), properties, id)
}

fn tile_of(lng: f64, lat: f64, z: u8) -> (i64, i64) {
    let n = f64::from(1u32 << z);
    let x = (lng / 360.0 + 0.5) * n;
    let sine = (lat.clamp(-85.0, 85.0) * std::f64::consts::PI / 180.0).sin();
    let y = (0.5 - 0.25 * ((1.0 + sine) / (1.0 - sine)).ln() / std::f64::consts::PI) * n;
    (x.floor() as i64, y.floor() as i64)
}

fn case() -> impl Strategy<Value = Case> {
    let centre = (
        prop_oneof![4 => -180.0f64..180.0, 1 => prop::sample::select(vec![-180.0, 180.0, 179.999, -179.999])],
        prop_oneof![4 => -80.0f64..80.0, 1 => -90.0f64..90.0],
        prop::sample::select(vec![30.0, 3.0, 0.3, 0.01, 0.0]),
    );
    let points =
        prop::collection::vec((centre, prop::collection::vec((-1.0f64..1.0, -1.0f64..1.0, feature()), 1..=40)), 1..=6);
    (
        points,
        0u8..=3,
        0u8..=16,
        prop::sample::select(vec![0u16, 10, 40, 100, 300]),
        prop::sample::select(vec![256u16, 512, 8192]),
        prop::sample::select(vec![2usize, 3, 5]),
        any::<bool>(),
        0u32..4,
        prop::collection::vec((0u8..=18, -1i64..=1, -1i64..=1, 0u8..4), 1..=8),
        prop::collection::vec(prop_oneof![0u32..4000, any::<u32>()], 0..=3),
        prop::collection::vec((0u32..=20, 0u32..=30), 1..=3),
    )
        .prop_map(
            |(
                groups,
                min_zoom,
                max_zoom,
                radius,
                extent,
                min_points,
                generate_id,
                mode,
                raw_tiles,
                extra_ids,
                leaf_args,
            )| {
                let max_zoom = max_zoom.max(min_zoom);
                let mut fs = Vec::new();
                let mut centres = Vec::new();
                for ((cx, cy, spread), members) in groups {
                    centres.push((cx, cy));
                    for (dx, dy, (_, _, props, id)) in members {
                        let mut f = serde_json::Map::new();
                        f.insert("type".to_owned(), json!("Feature"));
                        f.insert(
                            "geometry".to_owned(),
                            json!({"type": "Point", "coordinates": [cx + dx * spread, cy + dy * spread]}),
                        );
                        f.insert("properties".to_owned(), props);
                        if let Some(id) = id {
                            f.insert("id".to_owned(), id);
                        }
                        fs.push(Json::Object(f));
                    }
                }
                let tiles = raw_tiles
                    .iter()
                    .enumerate()
                    .map(|(i, &(z, dx, dy, kind))| {
                        let z = z.min(max_zoom + 2).min(20);
                        let (cx, cy) = centres[i % centres.len()];
                        let (tx, ty) = tile_of(cx, cy, z);
                        let n = 1i64 << z;
                        let x = match kind {
                            0 => 0,
                            1 => n - 1,
                            _ => (tx + dx).clamp(0, n - 1),
                        };
                        (z, x as u32, (ty + dy).clamp(0, n - 1) as u32)
                    })
                    .collect();
                Case {
                    features: json!({"type": "FeatureCollection", "features": fs}),
                    min_zoom,
                    max_zoom,
                    radius,
                    extent,
                    min_points,
                    generate_id,
                    mode,
                    tiles,
                    extra_ids,
                    leaf_args,
                }
            },
        )
}

fn request(c: &Case) -> Json {
    json!({
        "features": c.features,
        "options": {
            "min_zoom": c.min_zoom, "max_zoom": c.max_zoom, "radius": c.radius, "extent": c.extent,
            "min_points": c.min_points, "generate_id": c.generate_id,
        },
        "mode": c.mode,
        "tiles": c.tiles,
        "extra_ids": c.extra_ids,
        "leaf_args": c.leaf_args,
    })
}

fn rust(c: &Case) -> Json {
    let geojson::GeoJSON::FeatureCollection(fs) = geojson::convert(&c.features).unwrap() else {
        panic!("not a feature collection")
    };
    let mut options = Options {
        min_zoom: c.min_zoom,
        max_zoom: c.max_zoom,
        radius: c.radius,
        extent: c.extent,
        min_points: c.min_points,
        generate_id: c.generate_id,
        ..Options::default()
    };
    set_map_reduce(&mut options, c.mode);
    let index = match Supercluster::new(&fs, options) {
        Ok(index) => index,
        Err(e) => return json!({"error": e}),
    };

    let mut ids = Vec::new();
    let mut tiles = Vec::new();
    for &(z, x, y) in &c.tiles {
        let tile = index.get_tile(z, x, y);
        tiles.push(features(&tile));
        for f in &tile {
            if let Some(Value::Uint(id)) = f.properties.get("cluster_id")
                && !ids.contains(&(*id as u32))
            {
                ids.push(*id as u32);
            }
        }
    }
    ids.extend(&c.extra_ids);

    let or_error = |r: Result<Json, String>| r.unwrap_or_else(|e| json!({"error": e}));
    let clusters: Vec<Json> = ids
        .iter()
        .map(|&id| {
            json!({
                "id": id,
                "children": or_error(index.get_children(id).map(|f| json!({"features": features(&f)}))),
                "leaves": c.leaf_args.iter().map(|&(limit, offset)| {
                    or_error(index.get_leaves(id, limit, offset).map(|f| json!({"features": features(&f)})))
                }).collect::<Vec<_>>(),
                "expansion": or_error(index.get_cluster_expansion_zoom(id).map(|z| json!({"zoom": z}))),
            })
        })
        .collect();
    json!({"tiles": tiles, "clusters": clusters})
}

#[test]
fn supercluster_matches_cpp() {
    let tile_features = std::cell::Cell::new(0);
    let clusters = std::cell::Cell::new(0);
    let cases = run_cases(1500, &case(), |c| {
        let expected = cpp(&request(&c));
        let actual = rust(&c);
        tile_features.set(
            tile_features.get()
                + expected["tiles"].as_array().map_or(0, |t| t.iter().map(|f| f.as_array().map_or(0, Vec::len)).sum()),
        );
        clusters.set(clusters.get() + expected["clusters"].as_array().map_or(0, Vec::len));
        prop_assert!(
            actual == expected,
            "request {}\nC++  {}\nRust {}",
            abbreviate(&request(&c)),
            abbreviate(&expected),
            abbreviate(&actual)
        );
        Ok(())
    });
    println!(
        "Supercluster: {cases} cases ({} tile features, {} cluster ids queried), 0 mismatches",
        tile_features.get(),
        clusters.get()
    );
}

#[test]
fn point_count_abbreviated_matches_cpp() {
    let mut cases = 0;
    let check = |n: u32| {
        let expected: Json = serde_json::from_str(&take(unsafe { ref_supercluster_abbreviation(n) })).unwrap();
        assert_eq!(json!(supercluster::abbreviate(n)), expected, "{n}");
    };
    for n in 0..=200_000 {
        check(n);
        cases += 1;
    }
    for n in [u32::MAX, u32::MAX - 1, 1 << 31, 999_999_999, 1_000_000_000] {
        check(n);
        cases += 1;
    }
    cases += run_cases(20_000, &any::<u32>(), |n| {
        check(n);
        Ok(())
    });
    println!("point_count_abbreviated: {cases} cases, 0 mismatches");
}
