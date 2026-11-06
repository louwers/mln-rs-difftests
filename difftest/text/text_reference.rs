//! Safe wrappers around the C++ oracle (reference.cpp) and the shared test
//! data of //difftest/text: the font set and the shaping corpus.

use std::path::{Path, PathBuf};

mod ffi {
    unsafe extern "C" {
        pub fn ref_hbshaper_new(data: *const u8, len: usize, type_: u16) -> *mut core::ffi::c_void;
        pub fn ref_hbshaper_valid(shaper: *mut core::ffi::c_void) -> i32;
        pub fn ref_hbshaper_shape(
            shaper: *mut core::ffi::c_void,
            text: *const u16,
            len: usize,
            ids: *mut u32,
            adjusts: *mut f32,
            cap: usize,
        ) -> usize;
        pub fn ref_hbshaper_free(shaper: *mut core::ffi::c_void);
        pub fn ref_hbshaper_rasterize(
            shaper: *mut core::ffi::c_void,
            gid: u16,
            type_: u16,
            out: *mut u8,
            cap: usize,
        ) -> i64;
        pub fn ref_transform_raster_to_sdf(
            width: u32,
            height: u32,
            data: *const u8,
            radius: f64,
            cutoff: f64,
            out: *mut u8,
        );

        pub fn ref_ftface_new(data: *const u8, len: usize) -> *mut core::ffi::c_void;
        pub fn ref_ftface_free(face: *mut core::ffi::c_void);
        pub fn ref_ftface_info(face: *mut core::ffi::c_void, out: *mut i64);
        pub fn ref_ftface_charmap(face: *mut core::ffi::c_void, i: i32, out: *mut i64);
        pub fn ref_ftface_char_index(face: *mut core::ffi::c_void, c: u32) -> u32;
        pub fn ref_ftface_char_variant_index(face: *mut core::ffi::c_void, c: u32, vs: u32) -> u32;
        pub fn ref_ftface_advance(face: *mut core::ffi::c_void, gid: u32, error: *mut i32) -> i64;
        pub fn ref_hbft_h_advance(face: *mut core::ffi::c_void, gid: u32) -> i32;
        pub fn ref_hbft_nominal_glyph(face: *mut core::ffi::c_void, u: u32) -> u32;
        pub fn ref_hbft_shape_raw(
            face: *mut core::ffi::c_void,
            text: *const u16,
            len: usize,
            script: u32,
            out: *mut i32,
            cap: usize,
        ) -> usize;

        pub fn ref_parse_glyph_pbf(
            first: u32,
            second: u32,
            data: *const u8,
            len: usize,
            out: *mut u8,
            cap: usize,
        ) -> i64;

        pub fn ref_db_rows(
            path: *const core::ffi::c_char,
            pattern: *const core::ffi::c_char,
            ctx: *mut core::ffi::c_void,
            row: unsafe extern "C" fn(*mut core::ffi::c_void, *const core::ffi::c_char, *const u8, usize, i32),
        ) -> i32;

        pub fn ref_font_stack_hash(names: *const u8, len: usize) -> u64;
        pub fn ref_language_tag(tag: *const u8, len: usize, out: *mut u8, cap: usize) -> usize;

        pub fn ref_hb_general_category(u: u32) -> u32;
        pub fn ref_hb_script(u: u32) -> u32;
        pub fn ref_hb_combining_class(u: u32) -> u32;
    }
}

/// `mln::HBShaper` in C++.
pub struct RefShaper(*mut core::ffi::c_void);

impl RefShaper {
    pub fn new(data: &[u8], type_: u16) -> Self {
        RefShaper(unsafe { ffi::ref_hbshaper_new(data.as_ptr(), data.len(), type_) })
    }

    pub fn valid(&self) -> bool {
        unsafe { ffi::ref_hbshaper_valid(self.0) != 0 }
    }

    /// `createComplexGlyphIDs`: (GlyphID hash, [x_offset, y_offset, advance]).
    pub fn shape(&self, text: &[u16]) -> Vec<(u32, [f32; 3])> {
        let mut cap = text.len() * 4 + 16;
        loop {
            let mut ids = vec![0u32; cap];
            let mut adjusts = vec![0f32; cap * 3];
            let n = unsafe {
                ffi::ref_hbshaper_shape(self.0, text.as_ptr(), text.len(), ids.as_mut_ptr(), adjusts.as_mut_ptr(), cap)
            };
            if n <= cap {
                return (0..n).map(|i| (ids[i], [adjusts[3 * i], adjusts[3 * i + 1], adjusts[3 * i + 2]])).collect();
            }
            cap = n;
        }
    }
}

