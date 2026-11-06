// Differential tests of the M3 layer code against the C++ core (oracle
// process, reference.cpp):
//
// - buckets as the tile worker builds them (style layer JSON -> render layer
//   evaluate -> layout -> createBucket): circle, fill and line vertices,
//   indexes and segments, and every layer's paint property binders (the
//   interleaved attribute data, vertex offsets, statistics and
//   interpolation factors at several zooms);
// - the polyline generator (joins, caps, round/miter limits, sharp corners,
//   overscaling, clip distances);
// - the fill generator with triangulated outlines (earcut, outlines);
// - the line atlas dash patterns.
//
// Both sides print the same text report; the comparison is on that text
// (floats as bit patterns).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::PathBuf;

use feature::{Identifier, PropertyMap, Value};
use geometry::Point;
use mln_conversion::Convertible;
use mln_gfx::polyline_generator::{PolylineGenerator, PolylineGeneratorDistances, PolylineGeneratorOptions};
use mln_gfx::shaders::segment::SegmentBase;
use mln_map_types::MapMode;
use mln_renderer::bucket::Bucket;
use mln_renderer::bucket_parameters::BucketParameters;
use mln_renderer::buckets::circle_bucket::CircleBucket;
use mln_renderer::buckets::fill_bucket::FillBucket;
use mln_renderer::buckets::line_bucket::{LineBucket, LineLayoutVertex};
use mln_renderer::gfx::fill_generator::generate_fill_and_outline_buffers;
use mln_renderer::layermanager::create_render_layer;
use mln_renderer::layout::circle_layout::CircleLayout;
use mln_renderer::layout::pattern_layout::{
    FillLayoutSpec, LineLayoutSpec, PatternLayout, fill_layout_is_sorted, line_layout_is_sorted,
};
use mln_renderer::line_atlas::{DashPatternTexture, LinePatternCap};
use mln_renderer::paint_property_binder::{
    BinderValue, CrossFadedPaintPropertyBinder, InterleavedVertexBuffer, PaintPropertyBinder,
};
use mln_renderer::render_layer::LayerRenderData;
use mln_style::conversion::layer::convert_layer;
use mln_style::expression::Image;
use mln_style::layer_properties::LayerProperties;
use mln_style::{PropertyEvaluationParameters, TransitionOptions, TransitionParameters};
use mln_style_types::image::{ImageDependencies, ImagePositions};
use mln_style_types::{LineCapType, LineJoinType};
use mln_tile_id::geometry_tile_data::{
    GeometryCollection, GeometryCoordinates, GeometryTileFeature, GeometryTileLayer,
};
use mln_tile_id::{CanonicalTileID, OverscaledTileID};
use mln_util::chrono::TimePoint;
use mln_util::geometry::FeatureType;
use mln_util::immutable::Immutable;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serde_json::{Value as Json, json};

// --- the oracle process ---

struct Oracle {
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    _child: std::process::Child,
}

