// extern "C" wrappers around the C++ core's text shaping for //difftest/text:
//
// - mln::HBShaper (src/mln/text/harfbuzz*.cpp: HarfBuzz 8.2.0, HB_TINY, on an
//   hb-ft font), the oracle for crates/mln_text's HBShaper;
// - the FreeType face exactly as mln::FreeTypeFace sets it up
//   (FT_New_Memory_Face, force_ucs2_charmap, FT_Set_Char_Size(24 px)), plus
//   hb-ft's scale, the oracle for crates/mln_text/src/ft;
// - HarfBuzz's default Unicode functions (general category, script).

#include <mln/text/glyph.hpp>
#include <mln/text/glyph_pbf.hpp>
#include <mln/text/harfbuzz.hpp>
#include <mln/text/language_tag.hpp>
#include <mln/util/font_stack.hpp>
#include <mln/util/tiny_sdf.hpp>

#include <ft2build.h>
#include FT_FREETYPE_H
#include FT_ADVANCES_H
#include FT_TRUETYPE_TABLES_H

#include <hb-ft.h>
#include <sqlite3.h>
#include <hb.h>

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <exception>
#include <string>
#include <vector>

using namespace mln;

namespace {
const FreeTypeLibrary& library() {
    static FreeTypeLibrary lib;
    return lib;
}
} // namespace

