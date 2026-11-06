// Rust //vendor/pixelmatch vs. the C++ mapbox::pixelmatch: mismatch count
// and diff image must be identical.

use proptest::prelude::*;

unsafe extern "C" {
    fn ref_pixelmatch(
        img1: *const u8,
        img2: *const u8,
        width: usize,
        height: usize,
        output: *mut u8,
        threshold: f64,
        include_aa: bool,
    ) -> u64;
}

fn cpp(img1: &[u8], img2: &[u8], w: usize, h: usize, threshold: f64, include_aa: bool) -> (u64, Vec<u8>) {
    assert!(img1.len() >= w * h * 4 && img2.len() >= w * h * 4);
    let mut out = vec![0u8; w * h * 4];
    // SAFETY: both inputs and the output hold w * h * 4 bytes.
    let n = unsafe { ref_pixelmatch(img1.as_ptr(), img2.as_ptr(), w, h, out.as_mut_ptr(), threshold, include_aa) };
    (n, out)
}

fn rust(img1: &[u8], img2: &[u8], w: usize, h: usize, threshold: f64, include_aa: bool) -> (u64, Vec<u8>) {
    let mut out = vec![0u8; w * h * 4];
    let n = pixelmatch::pixelmatch(img1, img2, w, h, Some(&mut out), threshold, include_aa);
    (n, out)
}

fn same(img1: &[u8], img2: &[u8], w: usize, h: usize, threshold: f64, include_aa: bool) {
    let r = rust(img1, img2, w, h, threshold, include_aa);
    let c = cpp(img1, img2, w, h, threshold, include_aa);
    assert_eq!(r.0, c.0, "mismatch count, {w}x{h}, threshold {threshold}, includeAA {include_aa}");
    assert!(r.1 == c.1, "diff image, {w}x{h}, threshold {threshold}, includeAA {include_aa}");
    // Without an output image the count is the same.
    let n = pixelmatch::pixelmatch(img1, img2, w, h, None, threshold, include_aa);
    assert_eq!(n, c.0);
}

/// An image pair: random first image, second image with a fraction of
/// pixels perturbed (small changes exercise the anti-aliasing detector, large
/// ones the threshold).
fn image_pair() -> impl Strategy<Value = (usize, usize, Vec<u8>, Vec<u8>)> {
    (1usize..=12, 1usize..=12, 0u8..=3).prop_flat_map(|(w, h, palette_bits)| {
        let n = w * h * 4;
        // A small palette produces runs of equal pixels, as rendered images do.
        let channel = if palette_bits == 0 {
            any::<u8>().boxed()
        } else {
            (0u8..(1 << palette_bits))
                .prop_map(move |v| v.wrapping_mul(255 / ((1u8 << palette_bits) - 1).max(1)))
                .boxed()
        };
        (
            Just(w),
            Just(h),
            proptest::collection::vec(channel, n),
            proptest::collection::vec((any::<bool>(), -40i16..=40), n),
        )
            .prop_map(|(w, h, img1, changes)| {
                let img2 = img1
                    .iter()
                    .zip(changes)
                    .map(|(&v, (change, delta))| if change { (i16::from(v) + delta).clamp(0, 255) as u8 } else { v })
                    .collect();
                (w, h, img1, img2)
            })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn matches_cpp((w, h, img1, img2) in image_pair(), threshold in prop_oneof![Just(0.1285), Just(0.1), Just(0.05), Just(0.0), 0.0f64..1.0], include_aa: bool) {
        same(&img1, &img2, w, h, threshold, include_aa);
        same(&img2, &img1, w, h, threshold, include_aa);
    }
}

#[test]
fn edge_cases() {
    // 1x1, single row, single column, all-transparent, all-opaque extremes.
    let shapes = [(1usize, 1usize), (7, 1), (1, 7), (3, 3), (16, 9)];
    let fills: [[u8; 4]; 5] = [[0, 0, 0, 0], [255, 255, 255, 255], [0, 0, 0, 255], [255, 0, 0, 128], [1, 2, 3, 0]];
    for (w, h) in shapes {
        for a in fills {
            for b in fills {
                let img1: Vec<u8> = (0..w * h).flat_map(|_| a).collect();
                let mut img2: Vec<u8> = (0..w * h).flat_map(|_| b).collect();
                for threshold in [0.0, 0.1285, 1.0] {
                    for include_aa in [false, true] {
                        same(&img1, &img2, w, h, threshold, include_aa);
                    }
                }
                // A checkerboard of a and b: every pixel has differing neighbours.
                for (i, px) in img2.chunks_exact_mut(4).enumerate() {
                    px.copy_from_slice(if (i % w + i / w) % 2 == 0 { &a } else { &b });
                }
                same(&img1, &img2, w, h, 0.1285, false);
            }
        }
    }
    // Every alpha/value combination through blend(): one 256x256 image of
    // (v, v, v, a) against its inverse.
    let img1: Vec<u8> = (0..256 * 256)
        .flat_map(|i| {
            let (a, v) = ((i / 256) as u8, (i % 256) as u8);
            [v, v, v, a]
        })
        .collect();
    let img2: Vec<u8> = img1.iter().map(|v| 255 - v).collect();
    same(&img1, &img2, 256, 256, 0.1285, false);
    same(&img1, &img2, 256, 256, 0.0, true);
}

/// An anti-aliased edge (a smooth ramp shifted by one pixel) exercises both
/// the anti-aliasing detector (yellow) and real differences (red).
#[test]
fn antialiasing_paths_are_exercised() {
    let (w, h) = (32usize, 8usize);
    let ramp = |shift: usize| -> Vec<u8> {
        (0..w * h)
            .flat_map(|i| {
                let x = i % w;
                let v = if x < 12 + shift {
                    0u8
                } else if x > 15 + shift {
                    255
                } else {
                    ((x - 12 - shift) * 64) as u8
                };
                [v, v, v, 255]
            })
            .collect()
    };
    let mut img1 = ramp(0);
    let img2 = ramp(1);
    // A hard, non-anti-aliased difference too.
    img1[(4 * w + 2) * 4..(4 * w + 2) * 4 + 3].copy_from_slice(&[255, 255, 255]);
    same(&img1, &img2, w, h, 0.1285, false);
    let (count, out) = cpp(&img1, &img2, w, h, 0.1285, false);
    let yellow = out.chunks(4).filter(|p| *p == [255, 255, 0, 255]).count();
    let red = out.chunks(4).filter(|p| *p == [255, 0, 0, 255]).count();
    assert!(yellow > 0, "no anti-aliased pixels detected");
    assert!(red > 0 && count == red as u64);
}