static ORACLE: std::sync::LazyLock<std::sync::Mutex<Oracle>> = std::sync::LazyLock::new(|| {
    let base = PathBuf::from(std::env::var("TEST_SRCDIR").unwrap());
    let path = base.join("_main/difftest/layers/layers_oracle");
    let mut child = std::process::Command::new(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("start the C++ oracle");
    let stdin = child.stdin.take().unwrap();
    let stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    std::sync::Mutex::new(Oracle { stdin, stdout, _child: child })
});

fn oracle(request: &Json) -> String {
    use std::io::{BufRead, Write};
    let mut oracle = ORACLE.lock().unwrap_or_else(|e| e.into_inner());
    writeln!(oracle.stdin, "{request}").unwrap();
    oracle.stdin.flush().unwrap();
    let mut report = String::new();
    loop {
        let mut line = String::new();
        if oracle.stdout.read_line(&mut line).unwrap() == 0 {
            panic!("the C++ oracle exited (assert?) on {request}");
        }
        if line == "<<<END>>>\n" {
            return report;
        }
        report.push_str(&line);
    }
}

// --- report helpers (the same text as reference.cpp) ---

fn bits(f: f32) -> String {
    format!("{:08x}", f.to_bits())
}

fn hex(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for b in data {
        write!(out, "{b:02x}").unwrap();
    }
    out
}

fn dump_segments(out: &mut String, name: &str, segments: &[SegmentBase]) {
    out.push_str(name);
    for s in segments {
        write!(
            out,
            " {},{},{},{},{}",
            s.vertex_offset,
            s.index_offset,
            s.vertex_length,
            s.index_length,
            bits(s.sort_key)
        )
        .unwrap();
    }
    out.push('\n');
}

fn dump_indexes(out: &mut String, name: &str, indexes: &[u16]) {
    out.push_str(name);
    for i in indexes {
        write!(out, " {i}").unwrap();
    }
    out.push('\n');
}

fn dump_line_vertices(out: &mut String, name: &str, vertices: &[LineLayoutVertex]) {
    out.push_str(name);
    for v in vertices {
        write!(out, " {} {} {} {} {} {}", v.a1[0], v.a1[1], v.a2[0], v.a2[1], v.a2[2], v.a2[3]).unwrap();
    }
    out.push('\n');
}

fn dump_interleaved(out: &mut String, id: &str, buffer: &InterleavedVertexBuffer) {
    writeln!(
        out,
        "binders {id} stride={} count={} data={}",
        buffer.stride,
        buffer.vertex_count,
        hex(&buffer.shared_vertex_vector)
    )
    .unwrap();
}

fn binder_line(out: &mut String, name: &str, offset: usize, stats: Option<f32>, interp: Vec<f32>, count: usize) {
    write!(out, "binder {name} offset=").unwrap();
    if offset == usize::MAX {
        out.push_str("none");
    } else {
        write!(out, "{offset}").unwrap();
    }
    out.push_str(" stats=");
    match stats {
        Some(m) => out.push_str(&bits(m)),
        None => out.push_str("none"),
    }
    out.push_str(" interp=");
    for t in interp {
        write!(out, "{}/", bits(t)).unwrap();
    }
    writeln!(out, " count={count}").unwrap();
}

fn dump_binder<T: BinderValue>(
    out: &mut String,
    name: &str,
    binder: &PaintPropertyBinder<T>,
    buffer: &InterleavedVertexBuffer,
    zooms: &[f32],
) {
    binder_line(
        out,
        name,
        binder.vertex_offset,
        binder.statistics.max(),
        zooms.iter().map(|z| binder.interpolation_factor(*z)).collect(),
        binder.get_vertex_count(buffer),
    );
}

fn dump_faded_binder(
    out: &mut String,
    name: &str,
    binder: &CrossFadedPaintPropertyBinder<Image>,
    buffer: &InterleavedVertexBuffer,
    zooms: &[f32],
) {
    binder_line(
        out,
        name,
        binder.vertex_offset,
        None,
        zooms.iter().map(|z| binder.interpolation_factor(*z).0).collect(),
        binder.get_vertex_count(buffer),
    );
}

fn dump_circle(out: &mut String, bucket: &CircleBucket, zooms: &[f32]) {
    out.push_str("kind circle\nvertices");
    for v in &bucket.vertices {
        write!(out, " {} {}", v.a1[0], v.a1[1]).unwrap();
    }
    out.push('\n');
    dump_indexes(out, "triangles", &bucket.triangles);
    dump_segments(out, "segments", &bucket.segments);
    for (id, b) in &bucket.paint_property_binders {
        let b = b.read().unwrap();
        let buf = &b.interleaved_vertex_buffer;
        dump_interleaved(out, id, buf);
        dump_binder(out, "circle-blur", &b.circle_blur, buf, zooms);
        dump_binder(out, "circle-color", &b.circle_color, buf, zooms);
        dump_binder(out, "circle-opacity", &b.circle_opacity, buf, zooms);
        dump_binder(out, "circle-radius", &b.circle_radius, buf, zooms);
        dump_binder(out, "circle-stroke-color", &b.circle_stroke_color, buf, zooms);
        dump_binder(out, "circle-stroke-opacity", &b.circle_stroke_opacity, buf, zooms);
        dump_binder(out, "circle-stroke-width", &b.circle_stroke_width, buf, zooms);
    }
}

fn dump_fill(out: &mut String, bucket: &FillBucket, zooms: &[f32]) {
    out.push_str("kind fill\nvertices");
    for v in &bucket.vertices {
        write!(out, " {} {}", v.a1[0], v.a1[1]).unwrap();
    }
    out.push('\n');
    dump_indexes(out, "triangles", &bucket.triangles);
    dump_segments(out, "segments", &bucket.triangle_segments);
    dump_indexes(out, "basiclines", &bucket.basic_lines);
    dump_segments(out, "basicsegments", &bucket.basic_line_segments);
    for (id, b) in &bucket.paint_property_binders {
        let b = b.read().unwrap();
        let buf = &b.interleaved_vertex_buffer;
        dump_interleaved(out, id, buf);
        dump_binder(out, "fill-color", &b.fill_color, buf, zooms);
        dump_binder(out, "fill-opacity", &b.fill_opacity, buf, zooms);
        dump_binder(out, "fill-outline-color", &b.fill_outline_color, buf, zooms);
        dump_faded_binder(out, "fill-pattern", &b.fill_pattern, buf, zooms);
    }
}

fn dump_line(out: &mut String, bucket: &LineBucket, zooms: &[f32]) {
    out.push_str("kind line\n");
    dump_line_vertices(out, "vertices", &bucket.vertices);
    dump_indexes(out, "triangles", &bucket.triangles);
    dump_segments(out, "segments", &bucket.segments);
    for (id, b) in &bucket.paint_property_binders {
        let b = b.read().unwrap();
        let buf = &b.interleaved_vertex_buffer;
        dump_interleaved(out, id, buf);
        dump_binder(out, "line-blur", &b.line_blur, buf, zooms);
        dump_binder(out, "line-color", &b.line_color, buf, zooms);
        dump_binder(out, "line-floor-width", &b.line_floor_width, buf, zooms);
        dump_binder(out, "line-gap-width", &b.line_gap_width, buf, zooms);
        dump_binder(out, "line-offset", &b.line_offset, buf, zooms);
        dump_binder(out, "line-opacity", &b.line_opacity, buf, zooms);
        dump_faded_binder(out, "line-pattern", &b.line_pattern, buf, zooms);
        dump_binder(out, "line-width", &b.line_width, buf, zooms);
    }
}

// --- stub tile data (the C++ StubFeature / StubLayer) ---

struct FeatureData {
    type_: FeatureType,
    id: Identifier,
    properties: PropertyMap,
    geometry: GeometryCollection,
}

struct StubFeature<'a>(&'a FeatureData);

