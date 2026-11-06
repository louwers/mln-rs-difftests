// Differential tests of the GeoJSON source and the tile pipeline against the
// C++ core (oracle_main.cpp, run as a process):
//
// 1. source conversion: every source of the render-test styles plus edge
//    cases through `convert_source` (GeoJSON options, URL, data, errors);
// 2. the tiles a TilePyramid creates and renders for the camera and size of
//    every GeoJSON render-test style (circle, fill, line, ...), in static and
//    continuous mode;
// 3. the features a GeoJSON tile hands to the worker (geojson-vt or
//    supercluster output through GeoJSONTileData, after fixupPolygons) for
//    those tiles and their neighbourhood.
//
// Responses are compared as text in the format of oracle_main.cpp.

use std::any::Any;
use std::cell::RefCell;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::rc::Rc;

use feature::{Identifier, Value};
use mln_actor::{RunLoop, TaggedScheduler, scheduler};
use mln_conversion::{Convertible, Error};
use mln_map::transform::{NullTransformObserver, Transform};
use mln_map_types::{CameraOptions, ConstrainMode, MapDebugOptions, MapMode, TileLodMode, ViewportMode};
use mln_renderer::tile::geojson_tile_data::GeoJSONTileData;
use mln_renderer::tile::tile::{Kind, Tile, TileBase};
use mln_renderer::tile::tile_observer::TileObserver;
use mln_renderer::tile_parameters::TileParameters;
use mln_renderer::tile_pyramid::TilePyramid;
use mln_renderer::tile_render_data::TileRenderData;
use mln_style::conversion::convert_source;
use mln_style::layer_properties::LayerProperties;
use mln_style::source::Source;
use mln_style::sources::geojson_source::{GeoJSONOptions, GeoJSONSource};
use mln_tile_id::geometry_tile_data::GeometryTileData;
use mln_tile_id::{CanonicalTileID, OverscaledTileID, UnwrappedTileID};
use mln_util::constants;
use mln_util::geo::LatLng;
use mln_util::identity::SimpleIdentity;
use mln_util::immutable::Immutable;
use mln_util::size::Size;

/// The C++ oracle process, shared by the tests.
struct Oracle {
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    _child: std::process::Child,
}

static ORACLE: std::sync::LazyLock<std::sync::Mutex<Oracle>> = std::sync::LazyLock::new(|| {
    let base = PathBuf::from(std::env::var("TEST_SRCDIR").unwrap());
    let path = base.join("_main/difftest/sources/sources_oracle");
    let mut child = std::process::Command::new(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("start the C++ oracle");
    let stdin = child.stdin.take().unwrap();
    let stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    std::sync::Mutex::new(Oracle { stdin, stdout, _child: child })
});

fn oracle(request: &serde_json::Value) -> String {
    let mut oracle = ORACLE.lock().unwrap_or_else(|e| e.into_inner());
    let line = serde_json::to_string(request).unwrap();
    writeln!(oracle.stdin, "{line}").unwrap();
    oracle.stdin.flush().unwrap();
    let mut response = String::new();
    oracle.stdout.read_line(&mut response).unwrap();
    response.trim_end_matches('\n').to_owned()
}

fn render_tests_dir() -> PathBuf {
    let base = PathBuf::from(std::env::var("TEST_SRCDIR").unwrap());
    for repo in ["maplibre+", "maplibre"] {
        let dir = base.join(repo).join("metrics/integration/render-tests");
        if dir.is_dir() {
            return dir;
        }
    }
    panic!("render-tests not found in the runfiles");
}

/// Every `style.json` under the render tests, sorted.
fn render_test_styles() -> Vec<(String, serde_json::Value)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.file_name().is_some_and(|n| n == "style.json") {
                out.push(path);
            }
        }
    }
    let root = render_tests_dir();
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let json: serde_json::Value = serde_json::from_str(&text).ok()?;
            let name = path.parent()?.strip_prefix(&root).ok()?.to_string_lossy().into_owned();
            Some((name, json))
        })
        .collect()
}

