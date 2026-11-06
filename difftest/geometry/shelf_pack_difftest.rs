// Difftest of //vendor/shelf-pack against mapbox::ShelfPack
// (misc_reference.cpp): random sequences of packOne (fresh and known ids),
// pack (with and without inPlace), ref/unref of earlier results, getBin,
// shrink, clear and resize, with and without autoResize. After every
// operation the result, the sprite size and every bin handed out so far
// (id, position, size, max size, refcount) must be identical, which also
// shows that free bins are reused the same way.

use geometry_difftest_support::run_cases;
use proptest::prelude::*;
use serde_json::{Value as Json, json};
use shelf_pack::{Bin, BinRef, PackOptions, ShelfPack, ShelfPackOptions};

unsafe extern "C" {
    fn ref_geojson_free(p: *mut std::ffi::c_char);
    fn ref_shelf_pack(text: *const u8, len: usize) -> *mut std::ffi::c_char;
}

fn cpp(request: &Json) -> Json {
    let text = request.to_string();
    unsafe {
        let p = ref_shelf_pack(text.as_ptr(), text.len());
        let s = std::ffi::CStr::from_ptr(p).to_str().unwrap().to_owned();
        ref_geojson_free(p);
        serde_json::from_str(&s).unwrap()
    }
}

#[derive(Clone, Debug)]
enum Op {
    PackOne(i32, i32, i32),
    Pack(Vec<(i32, i32, i32)>, bool),
    Ref(u32),
    Unref(u32),
    GetBin(i32),
    Shrink,
    Clear,
    Resize(i32, i32),
}

impl Op {
    fn request(&self) -> Json {
        match self {
            Op::PackOne(id, w, h) => json!(["packOne", id, w, h]),
            Op::Pack(bins, in_place) => {
                json!(["pack", bins.iter().map(|b| json!([b.0, b.1, b.2])).collect::<Vec<_>>(), in_place])
            }
            Op::Ref(k) => json!(["ref", k]),
            Op::Unref(k) => json!(["unref", k]),
            Op::GetBin(id) => json!(["getBin", id]),
            Op::Shrink => json!(["shrink"]),
            Op::Clear => json!(["clear"]),
            Op::Resize(w, h) => json!(["resize", w, h]),
        }
    }
}

fn bin_json(bin: Option<&Bin>) -> Json {
    match bin {
        None => Json::Null,
        Some(b) => json!([b.id, b.x, b.y, b.w, b.h, b.maxw, b.maxh, b.refcount()]),
    }
}

fn rust(w: i32, h: i32, auto_resize: bool, ops: &[Op]) -> Json {
    let mut sprite = ShelfPack::new(w, h, ShelfPackOptions { auto_resize });
    let mut handles: Vec<BinRef> = Vec::new();
    let mut out = Vec::new();
    for op in ops {
        let r = match op {
            Op::PackOne(id, w, h) => {
                let bin = sprite.pack_one(*id, *w, *h);
                let j = bin_json(bin.map(|b| sprite.bin(b)));
                handles.extend(bin);
                j
            }
            Op::Pack(list, in_place) => {
                let mut bins: Vec<Bin> = list.iter().map(|&(id, w, h)| Bin::new(id, w, h)).collect();
                let results = sprite.pack(&mut bins, PackOptions { in_place: *in_place });
                let r: Vec<Json> = results.iter().map(|b| bin_json(Some(sprite.bin(*b)))).collect();
                handles.extend(&results);
                let b: Vec<Json> = bins.iter().map(|b| bin_json(Some(b))).collect();
                json!({"results": r, "bins": b})
            }
            Op::Ref(k) | Op::Unref(k) => {
                if handles.is_empty() {
                    Json::Null
                } else {
                    let b = handles[*k as usize % handles.len()];
                    json!(if matches!(op, Op::Ref(_)) { sprite.ref_(b) } else { sprite.unref(b) })
                }
            }
            Op::GetBin(id) => {
                let bin = sprite.get_bin(*id);
                let j = bin_json(bin.map(|b| sprite.bin(b)));
                handles.extend(bin);
                j
            }
            Op::Shrink => {
                sprite.shrink();
                Json::Null
            }
            Op::Clear => {
                sprite.clear();
                handles.clear();
                Json::Null
            }
            Op::Resize(w, h) => json!(sprite.resize(*w, *h)),
        };
        out.push(json!({
            "r": r,
            "w": sprite.width(),
            "h": sprite.height(),
            "bins": handles.iter().map(|b| bin_json(Some(sprite.bin(*b)))).collect::<Vec<_>>(),
        }));
    }
    Json::Array(out)
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    let dim = prop_oneof![1i32..=12, 1i32..=40, Just(0), Just(-1)];
    let id = prop_oneof![Just(-1i32), 1i32..=12];
    let op = prop_oneof![
        8 => (id.clone(), dim.clone(), dim.clone()).prop_map(|(i, w, h)| Op::PackOne(i, w, h)),
        2 => (prop::collection::vec((id.clone(), dim.clone(), dim.clone()), 0..=6), any::<bool>())
            .prop_map(|(b, p)| Op::Pack(b, p)),
        2 => any::<u32>().prop_map(Op::Ref),
        4 => any::<u32>().prop_map(Op::Unref),
        2 => id.prop_map(Op::GetBin),
        1 => Just(Op::Shrink),
        1 => (1i32..=128, 1i32..=128).prop_map(|(w, h)| Op::Resize(w, h)),
    ];
    prop::collection::vec(op, 1..=60)
}

#[test]
fn shelf_pack_matches_cpp() {
    let strategy = (
        prop::sample::select(vec![0i32, 10, 30, 64, 128]),
        prop::sample::select(vec![0i32, 10, 30, 64, 128]),
        any::<bool>(),
        ops(),
        any::<bool>(),
    );
    let total_ops = std::cell::Cell::new(0);
    let cases = run_cases(5000, &strategy, |(w, h, auto_resize, mut ops, clear)| {
        if clear {
            let at = ops.len() / 2;
            ops.insert(at, Op::Clear);
        }
        if auto_resize {
            // With autoResize, packOne of a zero or negative size can retry
            // forever (std::max(w, w1) * 2 does not grow a zero width): C++
            // recursion without end. The core packs positive sizes only.
            for op in &mut ops {
                if let Op::PackOne(_, w, h) = op {
                    *w = (*w).max(1);
                    *h = (*h).max(1);
                }
            }
        }
        total_ops.set(total_ops.get() + ops.len());
        let request = json!({
            "w": w, "h": h, "auto_resize": auto_resize,
            "ops": ops.iter().map(Op::request).collect::<Vec<_>>(),
        });
        let expected = cpp(&request);
        let actual = rust(w, h, auto_resize, &ops);
        prop_assert_eq!(actual, expected, "{:?}", ops);
        Ok(())
    });
    println!("ShelfPack: {cases} cases ({} operations), 0 mismatches", total_ops.get());
}