extern "C" {

// --- HBShaper ---------------------------------------------------------------

void* ref_hbshaper_new(const uint8_t* data, size_t len, uint16_t type) {
    std::string font(reinterpret_cast<const char*>(data), len);
    return new HBShaper(static_cast<GlyphIDType>(type), font, library());
}

int ref_hbshaper_valid(void* shaper) {
    return static_cast<HBShaper*>(shaper)->valid() ? 1 : 0;
}

// Shapes `text` (UTF-16); writes up to `cap` glyphs: `ids[i]` is the
// GlyphID union's `hash` (code | type << 16), `adjusts[3 * i ..]` is
// x_offset, y_offset, advance. Returns the glyph count.
size_t ref_hbshaper_shape(
    void* shaper, const uint16_t* text, size_t len, uint32_t* ids, float* adjusts, size_t cap) {
    std::u16string str(reinterpret_cast<const char16_t*>(text), len);
    std::vector<GlyphID> glyphIDs;
    std::vector<HBShapeAdjust> shapeAdjusts;
    static_cast<HBShaper*>(shaper)->createComplexGlyphIDs(str, glyphIDs, shapeAdjusts);
    const size_t n = glyphIDs.size();
    for (size_t i = 0; i < n && i < cap; ++i) {
        ids[i] = glyphIDs[i].hash;
        adjusts[3 * i + 0] = shapeAdjusts[i].x_offset;
        adjusts[3 * i + 1] = shapeAdjusts[i].y_offset;
        adjusts[3 * i + 2] = shapeAdjusts[i].advance;
    }
    return n;
}

void ref_hbshaper_free(void* shaper) {
    delete static_cast<HBShaper*>(shaper);
}

// --- FreeType face as FreeTypeFace sets it up --------------------------------

struct RefFace {
    FT_Library library = nullptr;
    FT_Face face = nullptr;
    std::vector<uint8_t> memory;
    hb_font_t* font = nullptr;
};

void* ref_ftface_new(const uint8_t* data, size_t len) {
    auto* f = new RefFace;
    FT_Init_FreeType(&f->library);
    f->memory.assign(data, data + len);
    if (FT_New_Memory_Face(f->library, f->memory.data(), (FT_Long)len, 0, &f->face) != 0) {
        FT_Done_FreeType(f->library);
        delete f;
        return nullptr;
    }
    // FreeTypeFace::force_ucs2_charmap
    for (int i = 0; i < f->face->num_charmaps; i++) {
        if (((f->face->charmaps[i]->platform_id == 0) && (f->face->charmaps[i]->encoding_id == 3)) ||
            ((f->face->charmaps[i]->platform_id == 3) && (f->face->charmaps[i]->encoding_id == 1))) {
            FT_Set_Charmap(f->face, f->face->charmaps[i]);
            break;
        }
    }
    FT_Set_Char_Size(f->face, 0, 24 * 64, 72, 72);
    f->font = hb_ft_font_create(f->face, nullptr);
    return f;
}

void ref_ftface_free(void* p) {
    auto* f = static_cast<RefFace*>(p);
    hb_font_destroy(f->font);
    FT_Done_Face(f->face);
    FT_Done_FreeType(f->library);
    delete f;
}

// out: x_scale, y_scale, num_glyphs, units_per_EM, selected charmap index
// (-1: none), num_charmaps, face_flags, hb x_scale, hb y_scale.
void ref_ftface_info(void* p, int64_t* out) {
    auto* f = static_cast<RefFace*>(p);
    FT_Face face = f->face;
    out[0] = face->size->metrics.x_scale;
    out[1] = face->size->metrics.y_scale;
    out[2] = face->num_glyphs;
    out[3] = face->units_per_EM;
    out[4] = -1;
    for (int i = 0; i < face->num_charmaps; i++) {
        if (face->charmaps[i] == face->charmap) out[4] = i;
    }
    out[5] = face->num_charmaps;
    out[6] = face->face_flags;
    int x = 0, y = 0;
    hb_font_get_scale(f->font, &x, &y);
    out[7] = x;
    out[8] = y;
}

// out: platform_id, encoding_id, FT_Encoding, cmap format.
void ref_ftface_charmap(void* p, int i, int64_t* out) {
    auto* f = static_cast<RefFace*>(p);
    FT_CharMap cm = f->face->charmaps[i];
    out[0] = cm->platform_id;
    out[1] = cm->encoding_id;
    out[2] = cm->encoding;
    out[3] = FT_Get_CMap_Format(cm);
}

uint32_t ref_ftface_char_index(void* p, uint32_t c) {
    return FT_Get_Char_Index(static_cast<RefFace*>(p)->face, c);
}

uint32_t ref_ftface_char_variant_index(void* p, uint32_t c, uint32_t vs) {
    return FT_Face_GetCharVariantIndex(static_cast<RefFace*>(p)->face, c, vs);
}

// FT_Get_Advance(face, gid, FT_LOAD_DEFAULT | FT_LOAD_NO_HINTING): the 16.16
// advance; *error is the FreeType error.
int64_t ref_ftface_advance(void* p, uint32_t gid, int* error) {
    FT_Fixed v = 0;
    *error = FT_Get_Advance(static_cast<RefFace*>(p)->face, gid, FT_LOAD_DEFAULT | FT_LOAD_NO_HINTING, &v);
    return v;
}

// hb_font_get_glyph_h_advance through hb-ft (26.6).
int32_t ref_hbft_h_advance(void* p, uint32_t gid) {
    return hb_font_get_glyph_h_advance(static_cast<RefFace*>(p)->font, gid);
}

// hb_font_get_nominal_glyph through hb-ft (0 when not found).
uint32_t ref_hbft_nominal_glyph(void* p, uint32_t u) {
    hb_codepoint_t g = 0;
    if (!hb_font_get_nominal_glyph(static_cast<RefFace*>(p)->font, u, &g)) return 0;
    return g;
}

// Shapes one text part as HBShaper::Impl does (LTR, `script`, the default
// language) on this face's hb-ft font; writes up to `cap` glyphs as 6
// int32 each: glyph, cluster, x_advance, y_advance, x_offset, y_offset.
size_t ref_hbft_shape_raw(void* p, const uint16_t* text, size_t len, uint32_t script, int32_t* out, size_t cap) {
    auto* f = static_cast<RefFace*>(p);
    hb_buffer_t* buffer = hb_buffer_create();
    hb_buffer_set_direction(buffer, HB_DIRECTION_LTR);
    hb_buffer_set_script(buffer, (hb_script_t)script);
    hb_buffer_set_language(buffer, hb_language_get_default());
    hb_buffer_add_utf16(buffer, text, (int)len, 0, (int)len);
    hb_shape(f->font, buffer, nullptr, 0);
    unsigned int count;
    hb_glyph_info_t* info = hb_buffer_get_glyph_infos(buffer, &count);
    hb_glyph_position_t* pos = hb_buffer_get_glyph_positions(buffer, &count);
    for (unsigned i = 0; i < count && i < cap; i++) {
        out[6 * i + 0] = (int32_t)info[i].codepoint;
        out[6 * i + 1] = (int32_t)info[i].cluster;
        out[6 * i + 2] = pos[i].x_advance;
        out[6 * i + 3] = pos[i].y_advance;
        out[6 * i + 4] = pos[i].x_offset;
        out[6 * i + 5] = pos[i].y_offset;
    }
    hb_buffer_destroy(buffer);
    return count;
}

// --- HBShaper::rasterizeGlyph, transformRasterToSDF ---------------------------------

namespace {
void put32(std::vector<uint8_t>& out, uint32_t v);
void put_glyph(std::vector<uint8_t>& buf, const Glyph& g);
int64_t copy_out(const std::vector<uint8_t>& buf, uint8_t* out, size_t cap) {
    std::memcpy(out, buf.data(), std::min(cap, buf.size()));
    return (int64_t)buf.size();
}
} // namespace

// `rasterizeGlyph(GlyphID(gid, type))`, serialized as one glyph of
// ref_parse_glyph_pbf.
int64_t ref_hbshaper_rasterize(void* shaper, uint16_t gid, uint16_t type, uint8_t* out, size_t cap) {
    std::vector<uint8_t> buf;
    put_glyph(buf, static_cast<HBShaper*>(shaper)->rasterizeGlyph(GlyphID(gid, static_cast<GlyphIDType>(type))));
    return copy_out(buf, out, cap);
}

// `util::transformRasterToSDF` of a width x height alpha raster into `out`.
void ref_transform_raster_to_sdf(uint32_t width, uint32_t height, const uint8_t* data, double radius, double cutoff, uint8_t* out) {
    AlphaImage raster({width, height}, data, (size_t)width * height);
    AlphaImage sdf = util::transformRasterToSDF(raster, radius, cutoff);
    if (sdf.data) std::memcpy(out, sdf.data.get(), sdf.bytes());
}

// --- parseGlyphPBF ---------------------------------------------------------------

namespace {
void put32(std::vector<uint8_t>& out, uint32_t v) {
    for (int i = 0; i < 4; i++) out.push_back((uint8_t)(v >> (8 * i)));
}

void put_glyph(std::vector<uint8_t>& buf, const Glyph& g) {
    put32(buf, g.id.hash);
    put32(buf, g.metrics.width);
    put32(buf, g.metrics.height);
    uint32_t bits;
    std::memcpy(&bits, &g.metrics.left, 4);
    put32(buf, bits);
    std::memcpy(&bits, &g.metrics.top, 4);
    put32(buf, bits);
    put32(buf, g.metrics.advance);
    put32(buf, g.bitmap.size.width);
    put32(buf, g.bitmap.size.height);
    if (g.bitmap.data) buf.insert(buf.end(), g.bitmap.data.get(), g.bitmap.data.get() + g.bitmap.bytes());
}
} // namespace

// Parses a glyph PBF; writes up to `cap` bytes: per glyph (little endian
// u32) GlyphID hash, width, height, left and top (f32 bits), advance,
// bitmap width, bitmap height, then the bitmap. Returns the byte count, or
// -(length of the exception message) with the message written.
int64_t ref_parse_glyph_pbf(uint32_t first, uint32_t second, const uint8_t* data, size_t len, uint8_t* out, size_t cap) {
    std::vector<uint8_t> buf;
    try {
        auto glyphs = parseGlyphPBF(GlyphRange{first, second, GlyphIDType::FontPBF},
                                    std::string(reinterpret_cast<const char*>(data), len));
        for (const auto& g : glyphs) put_glyph(buf, g);
    } catch (const std::exception& e) {
        std::string what = e.what();
        std::memcpy(out, what.data(), std::min(cap, what.size()));
        return -(int64_t)what.size();
    }
    std::memcpy(out, buf.data(), std::min(cap, buf.size()));
    return (int64_t)buf.size();
}

// --- Cache database rows ------------------------------------------------------------

// Calls `row(ctx, url, data, len, compressed)` for every row of `resources`
// in the SQLite database at `path` whose url is LIKE `pattern`, in url
// order. (Through the C++ core's SQLite: rusqlite's would clash with it in
// one binary.) Returns 0, or the SQLite error code.
int ref_db_rows(const char* path,
                const char* pattern,
                void* ctx,
                void (*row)(void*, const char*, const uint8_t*, size_t, int)) {
    sqlite3* db = nullptr;
    int rc = sqlite3_open_v2(path, &db, SQLITE_OPEN_READONLY, nullptr);
    if (rc != SQLITE_OK) {
        sqlite3_close(db);
        return rc;
    }
    sqlite3_stmt* stmt = nullptr;
    rc = sqlite3_prepare_v2(
        db, "SELECT url, data, compressed FROM resources WHERE url LIKE ?1 ORDER BY url", -1, &stmt, nullptr);
    if (rc == SQLITE_OK) {
        sqlite3_bind_text(stmt, 1, pattern, -1, SQLITE_TRANSIENT);
        while ((rc = sqlite3_step(stmt)) == SQLITE_ROW) {
            const char* url = reinterpret_cast<const char*>(sqlite3_column_text(stmt, 0));
            const auto* data = static_cast<const uint8_t*>(sqlite3_column_blob(stmt, 1));
            size_t len = sqlite3_column_bytes(stmt, 1);
            row(ctx, url, data, len, sqlite3_column_int(stmt, 2));
        }
        rc = rc == SQLITE_DONE ? 0 : rc;
    }
    sqlite3_finalize(stmt);
    sqlite3_close(db);
    return rc;
}

// --- FontStackHasher, LanguageTag ---------------------------------------------------

// FontStackHasher of `count` NUL-separated names in `names`.
uint64_t ref_font_stack_hash(const char* names, size_t len) {
    FontStack stack;
    std::string all(names, len);
    size_t start = 0;
    while (start < all.size()) {
        size_t end = all.find('\0', start);
        if (end == std::string::npos) end = all.size();
        stack.push_back(all.substr(start, end - start));
        start = end + 1;
    }
    return FontStackHasher()(stack);
}

// LanguageTag::fromBCP47(tag).toBCP47() plus the three fields, written as
// "language|script|region|bcp47" with "-" for an absent field.
size_t ref_language_tag(const char* tag, size_t len, char* out, size_t cap) {
    LanguageTag t = LanguageTag::fromBCP47(std::string(tag, len));
    std::string s = (t.language ? *t.language : "-") + "|" + (t.script ? *t.script : "-") + "|" +
                    (t.region ? *t.region : "-") + "|" + t.toBCP47();
    std::memcpy(out, s.data(), std::min(cap, s.size()));
    return s.size();
}

// --- HarfBuzz Unicode functions ------------------------------------------------

uint32_t ref_hb_general_category(uint32_t u) {
    return hb_unicode_general_category(hb_unicode_funcs_get_default(), u);
}

uint32_t ref_hb_script(uint32_t u) {
    return hb_unicode_script(hb_unicode_funcs_get_default(), u);
}

uint32_t ref_hb_combining_class(uint32_t u) {
    return hb_unicode_combining_class(hb_unicode_funcs_get_default(), u);
}

} // extern "C"
