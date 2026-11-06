// Difftest of //vendor/geojson-vt against mapbox::geojsonvt
// (geojson_reference.cpp): for random GeoJSON (clusters of every geometry
// type, from continent- to street-sized, across the antimeridian, with
// properties and ids) and random options, a sequence of getTile requests at
// many zooms and tile coordinates (inside, around and outside the data,
// wrapped and out-of-range x, too-high zooms) must give exactly the same
// tiles (geometry, properties incl. mapbox_clip_*, ids, num_points,
// num_simplified), errors, empty-tile identity, total, stats and internal
// tile cache. geoJSONToTile is compared the same way, with and without wrap
// and clip (unclipped coordinates overflow int16_t).

use geojson_canonical::*;
use geojson_vt::{EMPTY_TILE, GeoJSONVT, Options, TileOptions, geojson_to_tile};
use geometry_difftest_support::run_cases;
use proptest::prelude::*;
use serde_json::{Value as Json, json};

fn tile_options() -> impl Strategy<Value = TileOptions> {
    (
        prop::sample::select(vec![0.0, 0.5, 3.0, 20.0]),
        prop::sample::select(vec![256u16, 512, 4096, 8192]),
        prop::sample::select(vec![0u16, 16, 64, 128, 512, 2048]),
        any::<bool>(),
    )
        .prop_map(|(tolerance, extent, buffer, line_metrics)| TileOptions {
            tolerance,
            extent,
            buffer,
            line_metrics,
        })
}

fn options() -> impl Strategy<Value = Options> {
    (tile_options(), 0u8..=16, 0u8..=6, prop::sample::select(vec![0u32, 5, 50, 100000]), any::<bool>()).prop_map(
        |(tile, max_zoom, index_max_zoom, index_max_points, generate_id)| Options {
            tile,
            max_zoom,
            index_max_zoom,
            index_max_points,
            generate_id,
        },
    )
}

fn options_json(o: &Options) -> Json {
    json!({
        "tolerance": o.tile.tolerance,
        "extent": o.tile.extent,
        "buffer": o.tile.buffer,
        "line_metrics": o.tile.line_metrics,
        "max_zoom": o.max_zoom,
        "index_max_zoom": o.index_max_zoom,
        "index_max_points": o.index_max_points,
        "generate_id": o.generate_id,
    })
}

fn tile_options_json(o: &TileOptions) -> Json {
    json!({"tolerance": o.tolerance, "extent": o.extent, "buffer": o.buffer, "line_metrics": o.line_metrics})
}

/// The tile coordinates of `(lng, lat)` at zoom `z` (may be outside the
/// world for out-of-range coordinates).
fn tile_of(lng: f64, lat: f64, z: u8) -> (i64, i64) {
    let n = f64::from(1u32 << z);
    let x = (lng / 360.0 + 0.5) * n;
    let sine = (lat.clamp(-85.0, 85.0) * std::f64::consts::PI / 180.0).sin();
    let y = (0.5 - 0.25 * ((1.0 + sine) / (1.0 - sine)).ln() / std::f64::consts::PI) * n;
    (x.floor() as i64, y.floor() as i64)
}

/// A request: (z, dx, dy, x kind, y kind) around the data's centre.
fn queries() -> impl Strategy<Value = Vec<(u8, i64, i64, u8, u8)>> {
    prop::collection::vec((0u8..=17, -2i64..=2, -2i64..=2, 0u8..8, 0u8..8), 0..=12)
}

fn resolve_queries(raw: &[(u8, i64, i64, u8, u8)], (cx, cy, _): (f64, f64, f64), max_zoom: u8) -> Vec<(u8, u32, u32)> {
    raw.iter()
        .map(|&(z, dx, dy, xk, yk)| {
            // Mostly valid zooms; z = 17 asks for maxZoom + 1 (an error).
            let z = if z == 17 { max_zoom + 1 } else { z % (max_zoom + 1) };
            let (tx, ty) = tile_of(cx, cy, z);
            let n = 1i64 << z;
            let x = match xk {
                0 => tx + dx + n,
                1 => i64::from(u32::MAX) - dx.abs(),
                2 => tx + dx - n,
                _ => tx + dx,
            };
            let y = match yk {
                0 => ty + dy + n,
                1 => dy.abs(),
                2 => ty + dy,
                _ => (ty + dy).clamp(0, n - 1),
            };
            (z, x as u32, y as u32)
        })
        .collect()
}

