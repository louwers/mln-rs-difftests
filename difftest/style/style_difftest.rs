// Difftest for //crates/mln_style against the C++ core: style parsing +
// background-layer paint evaluation (every M1 render-test style.json and
// generated legacy-function styles), and Transitioning. Bit-exact.

use std::os::raw::c_char;
use std::path::PathBuf;

use mln_style::layer::static_immutable_cast;
use mln_style::layers::background_layer::Impl as BackgroundLayerImpl;
use mln_style::map::zoom_history::ZoomHistory;
use mln_style::properties::Transitioning;
use mln_style::property_value::PropertyValue;
use mln_style::renderer::property_evaluation_parameters::PropertyEvaluationParameters;
use mln_style::renderer::property_evaluator::PropertyEvaluator;
use mln_style::renderer::transition_parameters::TransitionParameters;
use mln_style::style::Impl;
use mln_style::transition_options::TransitionOptions;
use mln_style_types::VisibilityType;
use mln_util::Color;
use mln_util::chrono::{Duration, TimePoint, milliseconds};
use proptest::prelude::*;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct RefBackground {
    color: [f32; 4],
    opacity: f32,
    visibility: i32,
    pattern_empty: i32,
    layer_count: i32,
    zoom: f64,
}

unsafe extern "C" {
    fn mln_style_ref_evaluate_background(
        json: *const c_char,
        json_len: usize,
        layer_id: *const c_char,
        layer_id_len: usize,
        now_ns: i64,
        out: *mut RefBackground,
    ) -> i32;

    fn mln_style_ref_transition_color(
        from: *const f32,
        to: *const f32,
        duration_ms: i64,
        delay_ms: i64,
        elapsed_ns: i64,
        out: *mut f32,
    );
}

fn cpp_evaluate_background(json: &str, layer_id: &str, now: TimePoint) -> Result<RefBackground, i32> {
    let mut out = RefBackground::default();
    // SAFETY: the pointers are valid for the given lengths and `out` is a
    // valid RefBackground for the call.
    let status = unsafe {
        mln_style_ref_evaluate_background(
            json.as_ptr() as *const c_char,
            json.len(),
            layer_id.as_ptr() as *const c_char,
            layer_id.len(),
            now.time_since_epoch().count(),
            &mut out,
        )
    };
    if status == 0 { Ok(out) } else { Err(status) }
}

fn cpp_transition_color(from: [f32; 4], to: [f32; 4], duration_ms: i64, delay_ms: i64, elapsed_ns: i64) -> [f32; 4] {
    let mut out = [0.0_f32; 4];
    // SAFETY: four-element arrays as the C++ side reads/writes.
    unsafe {
        mln_style_ref_transition_color(from.as_ptr(), to.as_ptr(), duration_ms, delay_ms, elapsed_ns, out.as_mut_ptr());
    }
    out
}

/// The Rust side of `mln_style_ref_evaluate_background`, through `Style::Impl`.
fn rust_evaluate_background(json: &str, layer_id: &str, now: TimePoint) -> Option<RefBackground> {
    let mut style = Impl::new(1.0);
    style.load_json(json);
    if !style.loaded {
        return None;
    }
    let impls = style.get_layer_impls();
    let zoom = style.get_default_camera().zoom.unwrap_or(0.0);
    let impl_ = impls.iter().find(|impl_| impl_.id == layer_id)?;
    assert!(std::ptr::eq(impl_.get_type_info(), BackgroundLayerImpl::static_type_info()));
    let impl_ = static_immutable_cast::<BackgroundLayerImpl>(impl_);

    let unevaluated = impl_.paint.untransitioned();
    let unevaluated =
        impl_.paint.transitioned(&TransitionParameters { now, transition: TransitionOptions::default() }, unevaluated);
    let mut zoom_history = ZoomHistory::default();
    zoom_history.update(zoom as f32, &now);
    let parameters = PropertyEvaluationParameters::with_zoom_history(zoom_history, now, Duration::zero());
    let evaluated = unevaluated.evaluate(&parameters);

    let color = evaluated.background_color;
    Some(RefBackground {
        color: [color.r, color.g, color.b, color.a],
        opacity: evaluated.background_opacity,
        visibility: if impl_.visibility == VisibilityType::None { 1 } else { 0 },
        pattern_empty: (evaluated.background_pattern.from.empty() && evaluated.background_pattern.to.empty()) as i32,
        layer_count: impls.len() as i32,
        zoom,
    })
}