impl GeometryTileFeature for StubFeature<'_> {
    fn get_type(&self) -> FeatureType {
        self.0.type_
    }
    fn get_value(&self, key: &str) -> Option<Value> {
        self.0.properties.get(key).cloned()
    }
    fn get_properties(&self) -> &PropertyMap {
        &self.0.properties
    }
    fn get_id(&self) -> Identifier {
        self.0.id.clone()
    }
    fn get_geometries(&self) -> &GeometryCollection {
        &self.0.geometry
    }
}

struct StubLayer(Vec<FeatureData>);

impl GeometryTileLayer for StubLayer {
    fn feature_count(&self) -> usize {
        self.0.len()
    }
    fn get_feature(&self, index: usize) -> Box<dyn GeometryTileFeature + '_> {
        Box::new(StubFeature(&self.0[index]))
    }
    fn get_name(&self) -> String {
        "layer".to_owned()
    }
}

fn parse_geometry(rings: &Json) -> GeometryCollection {
    let mut geometry = GeometryCollection::new();
    for ring in rings.as_array().unwrap() {
        let coordinates: GeometryCoordinates = ring
            .as_array()
            .unwrap()
            .iter()
            .map(|p| Point::new(p[0].as_i64().unwrap() as i16, p[1].as_i64().unwrap() as i16))
            .collect();
        geometry.push(coordinates);
    }
    geometry
}

fn parse_value(v: &Json) -> Value {
    match v[1].as_str().unwrap() {
        "d" => Value::Double(v[2].as_f64().unwrap()),
        "i" => Value::Int(v[2].as_i64().unwrap()),
        "u" => Value::Uint(v[2].as_u64().unwrap()),
        "b" => Value::Bool(v[2].as_bool().unwrap()),
        _ => Value::String(v[2].as_str().unwrap().to_owned()),
    }
}

fn parse_id(v: &Json) -> Identifier {
    if v.is_null() {
        return Identifier::Null;
    }
    match v[0].as_str().unwrap() {
        "d" => Identifier::Double(v[1].as_f64().unwrap()),
        "i" => Identifier::Int(v[1].as_i64().unwrap()),
        "u" => Identifier::Uint(v[1].as_u64().unwrap()),
        _ => Identifier::String(v[1].as_str().unwrap().to_owned()),
    }
}

fn parse_features(array: &Json) -> Vec<FeatureData> {
    array
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let type_ = match f["type"].as_i64().unwrap() {
                1 => FeatureType::Point,
                2 => FeatureType::LineString,
                3 => FeatureType::Polygon,
                _ => FeatureType::Unknown,
            };
            let mut properties = PropertyMap::new();
            for p in f["props"].as_array().unwrap() {
                properties.insert(p[0].as_str().unwrap().to_owned(), parse_value(p));
            }
            FeatureData { type_, id: parse_id(&f["id"]), properties, geometry: parse_geometry(&f["geom"]) }
        })
        .collect()
}

// --- the Rust side of each request ---

fn rust_bucket_report(request: &Json) -> String {
    let mut out = String::new();
    let zoom = request["zoom"].as_f64().unwrap() as f32;
    let zooms: Vec<f32> = request["zooms"].as_array().unwrap().iter().map(|z| z.as_f64().unwrap() as f32).collect();

    let mut group: Vec<Immutable<dyn LayerProperties>> = Vec::new();
    for layer_json in request["layers"].as_array().unwrap() {
        let mut error = mln_conversion::Error::default();
        let Some(layer) = convert_layer(&Convertible::from(layer_json), &mut error) else {
            writeln!(out, "layer-error {}", error.message).unwrap();
            return out;
        };
        let Some(render_layer) = create_render_layer(&layer.base_impl()) else {
            out.push_str("no-render-layer\n");
            return out;
        };
        let mut render_layer = render_layer.borrow_mut();
        render_layer
            .transition(&TransitionParameters { now: TimePoint::max(), transition: TransitionOptions::default() });
        render_layer.evaluate(&PropertyEvaluationParameters::new(zoom));
        group.push(render_layer.base().evaluated_properties.clone());
    }

    let tile = &request["tile"];
    let tile_id = OverscaledTileID::with_zxy(
        request["oz"].as_u64().unwrap() as u8,
        0,
        tile[0].as_u64().unwrap() as u8,
        tile[1].as_u64().unwrap() as u32,
        tile[2].as_u64().unwrap() as u32,
    );
    let mode = if request["mode"].as_str().unwrap() == "continuous" { MapMode::Continuous } else { MapMode::Static };
    let type_info = group[0].base_impl.get_type_info();
    let parameters = BucketParameters {
        tile_id,
        mode,
        pixel_ratio: 1.0,
        layer_type: type_info,
        global_state: None,
        retain_features_by_id: false,
    };
    let source_layer = StubLayer(parse_features(&request["features"]));
    let mut image_dependencies = ImageDependencies::new();
    let available_images = BTreeSet::new();
    let mut render_data: HashMap<String, LayerRenderData> = HashMap::new();
    let canonical: CanonicalTileID = tile_id.canonical;
    let mut no_index = |_: &GeometryCollection, _: usize, _: &str, _: &str| {};

    let result = match type_info.type_ {
        "circle" => {
            let mut layout = CircleLayout::new(&parameters, &group, &source_layer);
            writeln!(out, "dependencies {}", layout.has_dependencies() as i32).unwrap();
            writeln!(out, "imagedeps {}", image_dependencies.len()).unwrap();
            layout.create_bucket_with(&source_layer, &mut no_index, &mut render_data, &canonical)
        }
        "fill" => {
            let sorted = fill_layout_is_sorted(&group[0]);
            let mut layout = PatternLayout::<FillLayoutSpec>::new(
                &parameters,
                &group,
                &source_layer,
                &mut image_dependencies,
                &available_images,
                sorted,
            );
            writeln!(out, "dependencies {}", layout.has_dependencies() as i32).unwrap();
            writeln!(out, "imagedeps {}", image_dependencies.len()).unwrap();
            layout.create_bucket_with(
                &source_layer,
                &ImagePositions::new(),
                &mut no_index,
                &mut render_data,
                &canonical,
            )
        }
        "line" => {
            let sorted = line_layout_is_sorted(&group[0]);
            let mut layout = PatternLayout::<LineLayoutSpec>::new(
                &parameters,
                &group,
                &source_layer,
                &mut image_dependencies,
                &available_images,
                sorted,
            );
            writeln!(out, "dependencies {}", layout.has_dependencies() as i32).unwrap();
            writeln!(out, "imagedeps {}", image_dependencies.len()).unwrap();
            layout.create_bucket_with(
                &source_layer,
                &ImagePositions::new(),
                &mut no_index,
                &mut render_data,
                &canonical,
            )
        }
        other => panic!("unexpected layer type {other}"),
    };
    if let Err(e) = result {
        writeln!(out, "exception {e}").unwrap();
        return out;
    }

    let sorted: BTreeMap<&String, &LayerRenderData> = render_data.iter().collect();
    let mut buckets: Vec<*const ()> = Vec::new();
    let mut bucket_refs: Vec<&dyn Bucket> = Vec::new();
    for (id, data) in &sorted {
        let ptr = std::sync::Arc::as_ptr(&data.bucket) as *const ();
        let n = match buckets.iter().position(|b| *b == ptr) {
            Some(n) => n,
            None => {
                buckets.push(ptr);
                bucket_refs.push(&*data.bucket);
                buckets.len() - 1
            }
        };
        writeln!(out, "layer {id} bucket {n} passes {}", data.layer_properties.render_passes).unwrap();
    }
    for (n, bucket) in bucket_refs.iter().enumerate() {
        writeln!(out, "bucket {n}").unwrap();
        match type_info.type_ {
            "circle" => dump_circle(&mut out, bucket.downcast_ref::<CircleBucket>().unwrap(), &zooms),
            "fill" => dump_fill(&mut out, bucket.downcast_ref::<FillBucket>().unwrap(), &zooms),
            "line" => dump_line(&mut out, bucket.downcast_ref::<LineBucket>().unwrap(), &zooms),
            _ => {}
        }
    }
    out
}

