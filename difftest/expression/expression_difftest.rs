// Whole-engine difftest of //crates/mln_expression against the C++
// expression engine: for each expression, the parse result (errors or type,
// isFeatureConstant, isZoomConstant, serialization) and the evaluation output
// at several zooms with a feature, compared bit-exactly in a canonical text
// form (reference.cpp writes the same form).
//
// Inputs: every expression of the shared corpus
// (metrics/integration/expression-tests) with each of its inputs' feature
// properties, plus generated interpolate/step/zoom/color/math expressions.

use std::collections::BTreeMap;
use std::path::PathBuf;

use feature::{Feature, Value as MbglValue, ValueObject};
use geometry::{Geometry, Point};
use mln_conversion::Convertible;
use mln_expression::expression::evaluate_feature;
use mln_expression::type_::{self, Type};
use mln_expression::{ParsingContext, Value, is_feature_constant, is_zoom_constant};
use proptest::prelude::*;

/// The C++ oracle process (oracle_main.cpp), shared by the tests.
struct Oracle {
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    _child: std::process::Child,
}

static ORACLE: std::sync::LazyLock<std::sync::Mutex<Oracle>> = std::sync::LazyLock::new(|| {
    let base = PathBuf::from(std::env::var("TEST_SRCDIR").unwrap());
    let path = base.join("_main/difftest/expression/expression_oracle");
    let mut child = std::process::Command::new(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("start the C++ oracle");
    let stdin = child.stdin.take().unwrap();
    let stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    std::sync::Mutex::new(Oracle { stdin, stdout, _child: child })
});

fn encode(t: &Type, out: &mut Vec<u8>) {
    match t {
        Type::Null => out.push(0),
        Type::Number => out.push(1),
        Type::Boolean => out.push(2),
        Type::String => out.push(3),
        Type::Color => out.push(4),
        Type::Padding => out.push(5),
        Type::VariableAnchorOffsetCollection => out.push(6),
        Type::Object => out.push(7),
        Type::Value => out.push(8),
        Type::Array(a) => {
            out.push(9);
            out.push(a.n.map_or(0xFF, |n| n as u8));
            encode(&a.item_type, out);
        }
        Type::Collator => out.push(10),
        Type::Formatted => out.push(11),
        Type::Error => out.push(12),
        Type::Image => out.push(13),
    }
}

fn reference_report(json: &str, expected: Option<&Type>, props: &str, zooms: &[f32]) -> String {
    use std::io::{BufRead, Write};
    let enc = expected.map(|t| {
        let mut v = Vec::new();
        encode(t, &mut v);
        v
    });
    let zooms: Vec<serde_json::Value> = zooms
        .iter()
        .map(|z| if z.is_nan() { serde_json::Value::Null } else { serde_json::Value::from(*z as f64) })
        .collect();
    let request = serde_json::json!({"e": json, "t": enc, "p": props, "z": zooms});
    let mut oracle = ORACLE.lock().unwrap_or_else(|e| e.into_inner());
    writeln!(oracle.stdin, "{request}").unwrap();
    oracle.stdin.flush().unwrap();
    let mut report = String::new();
    loop {
        // nunicode writes invalid UTF-8 for some 4-byte case mappings; the
        // Rust strings hold U+FFFD there (from_utf8_lossy), so compare the
        // C++ bytes the same way.
        let mut line = Vec::new();
        if oracle.stdout.read_until(b'\n', &mut line).unwrap() == 0 {
            panic!("the C++ oracle exited (assert?) on {json}");
        }
        let line = String::from_utf8_lossy(&line);
        if line == "<<<END>>>\n" {
            return report;
        }
        report.push_str(&line);
    }
}

fn quote(s: &str) -> String {
    let mut w = rapidjson::Writer::new();
    w.string(s);
    w.into_string()
}

fn canon(v: &MbglValue) -> String {
    match v {
        MbglValue::Null => "null".to_owned(),
        MbglValue::Bool(b) => format!("b:{b}"),
        MbglValue::Uint(u) => format!("u:{u}"),
        MbglValue::Int(i) => format!("i:{i}"),
        MbglValue::Double(d) => format!("d:{:016x}", d.to_bits()),
        MbglValue::String(s) => format!("s:{}", quote(s)),
        MbglValue::Array(a) => format!("[{}]", a.iter().map(canon).collect::<Vec<_>>().join(",")),
        MbglValue::Object(o) => {
            let sorted: BTreeMap<_, _> = o.iter().collect();
            format!(
                "{{{}}}",
                sorted.iter().map(|(k, v)| format!("{}:{}", quote(k), canon(v))).collect::<Vec<_>>().join(",")
            )
        }
    }
}

fn f32_hex(f: f32) -> String {
    format!("{:08x}", f.to_bits())
}

fn canon_expr(v: &Value) -> String {
    match v {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => format!("b:{b}"),
        Value::Number(d) => format!("n:{:016x}", d.to_bits()),
        Value::String(s) => format!("s:{}", quote(s)),
        Value::Color(c) => format!("c:{},{},{},{}", f32_hex(c.r), f32_hex(c.g), f32_hex(c.b), f32_hex(c.a)),
        Value::VariableAnchorOffsetCollection(o) => format!("v:{}", quote(&o.to_string())),
        Value::Collator(_) => "collator".to_owned(),
        Value::Formatted(f) => format!("f:{}", canon(&f.to_object())),
        Value::Image(i) => format!("i:{},{}", quote(i.id()), if i.is_available() { 1 } else { 0 }),
        Value::Padding(p) => {
            format!("p:{},{},{},{}", f32_hex(p.top), f32_hex(p.right), f32_hex(p.bottom), f32_hex(p.left))
        }
        Value::Array(a) => format!("[{}]", a.iter().map(canon_expr).collect::<Vec<_>>().join(",")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter().map(|(k, v)| format!("{}:{}", quote(k), canon_expr(v))).collect::<Vec<_>>().join(",")
        ),
    }
}

fn json_to_mbgl(v: &serde_json::Value) -> MbglValue {
    match v {
        serde_json::Value::Null => MbglValue::Null,
        serde_json::Value::Bool(b) => MbglValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                MbglValue::Uint(u)
            } else if let Some(i) = n.as_i64() {
                MbglValue::Int(i)
            } else {
                MbglValue::Double(n.as_f64().unwrap())
            }
        }
        serde_json::Value::String(s) => MbglValue::String(s.clone()),
        serde_json::Value::Array(a) => MbglValue::Array(a.iter().map(json_to_mbgl).collect()),
        serde_json::Value::Object(o) => {
            let mut m = ValueObject::new();
            for (k, v) in o {
                m.entry(k.clone()).or_insert_with(|| json_to_mbgl(v));
            }
            MbglValue::Object(m)
        }
    }
}

