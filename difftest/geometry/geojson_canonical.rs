// The canonical form of geojson_reference.cpp for the Rust results, the
// FFI wrappers around the reference, and proptest strategies for GeoJSON
// inputs. Shared by geojson_difftest.rs and geojson_vt_difftest.rs.

use feature::{Feature, Identifier, PropertyMap, Value};
use geojson::GeoJSON;
use geojson_vt::Tile;
use geometry::{Geometry, Point};
use proptest::prelude::*;
use serde_json::{Value as Json, json};

unsafe extern "C" {
    fn ref_geojson_free(p: *mut std::ffi::c_char);
    fn ref_geojson_convert(text: *const u8, len: usize) -> *mut std::ffi::c_char;
    fn ref_geojson_parse(text: *const u8, len: usize) -> *mut std::ffi::c_char;
    fn ref_geojson_stringify(text: *const u8, len: usize) -> *mut std::ffi::c_char;
    fn ref_geojson_stringify_doubles(x: f64, y: f64, property: f64) -> *mut std::ffi::c_char;
    fn ref_geojsonvt(text: *const u8, len: usize) -> *mut std::ffi::c_char;
    fn ref_geojson_to_tile(text: *const u8, len: usize) -> *mut std::ffi::c_char;
}

type RefFn = unsafe extern "C" fn(*const u8, usize) -> *mut std::ffi::c_char;

fn call_text(f: RefFn, text: &str) -> String {
    unsafe {
        let p = f(text.as_ptr(), text.len());
        let s = std::ffi::CStr::from_ptr(p).to_str().unwrap().to_owned();
        ref_geojson_free(p);
        s
    }
}

fn call(f: RefFn, text: &str) -> Json {
    serde_json::from_str(&call_text(f, text)).unwrap()
}

pub fn cpp_convert(text: &str) -> Json {
    call(ref_geojson_convert, text)
}

pub fn cpp_parse(text: &str) -> Json {
    call(ref_geojson_parse, text)
}

/// The bytes of `stringify(convert(text))`, or `error: <what>`.
pub fn cpp_stringify(text: &str) -> String {
    call_text(ref_geojson_stringify, text)
}

pub fn cpp_stringify_doubles(x: f64, y: f64, property: f64) -> String {
    unsafe {
        let p = ref_geojson_stringify_doubles(x, y, property);
        let s = std::ffi::CStr::from_ptr(p).to_str().unwrap().to_owned();
        ref_geojson_free(p);
        s
    }
}

pub fn cpp_geojsonvt(request: &Json) -> Json {
    call(ref_geojsonvt, &request.to_string())
}

pub fn cpp_geojson_to_tile(request: &Json) -> Json {
    call(ref_geojson_to_tile, &request.to_string())
}

// --- canonical form ---

pub trait Coord: Copy {
    fn canonical(self) -> Json;
}

impl Coord for f64 {
    fn canonical(self) -> Json {
        json!(self.to_bits())
    }
}

impl Coord for i16 {
    fn canonical(self) -> Json {
        json!(self)
    }
}

fn point<T: Coord>(p: &Point<T>) -> Json {
    json!([p.x.canonical(), p.y.canonical()])
}

fn points<T: Coord>(points: &[Point<T>]) -> Json {
    Json::Array(points.iter().map(point).collect())
}

fn rings<T: Coord>(rings: &[Vec<Point<T>>]) -> Json {
    Json::Array(rings.iter().map(|r| points(r)).collect())
}

pub fn geometry<T: Coord>(g: &Geometry<T>) -> Json {
    match g {
        Geometry::Empty => Json::Null,
        Geometry::Point(p) => json!({"t": "Point", "c": point(p)}),
        Geometry::MultiPoint(p) => json!({"t": "MultiPoint", "c": points(p)}),
        Geometry::LineString(p) => json!({"t": "LineString", "c": points(p)}),
        Geometry::MultiLineString(l) => json!({"t": "MultiLineString", "c": rings(l)}),
        Geometry::Polygon(r) => json!({"t": "Polygon", "c": rings(r)}),
        Geometry::MultiPolygon(p) => {
            json!({"t": "MultiPolygon", "c": Json::Array(p.iter().map(|p| rings(p)).collect())})
        }
        Geometry::GeometryCollection(g) => {
            json!({"t": "GeometryCollection", "g": Json::Array(g.iter().map(geometry).collect())})
        }
    }
}

