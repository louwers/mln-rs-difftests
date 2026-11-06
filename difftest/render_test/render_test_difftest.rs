// //tools/render-test's std::mt19937 / std::seed_seq / libstdc++ std::shuffle
// port (Manifest::doShuffle) vs. the C++ standard library.

use proptest::prelude::*;
use std_random::{Mt19937, SeedSeq, shuffle};

unsafe extern "C" {
    fn ref_mt19937_seed_seq(seed: u32, out: *mut u32, n: usize);
    fn ref_shuffle(seed: u32, out: *mut u32, n: usize);
}

fn cpp_sequence(seed: u32, n: usize) -> Vec<u32> {
    let mut out = vec![0u32; n];
    // SAFETY: `out` holds `n` values.
    unsafe { ref_mt19937_seed_seq(seed, out.as_mut_ptr(), n) };
    out
}

fn cpp_shuffle(seed: u32, n: usize) -> Vec<u32> {
    let mut out = vec![0u32; n];
    // SAFETY: `out` holds `n` values.
    unsafe { ref_shuffle(seed, out.as_mut_ptr(), n) };
    out
}

fn rust_sequence(seed: u32, n: usize) -> Vec<u32> {
    let mut g = Mt19937::from_seed_seq(&SeedSeq::new(&[seed]));
    (0..n).map(|_| g.next_u32()).collect()
}

fn rust_shuffle(seed: u32, n: usize) -> Vec<u32> {
    let mut v: Vec<u32> = (0..n as u32).collect();
    shuffle(&mut v, &mut Mt19937::from_seed_seq(&SeedSeq::new(&[seed])));
    v
}

#[test]
fn edge_cases() {
    for seed in [0, 1, 2, 5489, 0x7fffffff, 0x80000000, u32::MAX] {
        assert_eq!(rust_sequence(seed, 2000), cpp_sequence(seed, 2000), "seed {seed}");
        // The runner's suite sizes and the small/odd/even boundaries.
        for n in [0, 1, 2, 3, 4, 5, 1350, 1476, 65535, 65536, 70000] {
            assert_eq!(rust_shuffle(seed, n), cpp_shuffle(seed, n), "seed {seed}, n {n}");
        }
    }
}

proptest! {
    #[test]
    fn matches_cpp(seed: u32, n in 0usize..3000) {
        prop_assert_eq!(rust_sequence(seed, 700), cpp_sequence(seed, 700));
        prop_assert_eq!(rust_shuffle(seed, n), cpp_shuffle(seed, n));
    }
}