fn rust_report(json: &str, expected: Option<&Type>, props: &str, zooms: &[f32]) -> String {
    let Ok(document) = serde_json::from_str::<serde_json::Value>(json) else {
        return "json error".to_owned();
    };
    let mut ctx = match expected {
        Some(t) => ParsingContext::with_expected(t.clone()),
        None => ParsingContext::new(),
    };
    let parsed = ctx.parse_layer_property_expression(&Convertible::from(&document));
    let mut report = String::new();
    let Some(e) = parsed else {
        report += "compiled: error\n";
        for err in ctx.get_errors() {
            report += &format!("error: {}|{}\n", err.key, err.message);
        }
        return report;
    };

    report += "compiled: success\n";
    report += &format!("type: {}\n", type_::to_string(e.get_type()));
    report += &format!("featureConstant: {}\n", if is_feature_constant(&*e) { 1 } else { 0 });
    report += &format!("zoomConstant: {}\n", if is_zoom_constant(&*e) { 1 } else { 0 });
    report += &format!("serialized: {}\n", canon(&e.serialize()));

    let props: serde_json::Value = serde_json::from_str(props).unwrap();
    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    if let MbglValue::Object(p) = json_to_mbgl(&props) {
        feature.properties = p;
    }

    for zoom in zooms {
        let zoom = if zoom.is_nan() { None } else { Some(*zoom) };
        match evaluate_feature(&*e, zoom, &feature, None) {
            Ok(v) => report += &format!("out: {}\n", canon_expr(&v)),
            Err(err) => report += &format!("err: {}\n", err.message),
        }
    }
    report
}