impl RefShaper {
    /// `rasterizeGlyph(GlyphID(gid, type))`.
    pub fn rasterize(&self, gid: u16, type_: u16) -> RefGlyph {
        let mut cap = 4096;
        loop {
            let mut out = vec![0u8; cap];
            let n = unsafe { ffi::ref_hbshaper_rasterize(self.0, gid, type_, out.as_mut_ptr(), cap) } as usize;
            if n <= cap {
                out.truncate(n);
                return RefGlyph::parse(&out).remove(0);
            }
            cap = n;
        }
    }
}

/// A glyph in reference.cpp's serialization.
#[derive(Clone, Debug, PartialEq)]
pub struct RefGlyph {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub left: f32,
    pub top: f32,
    pub advance: u32,
    pub bitmap_width: u32,
    pub bitmap_height: u32,
    pub bitmap: Vec<u8>,
}

impl RefGlyph {
    pub fn parse(mut bytes: &[u8]) -> Vec<RefGlyph> {
        let mut out = Vec::new();
        let next = |b: &mut &[u8]| {
            let v = u32::from_le_bytes(b[..4].try_into().unwrap());
            *b = &b[4..];
            v
        };
        while !bytes.is_empty() {
            let id = next(&mut bytes);
            let width = next(&mut bytes);
            let height = next(&mut bytes);
            let left = f32::from_bits(next(&mut bytes));
            let top = f32::from_bits(next(&mut bytes));
            let advance = next(&mut bytes);
            let bitmap_width = next(&mut bytes);
            let bitmap_height = next(&mut bytes);
            let n = (bitmap_width * bitmap_height) as usize;
            let bitmap = bytes[..n].to_vec();
            bytes = &bytes[n..];
            out.push(RefGlyph { id, width, height, left, top, advance, bitmap_width, bitmap_height, bitmap });
        }
        out
    }
}

/// `util::transformRasterToSDF` in C++.
pub fn ref_transform_raster_to_sdf(width: u32, height: u32, data: &[u8], radius: f64, cutoff: f64) -> Vec<u8> {
    let mut out = vec![0u8; (width * height) as usize];
    unsafe { ffi::ref_transform_raster_to_sdf(width, height, data.as_ptr(), radius, cutoff, out.as_mut_ptr()) };
    out
}

impl Drop for RefShaper {
    fn drop(&mut self) {
        unsafe { ffi::ref_hbshaper_free(self.0) }
    }
}

/// The FreeType face of `mln::FreeTypeFace` (plus its hb-ft font) in C++.
pub struct RefFace(*mut core::ffi::c_void);

/// What `ref_ftface_info` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefFaceInfo {
    pub x_scale: i64,
    pub y_scale: i64,
    pub num_glyphs: i64,
    pub units_per_em: i64,
    pub charmap: i64,
    pub num_charmaps: i64,
    pub face_flags: i64,
    pub hb_x_scale: i64,
    pub hb_y_scale: i64,
}

/// FT_FACE_FLAG_SCALABLE / FT_FACE_FLAG_FIXED_SIZES.
pub const FT_FACE_FLAG_SCALABLE: i64 = 1 << 0;
pub const FT_FACE_FLAG_FIXED_SIZES: i64 = 1 << 1;

impl RefFace {
    pub fn new(data: &[u8]) -> Option<Self> {
        let p = unsafe { ffi::ref_ftface_new(data.as_ptr(), data.len()) };
        if p.is_null() { None } else { Some(RefFace(p)) }
    }

    pub fn info(&self) -> RefFaceInfo {
        let mut o = [0i64; 9];
        unsafe { ffi::ref_ftface_info(self.0, o.as_mut_ptr()) };
        RefFaceInfo {
            x_scale: o[0],
            y_scale: o[1],
            num_glyphs: o[2],
            units_per_em: o[3],
            charmap: o[4],
            num_charmaps: o[5],
            face_flags: o[6],
            hb_x_scale: o[7],
            hb_y_scale: o[8],
        }
    }