fn parse_cap(s: &str) -> LineCapType {
    match s {
        "round" => LineCapType::Round,
        "square" => LineCapType::Square,
        _ => LineCapType::Butt,
    }
}

fn parse_join(s: &str) -> LineJoinType {
    match s {
        "round" => LineJoinType::Round,
        "bevel" => LineJoinType::Bevel,
        _ => LineJoinType::Miter,
    }
}

fn get_segment(segment: &mut SegmentBase) -> &mut SegmentBase {
    segment
}

fn rust_polyline_report(request: &Json) -> String {
    let mut vertices: Vec<LineLayoutVertex> = Vec::new();
    let mut indexes: Vec<u16> = Vec::new();
    let mut segments: Vec<SegmentBase> = Vec::new();
    let options = PolylineGeneratorOptions {
        type_: if request["type"].as_i64().unwrap() == 3 { FeatureType::Polygon } else { FeatureType::LineString },
        join_type: parse_join(request["join"].as_str().unwrap()),
        miter_limit: request["miterLimit"].as_f64().unwrap() as f32,
        begin_cap: parse_cap(request["beginCap"].as_str().unwrap()),
        end_cap: parse_cap(request["endCap"].as_str().unwrap()),
        round_limit: request["roundLimit"].as_f64().unwrap() as f32,
        overscaling: request["overscaling"].as_u64().unwrap() as u32,
        clip_distances: request["clip"].as_array().map(|c| {
            PolylineGeneratorDistances::new(c[0].as_f64().unwrap(), c[1].as_f64().unwrap(), c[2].as_f64().unwrap())
        }),
    };
    {
        let mut generator = PolylineGenerator::new(
            &mut vertices,
            LineBucket::layout_vertex,
            &mut segments,
            &SegmentBase::new,
            get_segment,
            &mut indexes,
        );
        for line in request["lines"].as_array().unwrap() {
            let coordinates: GeometryCoordinates = line
                .as_array()
                .unwrap()
                .iter()
                .map(|p| Point::new(p[0].as_i64().unwrap() as i16, p[1].as_i64().unwrap() as i16))
                .collect();
            generator.generate(&coordinates, &options);
        }
    }
    let mut out = String::new();
    dump_line_vertices(&mut out, "vertices", &vertices);
    dump_indexes(&mut out, "indexes", &indexes);
    dump_segments(&mut out, "segments", &segments);
    out
}