// The text format of oracle_main.cpp.

fn str_(s: &str) -> String {
    format!("s{}:{}", s.len(), s)
}

fn hex_bits(d: f64) -> String {
    format!("{:x}", d.to_bits())
}

fn dump_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
        Value::Uint(u) => format!("u{u}"),
        Value::Int(i) => format!("i{i}"),
        Value::Double(d) => format!("d{}", hex_bits(*d)),
        Value::String(s) => str_(s),
        Value::Array(array) => {
            let items: Vec<String> = array.iter().map(dump_value).collect();
            format!("[{}]", items.join(","))
        }
        Value::Object(object) => dump_object(object.iter()),
    }
}

fn dump_object<'a>(entries: impl Iterator<Item = (&'a String, &'a Value)>) -> String {
    let mut entries: Vec<(&String, &Value)> = entries.collect();
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let items: Vec<String> = entries.into_iter().map(|(k, v)| format!("{}={}", str_(k), dump_value(v))).collect();
    format!("{{{}}}", items.join(","))
}

fn dump_identifier(id: &Identifier) -> String {
    match id {
        Identifier::Null => "null".to_owned(),
        Identifier::Uint(u) => format!("u{u}"),
        Identifier::Int(i) => format!("i{i}"),
        Identifier::Double(d) => format!("d{}", hex_bits(*d)),
        Identifier::String(s) => str_(s),
    }
}

fn dump_options(o: &GeoJSONOptions) -> String {
    let keys: Vec<String> = o.cluster_properties.keys().map(|k| str_(k)).collect();
    format!(
        "minzoom={} maxzoom={} tileSize={} buffer={} tolerance={} lineMetrics={} cluster={} clusterRadius={} \
         clusterMaxZoom={} clusterMinPoints={} synchronousUpdate={} clusterProperties=[{}]",
        o.minzoom,
        o.maxzoom,
        o.tile_size,
        o.buffer,
        hex_bits(o.tolerance),
        i32::from(o.line_metrics),
        i32::from(o.cluster),
        o.cluster_radius,
        o.cluster_max_zoom,
        o.cluster_min_points,
        i32::from(o.synchronous_update),
        keys.join(",")
    )
}

fn convert(json: &serde_json::Value, id: &str) -> Result<Box<dyn Source>, String> {
    let mut error = Error::default();
    convert_source(&Convertible::from(json), &mut error, id).ok_or(error.message)
}

fn as_geojson(source: &dyn Source) -> Option<&GeoJSONSource> {
    source.as_any().downcast_ref::<GeoJSONSource>()
}

fn rust_source(json: &serde_json::Value, id: &str) -> String {
    let source = match convert(json, id) {
        Ok(source) => source,
        Err(error) => return format!("E {error}"),
    };
    let Some(geojson) = as_geojson(&*source) else {
        return format!("T {}", source.get_type() as i32);
    };
    let mut out = format!("G id={} {}", str_(&source.get_id()), dump_options(&geojson.get_options()));
    out += &format!(" url={}", geojson.get_url().map_or("none".to_owned(), |url| str_(&url)));
    let has_data = geojson.impl_().get_data().and_then(|d| d.upgrade()).is_some();
    out += &format!(" data={}", if has_data { "yes" } else { "no" });
    out += &format!(" sync={}", i32::from(geojson.is_update_synchronous()));
    out
}

/// A tile that only records its state (oracle_main.cpp's FakeTile).
struct FakeTile {
    base: TileBase,
}

impl FakeTile {
    fn complete(&self) {
        self.base.loaded.set(true);
        self.base.renderable.set(true);
        self.base.pending.set(false);
    }
}

