// Difftests for mln_util::string (util::toString through rapidjson) and the
// //vendor/rapidjson Writer.

use proptest::prelude::*;
use rapidjson::Writer;
use reference_ffi as cpp;
use reference_ffi::special_f64;

fn config(cases: u32) -> ProptestConfig {
    ProptestConfig { cases, failure_persistence: None, ..ProptestConfig::default() }
}

fn check_double(v: f64) {
    for decimal in [false, true] {
        let r = mln_util::string::to_string_double_decimal(v, decimal);
        let c = cpp::to_string_double(v, decimal);
        assert_eq!(r, c, "toString({v:e} = {:#x}, {decimal})", v.to_bits());
    }
}

#[test]
fn double_edge_cases() {
    let mut n = 0;
    for v in special_f64() {
        check_double(v);
        n += 1;
    }
    // Every power of two and of ten, and their neighbours.
    let powers_of_two = (-1022i64..=1023).map(|e| ((e + 1023) as u64) << 52).chain((0..52).map(|k| 1u64 << k));
    for bits in powers_of_two {
        for v in [f64::from_bits(bits), f64::from_bits(bits - 1), f64::from_bits(bits + 1)] {
            check_double(v);
            check_double(-v);
            n += 2;
        }
    }
    for e in -325..=308 {
        let p: f64 = format!("1e{e}").parse().unwrap();
        for v in [p, f64::from_bits(p.to_bits().wrapping_sub(1)), f64::from_bits(p.to_bits() + 1)] {
            check_double(v);
            n += 1;
        }
    }
    for i in -2000i64..2000 {
        check_double(i as f64);
        check_double(i as f64 / 100.0);
        check_double(i as f64 * 1e15);
        n += 3;
    }
    println!("toString(double) edge cases: {n} (x2 decimal)");
}

proptest! {
    #![proptest_config(config(300_000))]

    #[test]
    fn double_any_bits(bits in any::<u64>()) {
        check_double(f64::from_bits(bits));
    }

    #[test]
    fn double_short(m in -1_000_000i64..1_000_000, e in -30i32..30) {
        check_double(format!("{m}e{e}").parse().unwrap());
    }

    #[test]
    fn integers(i in any::<i64>(), u in any::<u64>()) {
        prop_assert_eq!(mln_util::string::to_string_i64(i), cpp::to_string_i64(i));
        prop_assert_eq!(mln_util::string::to_string_u64(u), cpp::to_string_u64(u));
        prop_assert_eq!(mln_util::string::to_string_i32(i as i32), cpp::to_string_i64(i as i32 as i64));
        prop_assert_eq!(mln_util::string::to_string_u32(u as u32), cpp::to_string_u64(u as u32 as u64));
    }
}

/// One Writer call.
#[derive(Clone, Debug)]
enum Op {
    Null,
    Bool(bool),
    Int(i32),
    Uint(u32),
    Int64(i64),
    Uint64(u64),
    Double(f64),
    String(String),
    Key(String),
    StartObject,
    EndObject,
    StartArray,
    EndArray,
}

fn encode(ops: &[Op]) -> Vec<u8> {
    let mut p = Vec::new();
    for op in ops {
        match op {
            Op::Null => p.push(b'n'),
            Op::Bool(true) => p.push(b't'),
            Op::Bool(false) => p.push(b'f'),
            Op::Int(v) => {
                p.push(b'i');
                p.extend_from_slice(&v.to_le_bytes());
            }
            Op::Uint(v) => {
                p.push(b'u');
                p.extend_from_slice(&v.to_le_bytes());
            }
            Op::Int64(v) => {
                p.push(b'I');
                p.extend_from_slice(&v.to_le_bytes());
            }
            Op::Uint64(v) => {
                p.push(b'U');
                p.extend_from_slice(&v.to_le_bytes());
            }
            Op::Double(v) => {
                p.push(b'd');
                p.extend_from_slice(&v.to_le_bytes());
            }
            Op::String(s) | Op::Key(s) => {
                p.push(if matches!(op, Op::String(_)) { b's' } else { b'k' });
                p.extend_from_slice(&(s.len() as u32).to_le_bytes());
                p.extend_from_slice(s.as_bytes());
            }
            Op::StartObject => p.push(b'{'),
            Op::EndObject => p.push(b'}'),
            Op::StartArray => p.push(b'['),
            Op::EndArray => p.push(b']'),
        }
    }
    p
}