const ZOOMS: [f32; 9] = [f32::NAN, -1.0, 0.0, 0.5, 1.0, 2.75, 10.0, 22.0, 24.5];

fn compare(json: &str, expected: Option<&Type>, props: &str, zooms: &[f32]) -> Result<(), String> {
    let cpp = reference_report(json, expected, props, zooms);
    let rust = rust_report(json, expected, props, zooms);
    if cpp == rust {
        Ok(())
    } else {
        Err(format!(
            "expression {json}\nexpected type {expected:?}\nproperties {props}\n--- C++\n{cpp}--- Rust\n{rust}"
        ))
    }
}

fn corpus_root() -> PathBuf {
    let base = PathBuf::from(std::env::var("TEST_SRCDIR").unwrap());
    for repo in ["maplibre+", "maplibre"] {
        let p = base.join(repo).join("metrics/integration/expression-tests");
        if p.exists() {
            return p;
        }
    }
    panic!("expression-test-files not in the runfiles");
}

fn spec_type(spec: &serde_json::Value) -> Option<Type> {
    let string_to_type = |t: &str| match t {
        "string" | "number-format" => Type::String,
        "number" => Type::Number,
        "boolean" => Type::Boolean,
        "object" => Type::Object,
        "color" => Type::Color,
        "value" => Type::Value,
        "formatted" => Type::Formatted,
        "resolvedImage" => Type::Image,
        _ => Type::Null,
    };
    let t = spec.get("type")?.as_str()?;
    match t {
        "array" => {
            let item = spec.get("value").and_then(|v| v.as_str()).map_or(Type::Value, string_to_type);
            match spec.get("length").and_then(|l| l.as_f64()) {
                Some(n) if n as usize != 0 => Some(Type::array_n(item, n as usize)),
                _ => Some(Type::array(item)),
            }
        }
        "enum" => Some(Type::String),
        t => Some(string_to_type(t)),
    }
}

// Corpus groups whose C++ behaviour depends on code not ported yet
// (within/distance geometry) are compared in the corpus runner only.
const SKIPPED_GROUPS: &[&str] = &["within/", "distance/"];