/// Bit-exact; any NaN equals any NaN.
fn same_f32(a: f32, b: f32) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

fn same_background(a: &RefBackground, b: &RefBackground, compare_layer_count: bool) -> bool {
    (0..4).all(|i| same_f32(a.color[i], b.color[i]))
        && same_f32(a.opacity, b.opacity)
        && a.visibility == b.visibility
        && a.pattern_empty == b.pattern_empty
        && (!compare_layer_count || a.layer_count == b.layer_count)
        && a.zoom.to_bits() == b.zoom.to_bits()
}

fn render_tests_dir() -> PathBuf {
    let base =
        std::env::var("RUNFILES_DIR").or_else(|_| std::env::var("TEST_SRCDIR")).expect("RUNFILES_DIR or TEST_SRCDIR");
    PathBuf::from(base).join("maplibre+/metrics/integration/render-tests")
}

const NOW: TimePoint = TimePoint::new(milliseconds(1_000_000));

#[test]
fn m1_render_test_styles() {
    let cases = [
        ("background-color/default", "background"),
        ("background-color/literal", "background"),
        ("background-color/function", "background"),
        ("background-color/colorSpace-hcl", "background"),
        ("background-color/colorSpace-lab", "background"),
        ("background-color/transition", "background"),
        ("background-opacity/color", "background"),
        ("background-opacity/overlay", "background_overlay"),
        ("background-visibility/none", "background"),
        ("background-visibility/visible", "background"),
    ];
    for (test, layer_id) in cases {
        let path = render_tests_dir().join(test).join("style.json");
        let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let cpp = cpp_evaluate_background(&json, layer_id, NOW).unwrap_or_else(|e| panic!("{test}: C++ status {e}"));
        let rust = rust_evaluate_background(&json, layer_id, NOW).unwrap_or_else(|| panic!("{test}: Rust failed"));
        // background-opacity/overlay: C++ also parses the raster layer, the
        // port skips it (no raster layer type yet), so layer counts differ there.
        let compare_layer_count = test != "background-opacity/overlay";
        assert!(same_background(&cpp, &rust, compare_layer_count), "{test}:\n  C++  {cpp:?}\n  Rust {rust:?}");
    }
}

fn config(cases: u32) -> ProptestConfig {
    ProptestConfig { cases, failure_persistence: None, ..ProptestConfig::default() }
}

const COLORS: &[&str] = &[
    "red",
    "blue",
    "white",
    "black",
    "green",
    "transparent",
    "rgb(118,0,118)",
    "rgb(255,155,0)",
    "rgba(255, 0, 0, 0.5)",
    "rgba(10, 200, 30, 0.25)",
    "hsl(120, 50%, 50%)",
    "#123456",
    "#fff",
    "rgba(0,0,0,0)",
];

fn color_strategy() -> impl Strategy<Value = &'static str> {
    prop::sample::select(COLORS)
}