pub fn value(v: &Value) -> Json {
    match v {
        Value::Null => json!(["n"]),
        Value::Bool(b) => json!(["b", b]),
        Value::Uint(u) => json!(["u", u]),
        Value::Int(i) => json!(["i", i]),
        Value::Double(d) => json!(["d", d.to_bits()]),
        Value::String(s) => json!(["s", s]),
        Value::Array(a) => json!(["a", Json::Array(a.iter().map(value).collect())]),
        Value::Object(o) => json!(["o", property_map(o)]),
    }
}

/// Sorted by key (BTreeMap order = std::map<std::string> order).
pub fn property_map(map: &PropertyMap) -> Json {
    Json::Array(map.iter().map(|(k, v)| json!([k, value(v)])).collect())
}

pub fn identifier(id: &Identifier) -> Json {
    match id {
        Identifier::Null => json!(["n"]),
        Identifier::Uint(u) => json!(["u", u]),
        Identifier::Int(i) => json!(["i", i]),
        Identifier::Double(d) => json!(["d", d.to_bits()]),
        Identifier::String(s) => json!(["s", s]),
    }
}

pub fn feature<T: Coord>(f: &Feature<T>) -> Json {
    json!({"g": geometry(&f.geometry), "p": property_map(&f.properties), "id": identifier(&f.id)})
}

pub fn features<T: Coord>(f: &[Feature<T>]) -> Json {
    Json::Array(f.iter().map(feature).collect())
}

pub fn geojson(g: &GeoJSON) -> Json {
    match g {
        GeoJSON::Geometry(g) => json!({"geometry": geometry(g)}),
        GeoJSON::Feature(f) => json!({"feature": feature(f)}),
        GeoJSON::FeatureCollection(c) => json!({"feature_collection": features(c)}),
    }
}

pub fn geojson_result(r: &Result<GeoJSON, geojson::Error>) -> Json {
    match r {
        Ok(g) => geojson(g),
        Err(e) => json!({"error": e.message()}),
    }
}

pub fn tile(t: &Tile) -> Json {
    json!({"features": features(&t.features), "num_points": t.num_points, "num_simplified": t.num_simplified})
}

/// Abbreviates a long JSON text for failure messages.
pub fn abbreviate(j: &Json) -> String {
    let s = j.to_string();
    if s.len() > 3000 { format!("{}...({} bytes)", &s[..3000], s.len()) } else { s }
}

// --- strategies ---

fn number(spread: f64) -> impl Strategy<Value = f64> {
    prop_oneof![
        8 => (-1.0f64..1.0).prop_map(move |t| t * spread),
        1 => (-1000i32..1000).prop_map(f64::from),
        1 => prop::sample::select(vec![0.0, -0.0, 180.0, -180.0, 90.0, -90.0, 85.0511287798066, 1e-300, 5e-324]),
    ]
}

/// A coordinate around `(cx, cy)` within `spread` degrees.
fn coordinate(cx: f64, cy: f64, spread: f64) -> impl Strategy<Value = Json> {
    (number(spread), number(spread)).prop_map(move |(dx, dy)| json!([cx + dx, cy + dy]))
}

fn ring(cx: f64, cy: f64, spread: f64) -> impl Strategy<Value = Json> {
    prop::collection::vec(coordinate(cx, cy, spread), 3..=20).prop_map(|mut points| {
        points.push(points[0].clone());
        Json::Array(points)
    })
}

fn polygon(cx: f64, cy: f64, spread: f64) -> impl Strategy<Value = Json> {
    (ring(cx, cy, spread), prop::collection::vec(ring(cx, cy, spread / 4.0), 0..=2)).prop_map(|(outer, holes)| {
        let mut rings = vec![outer];
        rings.extend(holes);
        Json::Array(rings)
    })
}

fn line(cx: f64, cy: f64, spread: f64) -> impl Strategy<Value = Json> {
    prop::collection::vec(coordinate(cx, cy, spread), 2..=30).prop_map(Json::Array)
}