#[test]
fn corpus_expressions() {
    let root = corpus_root();
    let mut tests = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.file_name().is_some_and(|f| f == "test.json") {
                out.push(p);
            }
        }
    }
    walk(&root, &mut tests);

    let mut compared = 0;
    let mut failures = Vec::new();
    let mut skipped = 0;
    for path in tests {
        let id = path.parent().unwrap().strip_prefix(&root).unwrap().to_string_lossy().into_owned();
        if SKIPPED_GROUPS.iter().any(|g| id.starts_with(g)) {
            skipped += 1;
            continue;
        }
        let test: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let expression = &test["expression"];
        if expression.is_object() {
            // Legacy functions go through style's convertFunctionToExpression.
            skipped += 1;
            continue;
        }
        let json = expression.to_string();
        let expected = test.get("propertySpec").and_then(spec_type);
        let mut props_list: Vec<String> = test
            .get("inputs")
            .and_then(|i| i.as_array())
            .map(|inputs| {
                inputs
                    .iter()
                    .map(|input| input[1].get("properties").map_or("{}".to_owned(), |p| p.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if props_list.is_empty() {
            props_list.push("{}".to_owned());
        }
        for props in &props_list {
            compared += 1;
            if let Err(e) = compare(&json, expected.as_ref(), props, &ZOOMS) {
                failures.push(format!("[{id}]\n{e}"));
            }
            // Also without the property spec's expected type.
            compared += 1;
            if let Err(e) = compare(&json, None, props, &ZOOMS) {
                failures.push(format!("[{id}, untyped]\n{e}"));
            }
        }
    }
    println!(
        "corpus expressions: {compared} parse+evaluate cases compared ({} zooms each), {skipped} tests skipped, {} mismatches",
        ZOOMS.len(),
        failures.len()
    );
    for f in &failures {
        println!("{f}");
    }
    assert!(failures.is_empty(), "{} mismatches", failures.len());
}

// ---------------------------------------------------------------------------
// Generated expressions. The generators avoid inputs that trip live C++
// asserts in the -c fastbuild oracle: NaN never reaches a Color constructor
// (rgba arguments are literals; interpolated colors have alpha 0 or >= 0.1;
// exponential bases are positive), and no collators or slices.

use serde_json::json;

fn num(v: f64) -> serde_json::Value {
    serde_json::Value::Number(serde_json::Number::from_f64(v).unwrap())
}

fn interesting_f64() -> impl Strategy<Value = f64> {
    prop_oneof![
        3 => prop::sample::select(vec![
            0.0, -0.0, 1.0, -1.0, 0.5, 2.0, 3.0, 10.0, 22.0, 0.1, 0.2, 1e-7, 1e21, 1e22, 123456789.0,
            2147483647.0, 2147483648.0, -2147483649.0, 9007199254740992.0, 1.7976931348623157e308,
            5e-324, 2.2250738585072014e-308, 0.30000000000000004, 1.5, 2.5, -2.5, 255.0, 256.0,
        ]),
        3 => -30.0f64..30.0,
        1 => any::<f64>().prop_filter("finite", |v| v.is_finite()),
        1 => (-1e6f64..1e6).prop_map(|v| v.round()),
    ]
}

fn interp_type() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        Just(json!(["linear"])),
        (0.01f64..4.0).prop_map(|b| json!(["exponential", num(b)])),
        prop::sample::select(vec![0.5, 1.0, 1.2, 2.0]).prop_map(|b| json!(["exponential", num(b)])),
        (0.0f64..=1.0, 0.0f64..=1.0, 0.0f64..=1.0, 0.0f64..=1.0).prop_map(|(a, b, c, d)| json!([
            "cubic-bezier",
            num(a),
            num(b),
            num(c),
            num(d)
        ])),
    ]
}

fn stop_keys() -> impl Strategy<Value = Vec<f64>> {
    prop::collection::btree_set(-20i32..40, 1..5).prop_map(|keys| keys.into_iter().map(|k| k as f64 * 0.75).collect())
}

fn num_expr() -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        4 => interesting_f64().prop_map(num),
        1 => Just(json!(["zoom"])),
        1 => Just(json!(["get", "n"])),
        1 => Just(json!(["to-number", ["get", "s"]])),
        1 => Just(json!(["length", ["get", "s"]])),
        1 => Just(json!(["pi"])),
    ];
    leaf.prop_recursive(4, 24, 4, |inner| {
        prop_oneof![
            (prop::sample::select(vec!["+", "-", "*", "/", "%", "^", "min", "max"]), inner.clone(), inner.clone())
                .prop_map(|(op, a, b)| json!([op, a, b])),
            (
                prop::sample::select(vec![
                    "-", "sqrt", "ln", "log10", "log2", "abs", "floor", "ceil", "round", "sin", "cos", "tan", "asin",
                    "acos", "atan"
                ]),
                inner.clone()
            )
                .prop_map(|(op, a)| json!([op, a])),
            (interp_type(), inner.clone(), stop_keys(), prop::collection::vec(inner.clone(), 4)).prop_map(
                |(interp, input, keys, outputs)| {
                    let mut e = vec![json!("interpolate"), interp, input];
                    for (k, o) in keys.iter().zip(outputs.iter().cycle()) {
                        e.push(num(*k));
                        e.push(o.clone());
                    }
                    serde_json::Value::Array(e)
                }
            ),
            (inner.clone(), inner.clone(), stop_keys(), prop::collection::vec(inner.clone(), 4)).prop_map(
                |(input, first, keys, outputs)| {
                    let mut e = vec![json!("step"), input, first];
                    for (k, o) in keys.iter().zip(outputs.iter().cycle()) {
                        e.push(num(*k));
                        e.push(o.clone());
                    }
                    serde_json::Value::Array(e)
                }
            ),
        ]
    })
}