fn rust_fillgen_report(request: &Json) -> String {
    let mut fill_vertices = Vec::new();
    let mut fill_indexes = Vec::new();
    let mut fill_segments = Vec::new();
    let mut line_vertices = Vec::new();
    let mut line_indexes = Vec::new();
    let mut line_segments = Vec::new();
    let mut basic_line_indexes = Vec::new();
    let mut basic_line_segments = Vec::new();
    let mut out = String::new();
    for geometry in request["geometries"].as_array().unwrap() {
        if generate_fill_and_outline_buffers(
            &parse_geometry(geometry),
            &mut fill_vertices,
            &mut fill_indexes,
            &mut fill_segments,
            &mut line_vertices,
            &mut line_indexes,
            &mut line_segments,
            &mut basic_line_indexes,
            &mut basic_line_segments,
        )
        .is_err()
        {
            out.push_str("exception\n");
            break;
        }
    }
    out.push_str("vertices");
    for v in &fill_vertices {
        let v: &mln_renderer::buckets::fill_bucket::FillLayoutVertex = v;
        write!(out, " {} {}", v.a1[0], v.a1[1]).unwrap();
    }
    out.push('\n');
    dump_indexes(&mut out, "triangles", &fill_indexes);
    dump_segments(&mut out, "segments", &fill_segments);
    dump_line_vertices(&mut out, "linevertices", &line_vertices);
    dump_indexes(&mut out, "lineindexes", &line_indexes);
    dump_segments(&mut out, "linesegments", &line_segments);
    dump_indexes(&mut out, "basiclines", &basic_line_indexes);
    dump_segments(&mut out, "basicsegments", &basic_line_segments);
    out
}