/// A legacy camera function for background-color, as JSON.
fn color_function_strategy() -> impl Strategy<Value = String> {
    (
        prop::collection::vec((0u32..48, color_strategy()), 1..5),
        prop::sample::select(&["", r#""type":"interval","#, r#""type":"exponential","#][..]),
        prop::sample::select(&["", r#""base":1,"#, r#""base":1.5,"#, r#""base":2,"#, r#""base":0.5,"#][..]),
        prop::option::of(color_strategy()),
    )
        .prop_map(|(stops, type_, base, default)| {
            let stops: Vec<String> = stops.iter().map(|(z, c)| format!(r#"[{}, "{}"]"#, *z as f64 / 2.0, c)).collect();
            let default = default.map(|d| format!(r#""default":"{d}","#)).unwrap_or_default();
            format!(r#"{{{type_}{base}{default}"stops":[{}]}}"#, stops.join(","))
        })
}

fn opacity_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        (0u32..=100).prop_map(|o| format!("{}", o as f64 / 100.0)),
        (prop::collection::vec((0u32..48, 0u32..=100), 1..4)).prop_map(|stops| {
            let stops: Vec<String> =
                stops.iter().map(|(z, o)| format!("[{}, {}]", *z as f64 / 2.0, *o as f64 / 100.0)).collect();
            format!(r#"{{"stops":[{}]}}"#, stops.join(","))
        }),
    ]
}

proptest! {
    #![proptest_config(config(512))]

    #[test]
    fn legacy_functions(
        color in prop_oneof![color_strategy().prop_map(|c| format!(r#""{c}""#)), color_function_strategy()],
        opacity in opacity_strategy(),
        zoom in (0u32..2400).prop_map(|z| z as f64 / 100.0),
        visibility in prop::sample::select(&["visible", "none"][..]),
    ) {
        let json = format!(
            r#"{{"version":8,"zoom":{zoom},"sources":{{}},"layers":[{{"id":"bg","type":"background","layout":{{"visibility":"{visibility}"}},"paint":{{"background-color":{color},"background-opacity":{opacity}}}}}]}}"#
        );
        let cpp = cpp_evaluate_background(&json, "bg", NOW);
        let rust = rust_evaluate_background(&json, "bg", NOW);
        match (cpp, rust) {
            (Ok(cpp), Some(rust)) => prop_assert!(same_background(&cpp, &rust, true), "{json}\n  C++  {cpp:?}\n  Rust {rust:?}"),
            (Err(-3), None) => {}
            (cpp, rust) => prop_assert!(false, "{json}: C++ {cpp:?}, Rust {rust:?}"),
        }
    }

    #[test]
    fn transitions(
        from in prop::array::uniform4(0u32..=255),
        to in prop::array::uniform4(0u32..=255),
        duration_ms in prop_oneof![Just(-1i64), 0i64..3000],
        delay_ms in prop_oneof![Just(-1i64), 0i64..1500],
        elapsed_ns in 0i64..5_000_000_000,
    ) {
        let from = from.map(|v| v as f32 / 255.0);
        let to = to.map(|v| v as f32 / 255.0);
        let cpp = cpp_transition_color(from, to, duration_ms, delay_ms, elapsed_ns);

        let mut options = TransitionOptions::default();
        if duration_ms >= 0 {
            options.duration = Some(milliseconds(duration_ms));
        }
        if delay_ms >= 0 {
            options.delay = Some(milliseconds(delay_ms));
        }
        let start = TimePoint::new(Duration::zero());
        let t0 = Transitioning::with_prior(
            PropertyValue::Constant(Color::new_unchecked(from[0], from[1], from[2], from[3])),
            Transitioning::default(),
            &TransitionOptions::default(),
            start,
        );
        let t1 = Transitioning::with_prior(
            PropertyValue::Constant(Color::new_unchecked(to[0], to[1], to[2], to[3])),
            t0,
            &options,
            start,
        );
        let parameters = PropertyEvaluationParameters::new(0.0);
        let evaluator = PropertyEvaluator::new(&parameters, Color::black());
        let result = t1.evaluate(&evaluator, start + Duration::from_nanos(elapsed_ns));
        let rust = [result.r, result.g, result.b, result.a];
        prop_assert!((0..4).all(|i| same_f32(cpp[i], rust[i])), "C++ {cpp:?}, Rust {rust:?}");
    }
}