fn color_string() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(vec![
            "red",
            "blue",
            "transparent",
            "#fff",
            "#0f08",
            "#123456",
            "#12345678",
            "rgb(10, 20, 30)",
            "rgba(255, 0, 0, 0.5)",
            "rgba(0,0,0,0)",
            "hsl(120, 50%, 50%)",
            "hsla(240, 100%, 50%, 0.25)",
            "rebeccapurple",
            "not a color",
            "#ggg",
            "rgba(300, -1, 0.5, 2)",
            "rgb(50%, 10%, 0%)",
        ])
        .prop_map(str::to_owned),
        (any::<u8>(), any::<u8>(), any::<u8>(), 1u8..=10)
            .prop_map(|(r, g, b, a)| format!("rgba({r}, {g}, {b}, {})", a as f64 / 10.0)),
    ]
}

fn color_expr() -> impl Strategy<Value = serde_json::Value> {
    let channel = prop_oneof![
        3 => (0u16..=255).prop_map(|v| v as f64),
        1 => prop::sample::select(vec![-1.0, 255.5, 256.0, 0.25, 1e9]),
    ];
    let alpha = prop_oneof![
        3 => (0u8..=10).prop_map(|v| v as f64 / 10.0),
        1 => prop::sample::select(vec![-0.5, 1.5, 0.333]),
    ];
    let simple_input = prop_oneof![Just(json!(["zoom"])), Just(json!(["get", "n"])), interesting_f64().prop_map(num),];
    prop_oneof![
        color_string().prop_map(|s| json!(s)),
        color_string().prop_map(|s| json!(["to-color", s])),
        (channel.clone(), channel.clone(), channel.clone(), alpha).prop_map(|(r, g, b, a)| json!([
            "rgba",
            num(r),
            num(g),
            num(b),
            num(a)
        ])),
        (channel.clone(), channel.clone(), channel).prop_map(|(r, g, b)| json!(["rgb", num(r), num(g), num(b)])),
        (
            prop::sample::select(vec!["interpolate", "interpolate-hcl", "interpolate-lab"]),
            interp_type(),
            simple_input,
            stop_keys(),
            prop::collection::vec(color_string(), 4)
        )
            .prop_map(|(op, interp, input, keys, colors)| {
                let mut e = vec![json!(op), interp, input];
                for (k, c) in keys.iter().zip(colors.iter().cycle()) {
                    e.push(num(*k));
                    e.push(json!(["to-color", c, "black"]));
                }
                serde_json::Value::Array(e)
            }),
    ]
}

fn string_expr() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        num_expr().prop_map(|n| json!(["to-string", n])),
        color_expr().prop_map(|c| json!(["to-string", ["to-color", c]])),
        (num_expr(), num_expr()).prop_map(|(a, b)| json!(["concat", a, " ", b, ["get", "s"]])),
        num_expr().prop_map(|n| json!(["typeof", n])),
        (interesting_f64(), interesting_f64())
            .prop_map(|(a, b)| json!(["to-string", ["literal", [num(a), num(b), {"k": num(a)}]]])),
    ]
}

fn bool_expr() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        (prop::sample::select(vec!["<", ">", "<=", ">=", "==", "!="]), num_expr(), num_expr())
            .prop_map(|(op, a, b)| json!([op, a, b])),
        num_expr().prop_map(|n| json!(["to-boolean", n])),
        (num_expr(), num_expr()).prop_map(|(a, b)| json!(["!", ["==", a, b]])),
    ]
}

fn props() -> impl Strategy<Value = String> {
    (
        interesting_f64(),
        prop_oneof![
            interesting_f64().prop_map(|v| format!("{v}")),
            prop::sample::select(vec!["", "1e40", "0x1p3", " 12abc", "nan", "-inf", "1e-50", "ä", "12.5e"])
                .prop_map(str::to_owned),
        ],
    )
        .prop_map(|(n, s)| json!({"n": num(n), "s": s}).to_string())
}

fn check(json: &serde_json::Value, expected: Option<Type>, props: &str) -> Result<(), TestCaseError> {
    compare(&json.to_string(), expected.as_ref(), props, &ZOOMS).map_err(TestCaseError::fail)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 3000, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn generated_number_expressions(e in num_expr(), p in props(), typed in any::<bool>()) {
        check(&e, typed.then_some(Type::Number), &p)?;
    }

    #[test]
    fn generated_color_expressions(e in color_expr(), p in props(), typed in any::<bool>()) {
        check(&e, typed.then_some(Type::Color), &p)?;
    }

    #[test]
    fn generated_string_expressions(e in string_expr(), p in props(), typed in any::<bool>()) {
        check(&e, typed.then_some(Type::String), &p)?;
    }

    #[test]
    fn generated_boolean_expressions(e in bool_expr(), p in props()) {
        check(&e, Some(Type::Boolean), &p)?;
    }
}