fn run(ops: &[Op]) -> (String, Vec<bool>) {
    let mut w = Writer::new();
    let results = ops
        .iter()
        .map(|op| match op {
            Op::Null => w.null(),
            Op::Bool(b) => w.bool_(*b),
            Op::Int(v) => w.int(*v),
            Op::Uint(v) => w.uint(*v),
            Op::Int64(v) => w.int64(*v),
            Op::Uint64(v) => w.uint64(*v),
            Op::Double(v) => w.double(*v),
            Op::String(s) => w.string(s),
            Op::Key(s) => w.key(s),
            Op::StartObject => w.start_object(),
            Op::EndObject => w.end_object(),
            Op::StartArray => w.start_array(),
            Op::EndArray => w.end_array(),
        })
        .collect();
    (w.into_string(), results)
}

fn check_writer(ops: &[Op]) {
    let r = run(ops);
    let c = cpp::writer(&encode(ops), ops.len());
    assert_eq!(r, c, "Writer {ops:?}");
}

fn scalar() -> impl Strategy<Value = Op> {
    prop_oneof![
        Just(Op::Null),
        any::<bool>().prop_map(Op::Bool),
        any::<i32>().prop_map(Op::Int),
        any::<u32>().prop_map(Op::Uint),
        any::<i64>().prop_map(Op::Int64),
        any::<u64>().prop_map(Op::Uint64),
        prop_oneof![any::<u64>().prop_map(f64::from_bits), prop::sample::select(special_f64())].prop_map(Op::Double),
        any::<String>().prop_map(Op::String),
        "[\\x00-\\x7f]{0,8}".prop_map(Op::String),
    ]
}

/// A well-formed value: a scalar, or an array/object of values.
fn value() -> impl Strategy<Value = Vec<Op>> {
    let leaf = scalar().prop_map(|op| vec![op]);
    leaf.prop_recursive(4, 64, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(|items| {
                let mut ops = vec![Op::StartArray];
                ops.extend(items.into_iter().flatten());
                ops.push(Op::EndArray);
                ops
            }),
            prop::collection::vec(("[\\x00-\\x7f]{0,4}|\\PC{0,3}", inner), 0..6).prop_map(|members| {
                let mut ops = vec![Op::StartObject];
                for (k, v) in members {
                    ops.push(Op::Key(k));
                    ops.extend(v);
                }
                ops.push(Op::EndObject);
                ops
            }),
        ]
    })
}

#[test]
fn writer_escapes_every_ascii_byte() {
    for b in 0u8..=0x7f {
        let s = String::from(b as char);
        check_writer(&[Op::String(s.clone())]);
        check_writer(&[Op::StartObject, Op::Key(s), Op::Double(f64::NAN), Op::EndObject]);
    }
    let all: String = (0u8..=0x7f).map(|b| b as char).collect();
    check_writer(&[Op::String(all)]);
    check_writer(&[Op::String("é€😀\u{7f}\u{80}\u{10ffff}".to_owned())]);
    check_writer(&[
        Op::StartArray,
        Op::Int(1),
        Op::Double(f64::NAN),
        Op::Double(f64::INFINITY),
        Op::Double(-0.0),
        Op::EndArray,
    ]);
}

proptest! {
    #![proptest_config(config(30_000))]

    #[test]
    fn writer_documents(ops in value()) {
        check_writer(&ops);
    }
}
