// Exhaustive difftest of type::checkSubtype and type::toString
// (src/mln/style/expression/check_subtype.cpp, include/mln/style/expression/type.hpp)
// for every type up to array nesting depth 2 with N in {none, 0, 1, 2}.

use mln_expression::check_subtype::check_subtype;
use mln_expression::type_::{self, Array, Type};

unsafe extern "C" {
    fn ref_type_to_string(enc: *const u8, out: *mut u8, cap: usize) -> usize;
    fn ref_check_subtype(expected: *const u8, t: *const u8, out: *mut u8, cap: usize) -> std::ffi::c_long;
}

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

fn reference_to_string(t: &Type) -> String {
    let mut enc = Vec::new();
    encode(t, &mut enc);
    let mut buf = vec![0u8; 256];
    let len = unsafe { ref_type_to_string(enc.as_ptr(), buf.as_mut_ptr(), buf.len()) };
    assert!(len <= buf.len());
    String::from_utf8(buf[..len].to_vec()).unwrap()
}

fn reference_check_subtype(expected: &Type, t: &Type) -> Option<String> {
    let (mut e, mut tt) = (Vec::new(), Vec::new());
    encode(expected, &mut e);
    encode(t, &mut tt);
    let mut buf = vec![0u8; 512];
    let len = unsafe { ref_check_subtype(e.as_ptr(), tt.as_ptr(), buf.as_mut_ptr(), buf.len()) };
    if len < 0 {
        return None;
    }
    let len = len as usize;
    assert!(len <= buf.len());
    Some(String::from_utf8(buf[..len].to_vec()).unwrap())
}

fn all_types() -> Vec<Type> {
    let base = vec![
        Type::Null,
        Type::Number,
        Type::Boolean,
        Type::String,
        Type::Color,
        Type::Padding,
        Type::VariableAnchorOffsetCollection,
        Type::Object,
        Type::Value,
        Type::Collator,
        Type::Formatted,
        Type::Error,
        Type::Image,
    ];
    let ns = [None, Some(0), Some(1), Some(2)];
    let wrap = |items: &[Type]| -> Vec<Type> {
        let mut out = Vec::new();
        for item in items {
            for n in ns {
                out.push(Array::with_n(item.clone(), n).into());
            }
        }
        out
    };
    let depth1 = wrap(&base);
    let depth2 = wrap(&depth1);
    let mut all = base;
    all.extend(depth1);
    all.extend(depth2);
    all
}

#[test]
fn to_string_matches() {
    let types = all_types();
    assert_eq!(types.len(), 13 + 13 * 4 + 13 * 4 * 4);
    for t in &types {
        assert_eq!(type_::to_string(t), reference_to_string(t), "{t:?}");
    }
    println!("type::toString: {} types compared", types.len());
}

#[test]
fn check_subtype_matches() {
    let types = all_types();
    let mut count = 0;
    let mut subtypes = 0;
    for expected in &types {
        for t in &types {
            let rust = check_subtype(expected, t);
            let cpp = reference_check_subtype(expected, t);
            assert_eq!(rust, cpp, "checkSubtype({expected:?}, {t:?})");
            count += 1;
            subtypes += usize::from(rust.is_none());
        }
    }
    println!("type::checkSubtype: {count} pairs compared ({subtypes} subtype pairs)");
}