/// How many generated cases compile and evaluate, so a vacuous generator
/// shows up in the test log.
#[test]
fn generated_case_statistics() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    let (mut compiled, mut outputs, mut errors, mut total) = (0, 0, 0, 0);
    let strategies: Vec<(BoxedStrategy<serde_json::Value>, Option<Type>)> = vec![
        (num_expr().boxed(), Some(Type::Number)),
        (color_expr().boxed(), Some(Type::Color)),
        (string_expr().boxed(), Some(Type::String)),
        (bool_expr().boxed(), Some(Type::Boolean)),
    ];
    for (strategy, t) in &strategies {
        for _ in 0..500 {
            let e = strategy.new_tree(&mut runner).unwrap().current();
            let report = reference_report(&e.to_string(), t.as_ref(), r#"{"n": 1.5, "s": "2"}"#, &ZOOMS);
            total += 1;
            if report.starts_with("compiled: success") {
                compiled += 1;
            }
            outputs += report.lines().filter(|l| l.starts_with("out: ")).count();
            errors += report.lines().filter(|l| l.starts_with("err: ")).count();
            if total % 400 == 1 {
                println!("sample: {e}\n{report}");
            }
        }
    }
    println!("generated: {total} expressions, {compiled} compiled, {outputs} outputs, {errors} evaluation errors");
    assert!(compiled * 2 > total);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 3000, failure_persistence: None, ..ProptestConfig::default() })]

    /// number-format against ICU (platform/default number_format.cpp) for
    /// the root and en-US locales.
    #[test]
    fn generated_number_format(
        n in interesting_f64(),
        min in 0u8..6,
        extra in 0u8..6,
        locale in prop::sample::select(vec!["", "en-US", "en"]),
    ) {
        let e = json!(["number-format", num(n), {
            "locale": locale,
            "min-fraction-digits": min,
            "max-fraction-digits": min + extra,
        }]);
        check(&e, Some(Type::String), "{}")?;
    }

    #[test]
    fn generated_number_format_currency(
        n in interesting_f64(),
        currency in prop::sample::select(vec!["USD", "EUR", "JPY", "GBP"]),
    ) {
        let e = json!(["number-format", num(n), {"locale": "en-US", "currency": currency}]);
        check(&e, Some(Type::String), "{}")?;
    }
}

fn unicode_string() -> impl Strategy<Value = String> {
    prop_oneof![
        "\\PC{0,8}",
        "[a-zA-ZäöüÄÖÜßéèêÉçÇñÑøØåÅæÆœŒıİσςΣ ]{0,8}",
        prop::sample::select(vec!["ﬃ", "ß", "ǅ", "İstanbul", "ΌΣΟΣ", "ᾳ", "Ꭰ", "𐐀", "ŉ"]).prop_map(str::to_owned),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 3000, failure_persistence: None, ..ProptestConfig::default() })]

    /// upcase/downcase (nunicode case mapping) and the collator comparisons
    /// (nunicode DUCET collation, unaccent) against the C++ platform.
    #[test]
    fn generated_case_mapping_and_collation(
        a in unicode_string(),
        b in unicode_string(),
        op in prop::sample::select(vec!["==", "!=", "<", ">", "<=", ">="]),
        case_sensitive in any::<bool>(),
        diacritic_sensitive in any::<bool>(),
    ) {
        check(&json!(["upcase", a.clone()]), Some(Type::String), "{}")?;
        check(&json!(["downcase", b.clone()]), Some(Type::String), "{}")?;
        let collator = json!(["collator", {"case-sensitive": case_sensitive, "diacritic-sensitive": diacritic_sensitive}]);
        let props = json!({"a": a, "b": b}).to_string();
        check(&json!([op, ["string", ["get", "a"]], ["get", "b"], collator]), Some(Type::Boolean), &props)?;
    }
}