impl Tile for FakeTile {
    fn base(&self) -> &TileBase {
        &self.base
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn create_render_data(&self) -> Box<dyn TileRenderData> {
        unreachable!()
    }
    fn cancel(&self) {}
    fn layer_properties_updated(&self, _layer_properties: &Immutable<dyn LayerProperties>) -> bool {
        false
    }
}

fn tile_string(id: &OverscaledTileID) -> String {
    format!("{}/{}/{}/{}/{}", id.overscaled_z, id.wrap, id.canonical.z, id.canonical.x, id.canonical.y)
}

fn unwrapped_string(id: &UnwrappedTileID) -> String {
    format!("{}/{}/{}/{}", id.wrap, id.canonical.z, id.canonical.x, id.canonical.y)
}

struct Camera {
    lng: f64,
    lat: f64,
    zoom: f64,
    bearing: f64,
    pitch: f64,
}

fn rust_tiles(source_json: &serde_json::Value, size: Size, camera: &Camera, continuous: bool) -> String {
    let source = match convert(source_json, "source") {
        Ok(source) => source,
        Err(error) => return format!("E {error}"),
    };
    let Some(geojson) = as_geojson(&*source) else {
        return "E not geojson".to_owned();
    };
    let impl_ = geojson.impl_();

    let mut transform =
        Transform::new(Rc::new(NullTransformObserver), ConstrainMode::HeightOnly, ViewportMode::Default);
    transform.resize(size).unwrap();
    let camera_options = CameraOptions {
        center: Some(LatLng::new(camera.lat, camera.lng).unwrap()),
        zoom: Some(camera.zoom),
        bearing: Some(camera.bearing),
        pitch: Some(camera.pitch),
        ..Default::default()
    };
    transform.jump_to(&camera_options);

    let parameters = TileParameters {
        pixel_ratio: 1.0,
        debug_options: MapDebugOptions::NO_DEBUG,
        transform_state: transform.get_state(),
        file_source: None,
        mode: if continuous { MapMode::Continuous } else { MapMode::Static },
        prefetch_zoom_delta: constants::DEFAULT_PREFETCH_ZOOM_DELTA,
        thread_pool: TaggedScheduler::new(scheduler::get_background(), SimpleIdentity::new()),
        tile_lod_min_radius: 3.0,
        tile_lod_scale: 1.0,
        tile_lod_pitch_threshold: (60.0 / 180.0) * std::f64::consts::PI,
        tile_lod_zoom_shift: 0.0,
        tile_lod_mode: TileLodMode::Default,
        global_state: None,
        is_update_synchronous: false,
        capture_rendered_features: false,
        image_manager: mln_renderer::image_manager::ImageManager::create(),
        glyph_manager: mln_renderer::text::glyph_manager::GlyphManager::new(None),
        dynamic_texture_atlas: std::sync::Arc::new(mln_renderer::gfx::dynamic_texture_atlas::DynamicTextureAtlas::new()),
    };

    let out = RefCell::new(String::new());
    let mut pyramid = TilePyramid::new(parameters.thread_pool.clone());
    let create_tile = |id: &OverscaledTileID, observer: Rc<dyn TileObserver>| -> Option<Box<dyn Tile>> {
        out.borrow_mut().push_str(&format!(" C{}", tile_string(id)));
        Some(Box::new(FakeTile { base: TileBase::new(Kind::Geometry, *id, "source".to_owned(), Some(observer)) }))
    };
    let layers: Vec<Immutable<dyn LayerProperties>> = Vec::new();
    pyramid.update(
        &layers,
        true,
        false,
        &parameters,
        &*impl_,
        constants::TILE_SIZE_I,
        impl_.get_zoom_range(),
        None,
        create_tile,
    );
    let mut out = out.into_inner();
    out += " |";
    for (id, _) in pyramid.get_rendered_tiles() {
        out += &format!(" R{}", unwrapped_string(id));
    }
    out += " |";
    for tile in pyramid.get_tiles().values() {
        tile.as_any().downcast_ref::<FakeTile>().unwrap().complete();
    }
    let created = RefCell::new(String::new());
    pyramid.update(
        &layers,
        true,
        false,
        &parameters,
        &*impl_,
        constants::TILE_SIZE_I,
        impl_.get_zoom_range(),
        None,
        |id: &OverscaledTileID, observer: Rc<dyn TileObserver>| -> Option<Box<dyn Tile>> {
            created.borrow_mut().push_str(&format!(" C{}", tile_string(id)));
            Some(Box::new(FakeTile { base: TileBase::new(Kind::Geometry, *id, "source".to_owned(), Some(observer)) }))
        },
    );
    out += &created.into_inner();
    out += " |";
    for (id, tile) in pyramid.get_rendered_tiles() {
        out += &format!(" R{}={}", unwrapped_string(id), tile_string(&tile.id()));
    }
    out += " |";
    for id in pyramid.get_tiles().keys() {
        out += &format!(" T{}", tile_string(id));
    }
    out += &format!(" | loaded={}", i32::from(pyramid.is_loaded()));
    format!("OK{out}")
}

fn rust_features(source_json: &serde_json::Value, tiles: &[CanonicalTileID]) -> String {
    let source = match convert(source_json, "source") {
        Ok(source) => source,
        Err(error) => return format!("E {error}"),
    };
    let Some(geojson) = as_geojson(&*source) else {
        return "E not geojson".to_owned();
    };
    let Some(data) = geojson.impl_().get_data().and_then(|d| d.upgrade()) else {
        return "E no data".to_owned();
    };

    let mut out = "OK".to_owned();
    for id in tiles {
        out += &format!(" TILE {}/{}/{}", id.z, id.x, id.y);
        let features = Rc::new(RefCell::new(None));
        let f = features.clone();
        data.get_tile(id, Box::new(move |tile_features| *f.borrow_mut() = Some(tile_features)), true);
        let tile_features = features.borrow_mut().take().expect("synchronous getTile");
        let tile_data = GeoJSONTileData::new(tile_features);
        let layer = tile_data.get_layer("").unwrap();
        out += &format!(" n={} name={}", layer.feature_count(), str_(&layer.get_name()));
        for i in 0..layer.feature_count() {
            let feature = layer.get_feature(i);
            out += &format!(" F type={}", feature.get_type() as i32);
            out += &format!(" id={}", dump_identifier(&feature.get_id()));
            out += &format!(" props={}", dump_object(feature.get_properties().iter()));
            let geometries = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut geom = " geom=[".to_owned();
                for ring in feature.get_geometries().iter() {
                    geom += "(";
                    for p in ring {
                        geom += &format!("{},{};", p.x, p.y);
                    }
                    geom += ")";
                }
                geom + "]"
            }));
            match geometries {
                Ok(geom) => out += &geom,
                Err(payload) => {
                    let message = payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                        .unwrap_or_default();
                    out += &format!(" geomerror={message}");
                }
            }
        }
    }
    out.replace('\n', " ")
}