fn rust_dash_report(request: &Json) -> String {
    let from: Vec<f32> = request["from"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
    let to: Vec<f32> = request["to"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
    let cap = if request["cap"].as_str().unwrap() == "round" { LinePatternCap::Round } else { LinePatternCap::Square };
    let texture = DashPatternTexture::new(&from, &to, cap);
    let a = texture.get_from();
    let b = texture.get_to();
    let image = texture.get_image().unwrap();
    format!(
        "from {} {} {}\nto {} {} {}\nsize {}x{}\nimage {}\n",
        bits(a.width),
        bits(a.height),
        bits(a.y),
        bits(b.width),
        bits(b.height),
        bits(b.y),
        image.size.width,
        image.size.height,
        hex(&image.data)
    )
}

fn rust_report(request: &Json) -> String {
    match request["kind"].as_str().unwrap() {
        "bucket" => rust_bucket_report(request),
        "polyline" => rust_polyline_report(request),
        "fillgen" => rust_fillgen_report(request),
        "dash" => rust_dash_report(request),
        _ => "unknown kind\n".to_owned(),
    }
}

/// Compares both reports; on a difference, prints the first differing line.
fn check(request: &Json) -> Result<(), TestCaseError> {
    let expected = oracle(request);
    let actual = rust_report(request);
    if expected != actual {
        let first = expected.lines().zip(actual.lines()).position(|(a, b)| a != b);
        let detail = match first {
            Some(i) => format!(
                "line {i}:\n  C++:  {}\n  Rust: {}",
                expected.lines().nth(i).unwrap_or("").chars().take(600).collect::<String>(),
                actual.lines().nth(i).unwrap_or("").chars().take(600).collect::<String>()
            ),
            None => format!("line counts differ: C++ {} vs Rust {}", expected.lines().count(), actual.lines().count()),
        };
        return Err(TestCaseError::fail(format!("mismatch for {request}\n{detail}")));
    }
    Ok(())
}

fn run(cases: u32, strategy: impl Strategy<Value = Json>) {
    let mut runner = TestRunner::new(Config { cases, failure_persistence: None, ..Config::default() });
    runner.run(&strategy, |request| check(&request)).unwrap();
}

// --- input strategies ---

fn coord(lo: i64, hi: i64) -> impl Strategy<Value = Json> {
    (lo..hi, lo..hi).prop_map(|(x, y)| json!([x, y]))
}

fn feature_id() -> impl Strategy<Value = Json> {
    prop_oneof![
        Just(Json::Null),
        (0u64..100).prop_map(|v| json!(["u", v])),
        (-50i64..50).prop_map(|v| json!(["i", v])),
        (-10.0f64..10.0).prop_map(|v| json!(["d", v])),
        "[a-c]{0,2}".prop_map(|v| json!(["s", v])),
    ]
}

fn props() -> impl Strategy<Value = Json> {
    (
        prop_oneof![(-2.0f64..12.0).prop_map(|v| json!(["n", "d", v])), (0i64..9).prop_map(|v| json!(["n", "i", v])),],
        prop_oneof![
            Just("red"),
            Just("#00ff00"),
            Just("rgba(10, 20, 30, 0.5)"),
            Just("hsl(120, 50%, 50%)"),
            Just("transparent"),
            Just("not a color"),
        ],
        prop_oneof![Just("miter"), Just("bevel"), Just("round"), Just("bogus")],
        (-5.0f64..5.0),
        proptest::option::of((0.0f64..1.0, 0.0f64..1.0)),
    )
        .prop_map(|(n, c, j, k, clip)| {
            let mut v = vec![n, json!(["c", "s", c]), json!(["j", "s", j]), json!(["k", "d", k])];
            if let Some((a, b)) = clip {
                v.push(json!(["mapbox_clip_start", "d", a]));
                v.push(json!(["mapbox_clip_end", "d", b]));
            }
            Json::Array(v)
        })
}

/// A number-valued paint property: absent, constant, camera, source or
/// composite function.
fn number_property(lo: f64, hi: f64) -> impl Strategy<Value = Option<Json>> {
    prop_oneof![
        Just(None),
        (lo..hi).prop_map(|v| Some(json!(v))),
        (lo..hi, lo..hi, 0.0f64..10.0).prop_map(|(a, b, z)| Some(json!([
            "interpolate",
            ["linear"],
            ["zoom"],
            z,
            a,
            z + 5.0,
            b
        ]))),
        Just(Some(json!(["get", "n"]))),
        Just(Some(json!(["coalesce", ["get", "n"], 3]))),
        (0.0f64..8.0).prop_map(|z| Some(json!([
            "interpolate",
            ["linear"],
            ["zoom"],
            z,
            ["get", "n"],
            z + 4.0,
            ["*", 2, ["get", "n"]]
        ]))),
        (0.0f64..8.0).prop_map(|z| Some(json!(["step", ["zoom"], ["get", "n"], z, 4]))),
        (1.1f64..3.0).prop_map(|base| Some(json!({"stops": [[0, 1], [10, 6]], "base": base}))),
        Just(Some(json!({"property": "n", "stops": [[0, 1], [10, 6]]}))),
        Just(Some(json!({"property": "n", "stops": [[{"zoom": 0, "value": 0}, 1], [{"zoom": 10, "value": 10}, 7]]}))),
    ]
}

fn color_property() -> impl Strategy<Value = Option<Json>> {
    prop_oneof![
        Just(None),
        Just(Some(json!("red"))),
        Just(Some(json!("rgba(0, 0, 255, 0.4)"))),
        (0.0f64..10.0).prop_map(|z| Some(json!(["interpolate", ["linear"], ["zoom"], z, "red", z + 3.0, "blue"]))),
        Just(Some(json!(["to-color", ["get", "c"], "black"]))),
        Just(Some(json!(["get", "c"]))),
        (0.0f64..8.0).prop_map(|z| Some(json!([
            "interpolate",
            ["linear"],
            ["zoom"],
            z,
            ["to-color", ["get", "c"], "white"],
            z + 2.0,
            "green"
        ]))),
        Just(Some(json!(["case", [">", ["get", "n"], 5], "yellow", "purple"]))),
    ]
}

fn sort_key() -> impl Strategy<Value = Option<Json>> {
    prop_oneof![
        3 => Just(None),
        1 => Just(Some(json!(["get", "k"]))),
        1 => (-3.0f64..3.0).prop_map(|v| Some(json!(v))),
    ]
}

fn filter() -> impl Strategy<Value = Option<Json>> {
    prop_oneof![3 => Just(None), 1 => Just(Some(json!(["<", ["get", "n"], 6]))), 1 => Just(Some(json!(["has", "n"])))]
}

fn insert(object: &mut serde_json::Map<String, Json>, key: &str, value: Option<Json>) {
    if let Some(value) = value {
        object.insert(key.to_owned(), value);
    }
}

/// zoom, tile (z, x, y), overscaled z
fn tile() -> impl Strategy<Value = (f64, Json, u64)> {
    (0u64..15, 0u64..3).prop_flat_map(|(z, over)| {
        let n = 1u64 << z;
        (0..n, 0..n, 0.0f64..1.0).prop_map(move |(x, y, f)| ((z + over) as f64 + f, json!([z, x, y]), z + over))
    })
}

fn bucket_request(layers: Vec<Json>, features: Vec<Json>, tile: (f64, Json, u64), continuous: bool) -> Json {
    let (zoom, tile, oz) = tile;
    json!({
        "kind": "bucket",
        "layers": layers,
        "features": features,
        "tile": tile,
        "oz": oz,
        "mode": if continuous { "continuous" } else { "static" },
        "zoom": zoom,
        "zooms": [0.0, zoom, zoom + 0.25, zoom + 0.5, (zoom + 0.99).floor(), 22.0],
    })
}

fn circle_layer(id: &'static str) -> impl Strategy<Value = Json> {
    (
        (number_property(0.0, 20.0), color_property(), number_property(0.0, 1.0), number_property(0.0, 1.0)),
        (color_property(), number_property(0.0, 1.0), number_property(0.0, 5.0)),
        sort_key(),
        filter(),
    )
        .prop_map(
            move |((radius, color, blur, opacity), (stroke_color, stroke_opacity, stroke_width), sort_key, filter)| {
                let mut paint = serde_json::Map::new();
                insert(&mut paint, "circle-radius", radius);
                insert(&mut paint, "circle-color", color);
                insert(&mut paint, "circle-blur", blur);
                insert(&mut paint, "circle-opacity", opacity);
                insert(&mut paint, "circle-stroke-color", stroke_color);
                insert(&mut paint, "circle-stroke-opacity", stroke_opacity);
                insert(&mut paint, "circle-stroke-width", stroke_width);
                let mut layout = serde_json::Map::new();
                insert(&mut layout, "circle-sort-key", sort_key);
                let mut layer = json!({"id": id, "type": "circle", "source": "s", "paint": paint, "layout": layout});
                if let Some(filter) = filter {
                    layer["filter"] = filter;
                }
                layer
            },
        )
}

fn point_feature() -> impl Strategy<Value = Json> {
    (feature_id(), props(), proptest::collection::vec(proptest::collection::vec(coord(-300, 8500), 1..4), 1..3))
        .prop_map(|(id, props, geom)| json!({"type": 1, "id": id, "props": props, "geom": geom}))
}

/// A ring around a center: points at increasing (or decreasing) angles,
/// so mostly simple polygons, sometimes closed explicitly.
fn ring(max_radius: f64) -> impl Strategy<Value = Json> {
    (
        (-500.0f64..8700.0, -500.0f64..8700.0),
        proptest::collection::vec((0.0f64..1.0, 0.1f64..1.0), 3..12),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(move |((cx, cy), points, clockwise, close)| {
            let n = points.len();
            let mut coords: Vec<Json> = points
                .iter()
                .enumerate()
                .map(|(i, (jitter, r))| {
                    let a = (i as f64 + jitter * 0.9) / n as f64
                        * std::f64::consts::TAU
                        * if clockwise { -1.0 } else { 1.0 };
                    json!([
                        (cx + a.cos() * r * max_radius).round() as i64,
                        (cy + a.sin() * r * max_radius).round() as i64
                    ])
                })
                .collect();
            if close {
                coords.push(coords[0].clone());
            }
            Json::Array(coords)
        })
}

fn polygon_feature() -> impl Strategy<Value = Json> {
    (feature_id(), props(), proptest::collection::vec(ring(3000.0), 1..4))
        .prop_map(|(id, props, geom)| json!({"type": 3, "id": id, "props": props, "geom": geom}))
}

fn line_coords() -> impl Strategy<Value = Json> {
    prop_oneof![
        // random walks with sharp corners and duplicates
        (coord(-200, 8400), proptest::collection::vec((-1500i64..1500, -1500i64..1500, 0u8..4), 1..10)).prop_map(
            |(start, steps)| {
                let mut x = start[0].as_i64().unwrap();
                let mut y = start[1].as_i64().unwrap();
                let mut coords = vec![json!([x, y])];
                for (dx, dy, dup) in steps {
                    if dup == 0 {
                        coords.push(json!([x, y]));
                    }
                    x = (x + dx).clamp(-16000, 16000);
                    y = (y + dy).clamp(-16000, 16000);
                    coords.push(json!([x, y]));
                }
                Json::Array(coords)
            }
        ),
        ring(2000.0),
        // nearly straight and reversing lines
        (coord(0, 8000), -50i64..50, prop::bool::ANY).prop_map(|(start, d, reverse)| {
            let x = start[0].as_i64().unwrap();
            let y = start[1].as_i64().unwrap();
            let end = if reverse { json!([x, y + d]) } else { json!([x + 3000, y + d]) };
            json!([[x, y], [x + 1500, y], end])
        }),
    ]
}

fn line_feature() -> impl Strategy<Value = Json> {
    (feature_id(), props(), proptest::collection::vec(line_coords(), 1..3), prop_oneof![Just(2), Just(3)])
        .prop_map(|(id, props, geom, type_)| json!({"type": type_, "id": id, "props": props, "geom": geom}))
}

fn fill_layer(id: &'static str) -> impl Strategy<Value = Json> {
    (
        color_property(),
        number_property(0.0, 1.0),
        color_property(),
        proptest::option::of(any::<bool>()),
        sort_key(),
        filter(),
    )
        .prop_map(move |(color, opacity, outline, antialias, sort_key, filter)| {
            let mut paint = serde_json::Map::new();
            insert(&mut paint, "fill-color", color);
            insert(&mut paint, "fill-opacity", opacity);
            insert(&mut paint, "fill-outline-color", outline);
            insert(&mut paint, "fill-antialias", antialias.map(Json::Bool));
            let mut layout = serde_json::Map::new();
            insert(&mut layout, "fill-sort-key", sort_key);
            let mut layer = json!({"id": id, "type": "fill", "source": "s", "paint": paint, "layout": layout});
            if let Some(filter) = filter {
                layer["filter"] = filter;
            }
            layer
        })
}

fn line_layer(id: &'static str) -> impl Strategy<Value = Json> {
    (
        (color_property(), number_property(0.0, 20.0), number_property(0.0, 10.0), number_property(-10.0, 10.0)),
        (number_property(0.0, 3.0), number_property(0.0, 1.0)),
        (
            prop_oneof![
                Just(None),
                Just(Some(json!("miter"))),
                Just(Some(json!("bevel"))),
                Just(Some(json!("round"))),
                Just(Some(json!(["get", "j"])))
            ],
            prop_oneof![Just(None), Just(Some(json!("butt"))), Just(Some(json!("square"))), Just(Some(json!("round")))],
            proptest::option::of(0.5f64..10.0),
            proptest::option::of(0.5f64..3.0),
        ),
        sort_key(),
        filter(),
    )
        .prop_map(
            move |((color, width, gap, offset), (blur, opacity), (join, cap, miter, round), sort_key, filter)| {
                let mut paint = serde_json::Map::new();
                insert(&mut paint, "line-color", color);
                insert(&mut paint, "line-width", width);
                insert(&mut paint, "line-gap-width", gap);
                insert(&mut paint, "line-offset", offset);
                insert(&mut paint, "line-blur", blur);
                insert(&mut paint, "line-opacity", opacity);
                let mut layout = serde_json::Map::new();
                insert(&mut layout, "line-join", join);
                insert(&mut layout, "line-cap", cap);
                insert(&mut layout, "line-miter-limit", miter.map(|v| json!(v)));
                insert(&mut layout, "line-round-limit", round.map(|v| json!(v)));
                insert(&mut layout, "line-sort-key", sort_key);
                let mut layer = json!({"id": id, "type": "line", "source": "s", "paint": paint, "layout": layout});
                if let Some(filter) = filter {
                    layer["filter"] = filter;
                }
                layer
            },
        )
}

/// A second layer of the same type with the first one's layout (a layout
/// group shares one bucket), different paint.
fn with_second_layer(first: Json, second: Json) -> Vec<Json> {
    let mut second = second;
    second["layout"] = first["layout"].clone();
    if let Some(filter) = first.get("filter") {
        second["filter"] = filter.clone();
    } else if let Some(o) = second.as_object_mut() {
        o.remove("filter");
    }
    vec![first, second]
}

// --- tests ---

#[test]
fn circle_buckets() {
    run(
        400,
        (
            circle_layer("a"),
            proptest::option::of(circle_layer("b")),
            proptest::collection::vec(point_feature(), 0..8),
            tile(),
            any::<bool>(),
        )
            .prop_map(|(a, b, features, tile, continuous)| {
                let layers = match b {
                    Some(b) => with_second_layer(a, b),
                    None => vec![a],
                };
                bucket_request(layers, features, tile, continuous)
            }),
    );
}

#[test]
fn fill_buckets() {
    run(
        300,
        (
            fill_layer("a"),
            proptest::option::of(fill_layer("b")),
            proptest::collection::vec(polygon_feature(), 0..5),
            tile(),
        )
            .prop_map(|(a, b, features, tile)| {
                let layers = match b {
                    Some(b) => with_second_layer(a, b),
                    None => vec![a],
                };
                bucket_request(layers, features, tile, false)
            }),
    );
}

#[test]
fn line_buckets() {
    run(
        400,
        (
            line_layer("a"),
            proptest::option::of(line_layer("b")),
            proptest::collection::vec(line_feature(), 0..5),
            tile(),
        )
            .prop_map(|(a, b, features, tile)| {
                let layers = match b {
                    Some(b) => with_second_layer(a, b),
                    None => vec![a],
                };
                bucket_request(layers, features, tile, false)
            }),
    );
}

fn polyline_request() -> impl Strategy<Value = Json> {
    (
        proptest::collection::vec(line_coords(), 1..4),
        prop_oneof![Just(2), Just(3)],
        prop_oneof![Just("miter"), Just("bevel"), Just("round")],
        (
            prop_oneof![Just("butt"), Just("round"), Just("square")],
            prop_oneof![Just("butt"), Just("round"), Just("square")],
        ),
        prop_oneof![0.0f64..1.5, 1.0f64..10.0, Just(2.0)],
        prop_oneof![0.0f64..1.5, Just(1.05), 1.5f64..3.0],
        prop_oneof![Just(0u32), Just(1), Just(2), Just(16), Just(17), 1u32..64],
        proptest::option::of((0.0f64..1.0, 0.0f64..1.0, 1.0f64..20000.0)),
    )
        .prop_map(|(lines, type_, join, (begin, end), miter, round, overscaling, clip)| {
            json!({
                "kind": "polyline",
                "type": type_,
                "join": join,
                "miterLimit": miter,
                "beginCap": begin,
                "endCap": end,
                "roundLimit": round,
                "overscaling": overscaling,
                "clip": clip.map(|(a, b, t)| json!([a, b, t])),
                "lines": lines,
            })
        })
}

#[test]
fn polylines() {
    // Edge cases: duplicates only, two points, 180° turns, long lines past
    // MAX_LINE_DISTANCE, a closed ring.
    for (lines, type_) in [
        (json!([[[0, 0], [0, 0], [0, 0]]]), 2),
        (json!([[[0, 0], [10, 0]]]), 2),
        (json!([[[0, 0], [100, 0], [0, 0], [100, 0]]]), 2),
        (json!([[[-16000, 0], [16000, 0], [16000, 16000], [-16000, 16000], [-16000, -16000]]]), 2),
        (json!([[[0, 0], [100, 0], [100, 100], [0, 100], [0, 0]]]), 3),
        (json!([[[0, 0], [100, 1], [200, 0], [300, 1]]]), 2),
    ] {
        for join in ["miter", "bevel", "round"] {
            for cap in ["butt", "round", "square"] {
                let request = json!({
                    "kind": "polyline", "type": type_, "join": join, "miterLimit": 2.0,
                    "beginCap": cap, "endCap": cap, "roundLimit": 1.05, "overscaling": 1,
                    "clip": Json::Null, "lines": lines,
                });
                check(&request).unwrap();
            }
        }
    }
    run(1500, polyline_request());
}

#[test]
fn fill_generator() {
    check(&json!({"kind": "fillgen", "geometries": [[[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]]]})).unwrap();
    check(&json!({"kind": "fillgen", "geometries": [[[[0, 0], [0, 0], [0, 0]]], [[]]]})).unwrap();
    run(
        600,
        proptest::collection::vec(proptest::collection::vec(ring(4000.0), 1..5), 1..4)
            .prop_map(|geometries| json!({"kind": "fillgen", "geometries": geometries})),
    );
}

#[test]
fn dash_patterns() {
    for (from, to) in [
        (json!([1, 2]), json!([1, 2])),
        (json!([0, 1]), json!([0, 1])),
        (json!([1]), json!([1, 1])),
        (json!([1, 0, 1]), json!([0, 0, 1, 1])),
        (json!([2, 0, 0, 2]), json!([5, 1, 2])),
    ] {
        for cap in ["round", "square"] {
            check(&json!({"kind": "dash", "from": from, "to": to, "cap": cap})).unwrap();
        }
    }
    let dasharray = proptest::collection::vec(prop_oneof![Just(0.0f64), 0.0f64..10.0], 2..7);
    run(
        600,
        (dasharray.clone(), dasharray, prop_oneof![Just("round"), Just("square")])
            .prop_map(|(from, to, cap)| json!({"kind": "dash", "from": from, "to": to, "cap": cap})),
    );
}