/// A GeoJSON geometry object (or null) around `(cx, cy)`.
pub fn geometry_json(cx: f64, cy: f64, spread: f64) -> impl Strategy<Value = Json> {
    let leaf = prop_oneof![
        coordinate(cx, cy, spread).prop_map(|c| json!({"type": "Point", "coordinates": c})),
        prop::collection::vec(coordinate(cx, cy, spread), 1..=6)
            .prop_map(|c| json!({"type": "MultiPoint", "coordinates": c})),
        line(cx, cy, spread).prop_map(|c| json!({"type": "LineString", "coordinates": c})),
        prop::collection::vec(line(cx, cy, spread), 1..=3)
            .prop_map(|c| json!({"type": "MultiLineString", "coordinates": c})),
        polygon(cx, cy, spread).prop_map(|c| json!({"type": "Polygon", "coordinates": c})),
        prop::collection::vec(polygon(cx, cy, spread), 1..=3)
            .prop_map(|c| json!({"type": "MultiPolygon", "coordinates": c})),
    ];
    let leaf2 = leaf.clone();
    prop_oneof![
        10 => leaf,
        1 => Just(Json::Null),
        2 => prop::collection::vec(leaf2, 0..=3).prop_map(|g| json!({"type": "GeometryCollection", "geometries": g})),
    ]
}

pub fn property_value() -> impl Strategy<Value = Json> {
    let scalar = prop_oneof![
        Just(Json::Null),
        any::<bool>().prop_map(Json::Bool),
        any::<u64>().prop_map(|u| json!(u)),
        any::<i64>().prop_map(|i| json!(i)),
        (-1e6f64..1e6).prop_map(|d| json!(d)),
        "[a-z ]{0,8}".prop_map(Json::String),
    ];
    scalar.prop_recursive(2, 8, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..3).prop_map(Json::Array),
            prop::collection::btree_map("[a-c]", inner, 0..3).prop_map(|m| Json::Object(m.into_iter().collect())),
        ]
    })
}

pub fn properties() -> impl Strategy<Value = Json> {
    prop::collection::btree_map(
        prop::sample::select(vec!["a", "b", "name", "mapbox_clip_start", "mapbox_clip_end", "cluster"]),
        property_value(),
        0..=3,
    )
    .prop_map(|m| Json::Object(m.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()))
}

pub fn id() -> impl Strategy<Value = Option<Json>> {
    prop_oneof![
        3 => Just(None),
        1 => any::<u32>().prop_map(|u| Some(json!(u))),
        1 => any::<i64>().prop_map(|i| Some(json!(i))),
        1 => (-1e3f64..1e3).prop_map(|d| Some(json!(d))),
        1 => "[a-z0-9]{0,5}".prop_map(|s| Some(json!(s))),
    ]
}

pub fn feature_json(cx: f64, cy: f64, spread: f64) -> impl Strategy<Value = Json> {
    (geometry_json(cx, cy, spread), properties(), id(), any::<bool>()).prop_map(|(g, p, id, with_props)| {
        let mut f = serde_json::Map::new();
        f.insert("type".to_owned(), json!("Feature"));
        f.insert("geometry".to_owned(), g);
        if with_props {
            f.insert("properties".to_owned(), p);
        }
        if let Some(id) = id {
            f.insert("id".to_owned(), id);
        }
        Json::Object(f)
    })
}

/// A cluster centre and spread: anywhere on (or a bit beyond) the world,
/// from continent-sized to street-sized.
pub fn centre() -> impl Strategy<Value = (f64, f64, f64)> {
    (
        prop_oneof![4 => -180.0f64..180.0, 1 => -400.0f64..400.0],
        prop_oneof![4 => -80.0f64..80.0, 1 => -95.0f64..95.0],
        prop::sample::select(vec![60.0, 10.0, 1.0, 0.1, 0.01, 0.001]),
    )
}

/// A GeoJSON document: a feature collection, a single feature or a bare
/// geometry, around one centre.
pub fn geojson_json() -> impl Strategy<Value = (Json, (f64, f64, f64))> {
    centre().prop_flat_map(|(cx, cy, spread)| {
        (
            prop_oneof![
                6 => prop::collection::vec(feature_json(cx, cy, spread), 0..=8)
                    .prop_map(|f| json!({"type": "FeatureCollection", "features": f})),
                1 => feature_json(cx, cy, spread),
                1 => geometry_json(cx, cy, spread).prop_filter("null is not GeoJSON", |g| !g.is_null()),
            ],
            Just((cx, cy, spread)),
        )
    })
}