fn rust_geojsonvt(doc: &Json, options: Options, queries: &[(u8, u32, u32)]) -> Json {
    let geojson = match geojson::convert(doc) {
        Ok(g) => g,
        Err(e) => return json!({"error": e.message()}),
    };
    let mut index = GeoJSONVT::from_geojson(&geojson, options);
    let mut tiles = Vec::new();
    for &(z, x, y) in queries {
        tiles.push(match index.get_tile(z, x, y) {
            Ok(t) => json!({"empty_tile": std::ptr::eq(t, &EMPTY_TILE), "tile": tile(t)}),
            Err(e) => json!({"error": e}),
        });
    }
    let stats: Vec<Json> = index.stats.iter().map(|(z, n)| json!([z, n])).collect();
    let mut internal: Vec<[u32; 4]> = index
        .get_internal_tiles()
        .values()
        .map(|t| [u32::from(t.z), t.x, t.y, t.source_features.len() as u32])
        .collect();
    internal.sort();
    internal.dedup();
    json!({"tiles": tiles, "total": index.total, "stats": stats, "internal": internal})
}

#[test]
fn index_matches_cpp() {
    let strategy = (geojson_json(), options(), queries());
    let tiles = std::cell::Cell::new(0);
    let features = std::cell::Cell::new(0);
    let errors = std::cell::Cell::new(0);
    let cases = run_cases(2500, &strategy, |((doc, centre), options, raw)| {
        let queries = resolve_queries(&raw, centre, options.max_zoom);
        tiles.set(tiles.get() + queries.len());
        let request = json!({"geojson": doc, "options": options_json(&options), "queries": queries});
        let expected = cpp_geojsonvt(&request);
        let actual = rust_geojsonvt(&doc, options, &queries);
        for t in expected["tiles"].as_array().into_iter().flatten() {
            match t["tile"]["features"].as_array() {
                Some(f) => features.set(features.get() + f.len()),
                None => errors.set(errors.get() + 1),
            }
        }
        prop_assert!(
            actual == expected,
            "request {}\nC++  {}\nRust {}",
            abbreviate(&request),
            abbreviate(&expected),
            abbreviate(&actual)
        );
        Ok(())
    });
    println!(
        "GeoJSONVT: {cases} cases ({} getTile requests: {} tile features, {} errors), 0 mismatches",
        tiles.get(),
        features.get(),
        errors.get()
    );
}

#[test]
fn geojson_to_tile_matches_cpp() {
    let strategy = (geojson_json(), tile_options(), 0u8..=20, -2i64..=2, -2i64..=2, any::<bool>(), any::<bool>());
    let features = std::cell::Cell::new(0);
    let cases = run_cases(4000, &strategy, |((doc, (cx, cy, _)), options, z, dx, dy, wrap, clip)| {
        let (tx, ty) = tile_of(cx, cy, z);
        let (x, y) = ((tx + dx) as u32, (ty + dy) as u32);
        let request = json!({
            "geojson": doc, "z": z, "x": x, "y": y,
            "options": tile_options_json(&options), "wrap": wrap, "clip": clip,
        });
        let expected = cpp_geojson_to_tile(&request);
        let actual = match geojson::convert(&doc) {
            Ok(g) => tile(&geojson_to_tile(&g, z, x, y, &options, wrap, clip)),
            Err(e) => json!({"error": e.message()}),
        };
        features.set(features.get() + expected["features"].as_array().map_or(0, Vec::len));
        prop_assert!(
            actual == expected,
            "request {}\nC++  {}\nRust {}",
            abbreviate(&request),
            abbreviate(&expected),
            abbreviate(&actual)
        );
        Ok(())
    });
    println!("geoJSONToTile: {cases} cases ({} tile features), 0 mismatches", features.get());
}