    /// (platform_id, encoding_id, FT_Encoding, format) of charmap `i`.
    pub fn charmap(&self, i: usize) -> [i64; 4] {
        let mut o = [0i64; 4];
        unsafe { ffi::ref_ftface_charmap(self.0, i as i32, o.as_mut_ptr()) };
        o
    }

    pub fn char_index(&self, c: u32) -> u32 {
        unsafe { ffi::ref_ftface_char_index(self.0, c) }
    }

    pub fn char_variant_index(&self, c: u32, vs: u32) -> u32 {
        unsafe { ffi::ref_ftface_char_variant_index(self.0, c, vs) }
    }

    /// `FT_Get_Advance` with the hb-ft load flags: Ok(16.16 advance) or the
    /// FreeType error code.
    pub fn advance(&self, gid: u32) -> Result<i64, i32> {
        let mut error = 0;
        let v = unsafe { ffi::ref_ftface_advance(self.0, gid, &mut error) };
        if error == 0 { Ok(v) } else { Err(error) }
    }

    pub fn hb_h_advance(&self, gid: u32) -> i32 {
        unsafe { ffi::ref_hbft_h_advance(self.0, gid) }
    }

    pub fn hb_nominal_glyph(&self, u: u32) -> u32 {
        unsafe { ffi::ref_hbft_nominal_glyph(self.0, u) }
    }

    /// One text part shaped as HBShaper::Impl does: [glyph, cluster,
    /// x_advance, y_advance, x_offset, y_offset] in HarfBuzz units.
    pub fn hb_shape_raw(&self, text: &[u16], script: [u8; 4]) -> Vec<[i32; 6]> {
        let mut cap = text.len() * 4 + 16;
        loop {
            let mut out = vec![0i32; cap * 6];
            let n = unsafe {
                ffi::ref_hbft_shape_raw(
                    self.0,
                    text.as_ptr(),
                    text.len(),
                    u32::from_be_bytes(script),
                    out.as_mut_ptr(),
                    cap,
                )
            };
            if n <= cap {
                return (0..n).map(|i| out[6 * i..6 * i + 6].try_into().unwrap()).collect();
            }
            cap = n;
        }
    }
}

impl Drop for RefFace {
    fn drop(&mut self) {
        unsafe { ffi::ref_ftface_free(self.0) }
    }
}

/// `parseGlyphPBF` in C++: the serialized glyphs (see reference.cpp) or the
/// exception's message.
pub fn ref_parse_glyph_pbf(first: u32, second: u32, data: &[u8]) -> Result<Vec<u8>, String> {
    let mut cap = data.len() * 2 + 1024;
    loop {
        let mut out = vec![0u8; cap];
        let n = unsafe { ffi::ref_parse_glyph_pbf(first, second, data.as_ptr(), data.len(), out.as_mut_ptr(), cap) };
        let len = n.unsigned_abs() as usize;
        if len <= cap {
            out.truncate(len);
            return if n < 0 { Err(String::from_utf8_lossy(&out).into_owned()) } else { Ok(out) };
        }
        cap = len;
    }
}

/// `FontStackHasher` in C++ (names must not contain NUL; an empty name
/// is not representable).
pub fn ref_font_stack_hash(names: &[String]) -> u64 {
    let joined = names.join("\0");
    unsafe { ffi::ref_font_stack_hash(joined.as_ptr(), joined.len()) }
}

/// `LanguageTag::fromBCP47` in C++: "language|script|region|toBCP47()".
pub fn ref_language_tag(tag: &str) -> String {
    let mut out = vec![0u8; tag.len() * 2 + 64];
    let n = unsafe { ffi::ref_language_tag(tag.as_ptr(), tag.len(), out.as_mut_ptr(), out.len()) };
    out.truncate(n);
    String::from_utf8_lossy(&out).into_owned()
}

/// `hb_unicode_general_category` (HarfBuzz 8.2.0, HB_TINY).
pub fn hb_general_category(u: u32) -> u32 {
    unsafe { ffi::ref_hb_general_category(u) }
}

/// `hb_unicode_script` as a tag.
pub fn hb_script(u: u32) -> [u8; 4] {
    unsafe { ffi::ref_hb_script(u) }.to_be_bytes()
}