/// The GeoJSON sources of the render-test styles: (test, source id, JSON).
fn render_test_geojson_sources() -> Vec<(String, String, serde_json::Value, serde_json::Value)> {
    let mut out = Vec::new();
    for (name, style) in render_test_styles() {
        let Some(sources) = style.get("sources").and_then(|s| s.as_object()) else {
            continue;
        };
        for (id, source) in sources {
            if source.get("type").and_then(|t| t.as_str()) == Some("geojson") {
                out.push((name.clone(), id.clone(), source.clone(), style.clone()));
            }
        }
    }
    out
}

/// Edge cases of the GeoJSON source conversion (geojson.cpp,
/// geojson_options.cpp, source.cpp). Out-of-range numbers (UB in the C++
/// `static_cast`s) are not compared (POLICY CHANGE, COORDINATION.md).
fn edge_case_sources() -> Vec<serde_json::Value> {
    let point = serde_json::json!({"type": "Point", "coordinates": [1, 2]});
    let fc = serde_json::json!({"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"a": 1, "b": "x"}, "geometry": {"type": "Point", "coordinates": [0, 0]}},
        {"type": "Feature", "properties": {"a": 2}, "geometry": {"type": "Point", "coordinates": [0.001, 0.001]}},
        {"type": "Feature", "properties": {"a": 3}, "geometry": {"type": "Point", "coordinates": [40, 40]}}
    ]});
    let mut cases = vec![
        serde_json::json!(1),
        serde_json::json!([]),
        serde_json::json!({}),
        serde_json::json!({"type": 1}),
        serde_json::json!({"type": "nope"}),
        serde_json::json!({"type": "geojson"}),
        serde_json::json!({"type": "geojson", "data": 1}),
        serde_json::json!({"type": "geojson", "data": true}),
        serde_json::json!({"type": "geojson", "data": "local://geojson/x.geojson"}),
        serde_json::json!({"type": "geojson", "data": {}}),
        serde_json::json!({"type": "geojson", "data": {"type": "Bogus"}}),
        serde_json::json!({"type": "geojson", "data": {"type": "Point"}}),
        serde_json::json!({"type": "geojson", "data": {"type": "Point", "coordinates": [1]}}),
        serde_json::json!({"type": "geojson", "data": {"type": "FeatureCollection"}}),
        serde_json::json!({"type": "geojson", "data": {"type": "FeatureCollection", "features": 1}}),
        serde_json::json!({"type": "geojson", "data": {"type": "Feature"}}),
        serde_json::json!({"type": "geojson", "data": point}),
        serde_json::json!({"type": "geojson", "data": fc, "cluster": true}),
        serde_json::json!({"type": "geojson", "data": fc, "cluster": false}),
        serde_json::json!({"type": "geojson", "data": fc, "cluster": 1}),
        serde_json::json!({"type": "geojson", "data": fc, "cluster": true, "clusterRadius": 10, "clusterMaxZoom": 3, "clusterMinPoints": 3}),
        serde_json::json!({"type": "geojson", "data": fc, "cluster": true, "clusterProperties": 1}),
        serde_json::json!({"type": "geojson", "data": fc, "cluster": true, "clusterProperties": {"sum": ["+", ["get", "a"]]}}),
        serde_json::json!({"type": "geojson", "data": fc, "cluster": true, "clusterProperties": {"sum": [["+", ["accumulated"], ["get", "sum"]], ["get", "a"]]}}),
        serde_json::json!({"type": "geojson", "data": fc, "cluster": true, "clusterProperties": {"sum": ["+"]}}),
        serde_json::json!({"type": "geojson", "data": fc, "cluster": true, "clusterProperties": {"sum": [1, ["get", "a"]]}}),
        serde_json::json!({"type": "geojson", "data": fc, "cluster": true, "clusterProperties": {"sum": ["+", ["bogus"]]}}),
        serde_json::json!({"type": "geojson", "data": fc, "cluster": true, "clusterProperties": {"max": ["max", ["get", "a"]], "s": ["concat", ["get", "b"]]}}),
        serde_json::json!({"type": "geojson", "data": point, "cluster": true}),
        serde_json::json!({"type": "geojson", "data": point, "minzoom": 2, "maxzoom": 9.7, "buffer": 64, "tolerance": 1.5}),
        serde_json::json!({"type": "geojson", "data": point, "minzoom": "2"}),
        serde_json::json!({"type": "geojson", "data": point, "maxzoom": null}),
        serde_json::json!({"type": "geojson", "data": point, "tolerance": "x"}),
        serde_json::json!({"type": "geojson", "data": point, "lineMetrics": true}),
        serde_json::json!({"type": "geojson", "data": point, "lineMetrics": 0}),
        serde_json::json!({"type": "geojson", "data": point, "synchronousUpdate": true}),
        serde_json::json!({"type": "geojson", "data": point, "synchronousUpdate": "yes"}),
        serde_json::json!({"type": "raster", "url": "x"}),
        serde_json::json!({"type": "vector", "tiles": ["x"]}),
        serde_json::json!({"type": "image"}),
    ];
    // Non-point features in a clustered source: C++ throws
    // bad_variant_access out of the conversion.
    cases.push(
        serde_json::json!({"type": "geojson", "cluster": true, "data": {"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {}, "geometry": {"type": "LineString", "coordinates": [[0, 0], [1, 1]]}}]}}),
    );
    cases
}

#[test]
fn source_conversion() {
    let _run_loop = RunLoop::new();
    let mut inputs: Vec<(String, serde_json::Value)> =
        render_test_geojson_sources().into_iter().map(|(_, id, json, _)| (id, json)).collect();
    // Every source of every style, of any type.
    for (_, style) in render_test_styles() {
        if let Some(sources) = style.get("sources").and_then(|s| s.as_object()) {
            for (id, source) in sources {
                if source.get("type").and_then(|t| t.as_str()) != Some("geojson") {
                    inputs.push((id.clone(), source.clone()));
                }
            }
        }
    }
    for case in edge_case_sources() {
        inputs.push(("edge".to_owned(), case));
    }

    let mut mismatches = 0;
    let mut checked = 0;
    for (id, json) in &inputs {
        let cpp = oracle(&serde_json::json!({"cmd": "source", "id": id, "json": json.to_string()}));
        let rust = rust_source(json, id);
        // C++ lets the Supercluster exception escape the conversion; the
        // port returns it as the conversion error (see DEVIATIONS.md).
        let cpp = cpp.replacen("E exception: ", "E ", 1);
        // Sources the port doesn't convert yet (vector, raster, ...): C++
        // converts them, the port reports "not ported yet".
        if rust.contains("is not ported yet") {
            continue;
        }
        checked += 1;
        if cpp != rust {
            mismatches += 1;
            if mismatches <= 20 {
                eprintln!("source mismatch for {json}\n  C++:  {cpp}\n  Rust: {rust}");
            }
        }
    }
    eprintln!("source_conversion: {checked} sources compared, {mismatches} mismatches");
    assert!(checked > 100);
    assert_eq!(mismatches, 0);
}

fn camera_of(style: &serde_json::Value) -> Camera {
    let center = style.get("center").and_then(|c| c.as_array());
    Camera {
        lng: center.and_then(|c| c.first()).and_then(|v| v.as_f64()).unwrap_or(0.0),
        lat: center.and_then(|c| c.get(1)).and_then(|v| v.as_f64()).unwrap_or(0.0),
        zoom: style.get("zoom").and_then(|v| v.as_f64()).unwrap_or(0.0),
        bearing: style.get("bearing").and_then(|v| v.as_f64()).unwrap_or(0.0),
        pitch: style.get("pitch").and_then(|v| v.as_f64()).unwrap_or(0.0),
    }
}

fn size_of(style: &serde_json::Value) -> Size {
    let test = style.get("metadata").and_then(|m| m.get("test"));
    let width = test.and_then(|t| t.get("width")).and_then(|v| v.as_u64()).unwrap_or(512);
    let height = test.and_then(|t| t.get("height")).and_then(|v| v.as_u64()).unwrap_or(512);
    Size::new(width as u32, height as u32)
}

/// The canonical ids of the tiles in a tiles response (`C` entries).
fn created_tiles(response: &str) -> Vec<CanonicalTileID> {
    let mut out = Vec::new();
    for word in response.split(' ') {
        if let Some(id) = word.strip_prefix('C') {
            let Ok(parts) = id.split('/').map(|p| p.parse::<u32>()).collect::<Result<Vec<u32>, _>>() else {
                continue;
            };
            if parts.len() != 5 {
                continue;
            }
            let canonical = CanonicalTileID::new(parts[2] as u8, parts[3], parts[4]);
            if !out.contains(&canonical) {
                out.push(canonical);
            }
        }
    }
    out
}

#[test]
fn tile_pyramid_and_features() {
    let _run_loop = RunLoop::new();
    let mut tile_mismatches = 0;
    let mut feature_mismatches = 0;
    let mut views = 0;
    let mut feature_tiles = 0;
    for (name, id, source, style) in render_test_geojson_sources() {
        let camera = camera_of(&style);
        let size = size_of(&style);
        if size.width == 0 || size.height == 0 {
            continue;
        }
        let mut tiles_for_features: Vec<CanonicalTileID> = Vec::new();
        for continuous in [false, true] {
            let request = serde_json::json!({
                "cmd": "tiles",
                "source": source.to_string(),
                "size": [size.width, size.height],
                "camera": {"lng": camera.lng, "lat": camera.lat, "zoom": camera.zoom, "bearing": camera.bearing, "pitch": camera.pitch},
                "mode": if continuous { "continuous" } else { "static" },
            });
            let cpp = oracle(&request);
            let rust = rust_tiles(&source, size, &camera, continuous);
            views += 1;
            if cpp != rust {
                tile_mismatches += 1;
                if tile_mismatches <= 10 {
                    eprintln!("tiles mismatch for {name} source {id}\n  C++:  {cpp}\n  Rust: {rust}");
                }
            }
            for tile in created_tiles(&cpp) {
                if !tiles_for_features.contains(&tile) {
                    tiles_for_features.push(tile);
                }
            }
        }
        if source.get("data").is_some_and(|d| d.is_string()) {
            continue;
        }
        // The neighbourhood: all tiles of z0..=2 and the parents and
        // children of the pyramid's tiles.
        let mut extra = Vec::new();
        for z in 0..=2u8 {
            for x in 0..(1u32 << z) {
                for y in 0..(1u32 << z) {
                    extra.push(CanonicalTileID::new(z, x, y));
                }
            }
        }
        for tile in &tiles_for_features {
            if tile.z > 0 {
                extra.push(tile.scaled_to(tile.z - 1));
            }
            if tile.z < 18 {
                extra.extend(tile.children());
            }
        }
        for tile in extra {
            if !tiles_for_features.contains(&tile) {
                tiles_for_features.push(tile);
            }
        }
        // Tiles above the source's maxzoom throw in geojson-vt (the pyramid
        // never requests them).
        let maxzoom = source.get("maxzoom").and_then(|v| v.as_f64()).map_or(18, |z| z as u8);
        tiles_for_features.retain(|t| t.z <= maxzoom);

        let tiles_json: Vec<[u32; 3]> = tiles_for_features.iter().map(|t| [u32::from(t.z), t.x, t.y]).collect();
        let cpp = oracle(&serde_json::json!({"cmd": "features", "source": source.to_string(), "tiles": tiles_json}));
        let rust = rust_features(&source, &tiles_for_features);
        feature_tiles += tiles_for_features.len();
        if cpp != rust {
            feature_mismatches += 1;
            if feature_mismatches <= 10 {
                let first = cpp.split(" TILE ").zip(rust.split(" TILE ")).find(|(a, b)| a != b);
                eprintln!(
                    "features mismatch for {name} source {id}: first differing tile\n  C++:  {:?}\n  Rust: {:?}",
                    first.map(|f| f.0),
                    first.map(|f| f.1)
                );
            }
        }
    }
    eprintln!(
        "tile_pyramid_and_features: {views} views ({tile_mismatches} mismatches), {feature_tiles} feature tiles ({feature_mismatches} source mismatches)"
    );
    assert!(views > 100);
    assert_eq!(tile_mismatches, 0);
    assert_eq!(feature_mismatches, 0);
}
