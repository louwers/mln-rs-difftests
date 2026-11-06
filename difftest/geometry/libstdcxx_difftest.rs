// Difftest of //vendor/libstdcxx against libstdc++ (libstdcxx_reference.cpp):
// the exact permutations std::sort, std::stable_sort (also with a
// non-transitive "almost equal" comparator), make_heap/sort_heap and
// std::priority_queue produce for keys with many duplicates.

use geometry_difftest_support::run_cases;
use proptest::prelude::*;

unsafe extern "C" {
    fn ref_std_sort(keys: *const i32, perm: *mut u32, n: usize);
    fn ref_std_stable_sort(keys: *const i32, perm: *mut u32, n: usize);
    fn ref_std_stable_sort_almost(keys: *const f64, perm: *mut u32, n: usize, tolerance: f64);
    fn ref_std_make_sort_heap(keys: *const i32, perm: *mut u32, n: usize, heap: *mut u32);
    fn ref_priority_queue(keys: *const i32, pop_after: *const u8, n: usize, popped: *mut u32) -> usize;
}

fn identity(n: usize) -> Vec<u32> {
    (0..n as u32).collect()
}

/// Keys with `0..range` values (many duplicates for small ranges), lengths
/// up to 3000 (introsort's depth limit and heap fallback need long runs).
fn keys() -> impl Strategy<Value = Vec<i32>> {
    (1i32..=64, 0usize..=3000).prop_flat_map(|(range, len)| prop::collection::vec(0..range, len..=len))
}

/// Mostly sorted / reversed keys, which drive introsort into its worst cases.
fn patterned_keys() -> impl Strategy<Value = Vec<i32>> {
    (0usize..=2000, 0u8..4, 1i32..=8).prop_map(|(len, kind, modulus)| {
        (0..len as i32)
            .map(|i| match kind {
                0 => i / modulus,
                1 => (len as i32 - i) / modulus,
                2 => (i % 2) * i,
                _ => i % modulus,
            })
            .collect()
    })
}

#[test]
fn sort_matches_libstdcxx() {
    let mut cases = 0;
    for strategy in [keys().boxed(), patterned_keys().boxed()] {
        cases += run_cases(3000, &strategy, |keys| {
            let mut expected = identity(keys.len());
            unsafe { ref_std_sort(keys.as_ptr(), expected.as_mut_ptr(), keys.len()) };
            let mut actual = identity(keys.len());
            libstdcxx::sort_by(&mut actual, |a, b| keys[*a as usize] < keys[*b as usize]);
            prop_assert_eq!(actual, expected);
            Ok(())
        });
    }
    println!("std::sort: {cases} cases, 0 mismatches");
}

#[test]
fn stable_sort_matches_libstdcxx() {
    let mut cases = 0;
    for strategy in [keys().boxed(), patterned_keys().boxed()] {
        cases += run_cases(3000, &strategy, |keys| {
            let mut expected = identity(keys.len());
            unsafe { ref_std_stable_sort(keys.as_ptr(), expected.as_mut_ptr(), keys.len()) };
            let mut actual = identity(keys.len());
            libstdcxx::stable_sort_by(&mut actual, |a, b| keys[*a as usize] < keys[*b as usize]);
            prop_assert_eq!(actual, expected);
            Ok(())
        });
    }
    println!("std::stable_sort: {cases} cases, 0 mismatches");
}

#[test]
fn stable_sort_almost_equal_matches_libstdcxx() {
    let strategy = (0usize..=1500, 0.0f64..=3.0)
        .prop_flat_map(|(len, tolerance)| (prop::collection::vec(-50.0f64..50.0, len..=len), Just(tolerance)));
    let mut cases = 0;
    cases += run_cases(3000, &strategy, |(keys, tolerance)| {
        let mut expected = identity(keys.len());
        unsafe { ref_std_stable_sort_almost(keys.as_ptr(), expected.as_mut_ptr(), keys.len(), tolerance) };
        let mut actual = identity(keys.len());
        libstdcxx::stable_sort_by(&mut actual, |a, b| {
            let (ka, kb) = (keys[*a as usize], keys[*b as usize]);
            if (ka - kb).abs() <= tolerance {
                return false;
            }
            ka < kb
        });
        prop_assert_eq!(actual, expected);
        Ok(())
    });
    println!("std::stable_sort (almost equal): {cases} cases, 0 mismatches");
}

#[test]
fn heap_matches_libstdcxx() {
    let mut cases = 0;
    cases += run_cases(3000, &keys(), |keys| {
        let n = keys.len();
        let mut expected = identity(n);
        let mut expected_heap = vec![0u32; n];
        unsafe { ref_std_make_sort_heap(keys.as_ptr(), expected.as_mut_ptr(), n, expected_heap.as_mut_ptr()) };
        let mut actual = identity(n);
        let comp = |a: &u32, b: &u32| keys[*a as usize] < keys[*b as usize];
        libstdcxx::make_heap(&mut actual, comp);
        prop_assert_eq!(&actual, &expected_heap);
        libstdcxx::sort_heap(&mut actual, comp);
        prop_assert_eq!(actual, expected);
        Ok(())
    });
    println!("std::make_heap/sort_heap: {cases} cases, 0 mismatches");
}

#[test]
fn priority_queue_matches_libstdcxx() {
    let strategy = keys().prop_flat_map(|keys| {
        let n = keys.len();
        (Just(keys), prop::collection::vec(any::<bool>(), n..=n))
    });
    let mut cases = 0;
    cases += run_cases(3000, &strategy, |(keys, pop_after)| {
        let n = keys.len();
        let flags: Vec<u8> = pop_after.iter().map(|b| *b as u8).collect();
        let mut expected = vec![0u32; n];
        let k = unsafe { ref_priority_queue(keys.as_ptr(), flags.as_ptr(), n, expected.as_mut_ptr()) };
        prop_assert_eq!(k, n);

        let mut queue = libstdcxx::PriorityQueue::new(|a: &u32, b: &u32| keys[*a as usize] < keys[*b as usize]);
        let mut actual = Vec::with_capacity(n);
        for (i, pop) in pop_after.iter().enumerate() {
            queue.push(i as u32);
            if *pop {
                actual.push(*queue.top());
                queue.pop();
            }
        }
        while !queue.is_empty() {
            actual.push(*queue.top());
            queue.pop();
        }
        prop_assert_eq!(actual, expected);
        Ok(())
    });
    println!("std::priority_queue: {cases} cases, 0 mismatches");
}