/// `hb_unicode_combining_class`.
pub fn hb_combining_class(u: u32) -> u32 {
    unsafe { ffi::ref_hb_combining_class(u) }
}

/// The runfiles directory of the C++ repository.
pub fn maplibre_root() -> PathBuf {
    let base = std::env::var_os("RUNFILES_DIR")
        .or_else(|| std::env::var_os("TEST_SRCDIR"))
        .expect("RUNFILES_DIR or TEST_SRCDIR must be set");
    for repo in ["maplibre+", "maplibre"] {
        let p = Path::new(&base).join(repo);
        if p.exists() {
            return p;
        }
    }
    panic!("no maplibre repository in the runfiles");
}

/// A font of the difftest font set.
pub struct TestFont {
    pub name: String,
    pub data: Vec<u8>,
}

/// The font set: `@maplibre//vendor:harfbuzz_test_fonts` (HarfBuzz's perf
/// and API test fonts) and the fonts the render tests load through
/// `font-faces` (stored in metrics/cache-style.db).
pub fn test_fonts() -> Vec<TestFont> {
    let root = maplibre_root();
    let mut fonts = Vec::new();
    for dir in ["vendor/harfbuzz/perf/fonts", "vendor/harfbuzz/test/api/fonts"] {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(root.join(dir))
            .unwrap_or_else(|e| panic!("{dir}: {e}"))
            .map(|e| e.unwrap().path())
            .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("ttf" | "otf")))
            .collect();
        paths.sort();
        for p in paths {
            let name = format!("{dir}/{}", p.file_name().unwrap().to_string_lossy());
            fonts.push(TestFont { name, data: std::fs::read(&p).unwrap() });
        }
    }
    fonts.extend(render_test_fonts());
    fonts
}

/// The `resources` rows of a cache database (relative to the C++ repo)
/// whose url is LIKE `pattern`: (url, data), decompressed as
/// `util::decompress` does (zlib, or gzip).
pub fn db_rows(db: &str, pattern: &str) -> Vec<(String, Vec<u8>)> {
    unsafe extern "C" fn row(
        ctx: *mut core::ffi::c_void,
        url: *const core::ffi::c_char,
        data: *const u8,
        len: usize,
        compressed: i32,
    ) {
        let rows = unsafe { &mut *(ctx as *mut Vec<(String, Vec<u8>, bool)>) };
        let url = unsafe { std::ffi::CStr::from_ptr(url) }.to_string_lossy().into_owned();
        let data = if len == 0 { Vec::new() } else { unsafe { std::slice::from_raw_parts(data, len) }.to_vec() };
        rows.push((url, data, compressed != 0));
    }
    let path = std::ffi::CString::new(maplibre_root().join(db).to_str().unwrap()).unwrap();
    let pattern = std::ffi::CString::new(pattern).unwrap();
    let mut rows: Vec<(String, Vec<u8>, bool)> = Vec::new();
    let rc = unsafe {
        ffi::ref_db_rows(path.as_ptr(), pattern.as_ptr(), &mut rows as *mut _ as *mut core::ffi::c_void, row)
    };
    assert_eq!(rc, 0, "{db}: SQLite error {rc}");
    rows.into_iter()
        .map(|(url, data, compressed)| {
            let data = if compressed {
                use std::io::Read;
                let mut out = Vec::new();
                if flate2::read::ZlibDecoder::new(&data[..]).read_to_end(&mut out).is_err() {
                    out.clear();
                    flate2::read::GzDecoder::new(&data[..]).read_to_end(&mut out).unwrap();
                }
                out
            } else {
                data
            };
            (url, data)
        })
        .collect()
}

/// The `font-faces` fonts of the render tests (text-font/{devanagari,
/// khmer,burmese}), from metrics/cache-style.db.
pub fn render_test_fonts() -> Vec<TestFont> {
    let fonts: Vec<TestFont> = db_rows("metrics/cache-style.db", "http://glyphs/%.ttf")
        .into_iter()
        .map(|(url, data)| TestFont { name: format!("metrics/cache-style.db:{url}"), data })
        .collect();
    assert_eq!(fonts.len(), 3, "render-test font-faces fonts");
    fonts
}

/// UTF-16 of a string.
pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
