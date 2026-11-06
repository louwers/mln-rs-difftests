// extern "C" wrappers around the C++ util/math functions the Rust port
// (//crates/mln_math, //crates/mln_util, //vendor/csscolorparser,
// //vendor/rapidjson) must match. Strings cross as (ptr, len); results that
// are strings are written to a caller buffer (the full length is returned).

#include <mln/math/angles.hpp>
#include <mln/math/clamp.hpp>
#include <mln/math/log2.hpp>
#include <mln/math/minmax.hpp>
#include <mln/math/wrap.hpp>
#include <mln/tile/tile_id.hpp>
#include <mln/util/color.hpp>
#include <mln/util/geo.hpp>
#include <mln/util/interpolate.hpp>
#include <mln/util/string.hpp>
#include <mln/util/unitbezier.hpp>

#include <csscolorparser/csscolorparser.hpp>
#include <rapidjson/stringbuffer.h>
#include <rapidjson/writer.h>

#include <cerrno>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <stdexcept>
#include <string>

namespace {

size_t copy_out(const std::string& s, char* buf, size_t cap) {
    if (cap > 0) {
        std::memcpy(buf, s.data(), std::min(cap, s.size()));
    }
    return s.size();
}

mln::Color make_color(const float c[4]) {
    // Field-wise, to bypass the constructor's range asserts.
    mln::Color color;
    color.r = c[0];
    color.g = c[1];
    color.b = c[2];
    color.a = c[3];
    return color;
}

void color_out(const mln::Color& c, float out[4]) {
    out[0] = c.r;
    out[1] = c.g;
    out[2] = c.b;
    out[3] = c.a;
}

} // namespace

extern "C" {

// --- math ---

double ref_wrap(double v, double lo, double hi) {
    return mln::util::wrap(v, lo, hi);
}
float ref_wrapf(float v, float lo, float hi) {
    return mln::util::wrap(v, lo, hi);
}
double ref_clamp(double v, double lo, double hi) {
    return mln::util::clamp(v, lo, hi);
}
float ref_clampf(float v, float lo, float hi) {
    return mln::util::clamp(v, lo, hi);
}
int64_t ref_clamp_i64(int64_t v, int64_t lo, int64_t hi) {
    return mln::util::clamp(v, lo, hi);
}
double ref_min(double a, double b) {
    return mln::util::min(a, b);
}
double ref_max(double a, double b) {
    return mln::util::max(a, b);
}
float ref_minf(float a, float b) {
    return mln::util::min(a, b);
}
float ref_maxf(float a, float b) {
    return mln::util::max(a, b);
}
uint32_t ref_ceil_log2(uint64_t x) {
    return mln::util::ceil_log2(x);
}
double ref_log2(double x) {
    return mln::util::log2(x);
}
void ref_angles(double d, float f, double out_d[2], float out_f[2]) {
    out_d[0] = mln::util::deg2rad(d);
    out_d[1] = mln::util::rad2deg(d);
    out_f[0] = mln::util::deg2radf(f);
    out_f[1] = mln::util::rad2degf(f);
}

// --- unitbezier ---

// out: solve, solveCurveX, sampleCurveX(x), sampleCurveY(x),
// sampleCurveDerivativeX(x), p1.x, p1.y, p2.x, p2.y
void ref_unitbezier(double p1x, double p1y, double p2x, double p2y, double x, double epsilon, double out[9]) {
    const mln::util::UnitBezier b(p1x, p1y, p2x, p2y);
    out[0] = b.solve(x, epsilon);
    out[1] = b.solveCurveX(x, epsilon);
    out[2] = b.sampleCurveX(x);
    out[3] = b.sampleCurveY(x);
    out[4] = b.sampleCurveDerivativeX(x);
    const auto p1 = b.getP1();
    const auto p2 = b.getP2();
    out[5] = p1.first;
    out[6] = p1.second;
    out[7] = p2.first;
    out[8] = p2.second;
}

// --- C library ---

float ref_strtof(const char* s, size_t len, size_t* consumed, int* erange) {
    const std::string str(s, len);
    char* end = nullptr;
    errno = 0;
    const float v = std::strtof(str.c_str(), &end);
    *erange = errno == ERANGE;
    *consumed = static_cast<size_t>(end - str.c_str());
    return v;
}

double ref_strtod(const char* s, size_t len, size_t* consumed, int* erange) {
    const std::string str(s, len);
    char* end = nullptr;
    errno = 0;
    const double v = std::strtod(str.c_str(), &end);
    *erange = errno == ERANGE;
    *consumed = static_cast<size_t>(end - str.c_str());
    return v;
}

int64_t ref_strtoll(const char* s, size_t len, int base, size_t* consumed, int* erange) {
    const std::string str(s, len);
    char* end = nullptr;
    errno = 0;
    const long long v = std::strtoll(str.c_str(), &end, base);
    *erange = errno == ERANGE;
    *consumed = static_cast<size_t>(end - str.c_str());
    return v;
}

// 0: ok, 1: std::invalid_argument, 2: std::out_of_range
int ref_stof(const char* s, size_t len, float* out) {
    try {
        *out = mln::util::stof(std::string(s, len));
        return 0;
    } catch (const std::invalid_argument&) {
        return 1;
    } catch (const std::out_of_range&) {
        return 2;
    }
}

// --- csscolorparser ---

int ref_css_parse(const char* s, size_t len, uint8_t rgb[3], float* a) {
    const auto c = CSSColorParser::parse(std::string(s, len));
    if (!c) return 0;
    rgb[0] = c->r;
    rgb[1] = c->g;
    rgb[2] = c->b;
    *a = c->a;
    return 1;
}

int ref_css_equal(const uint8_t rgb_a[3], float a_a, const uint8_t rgb_b[3], float a_b) {
    CSSColorParser::Color a(rgb_a[0], rgb_a[1], rgb_a[2], a_a);
    CSSColorParser::Color b(rgb_b[0], rgb_b[1], rgb_b[2], a_b);
    return a == b;
}

// --- Color ---

// 0: none, 1: parsed, 2: skipped (the CSS alpha is NaN, which the
// mln::Color constructor asserts on)
int ref_color_parse(const char* s, size_t len, float out[4]) {
    const std::string str(s, len);
    const auto css = CSSColorParser::parse(str);
    if (css && std::isnan(css->a)) return 2;
    const auto c = mln::Color::parse(str);
    if (!c) return 0;
    color_out(*c, out);
    return 1;
}

void ref_color_interpolate(const float from[4], const float to[4], double t, int space, float out[4]) {
    const auto c = mln::Color::interpolate(make_color(from), make_color(to), t, static_cast<mln::ColorSpace>(space));
    color_out(c, out);
}

void ref_color_to_array(const float c[4], double out[4]) {
    const auto a = make_color(c).toArray();
    for (int i = 0; i < 4; i++) out[i] = a[i];
}

size_t ref_color_stringify(const float c[4], char* buf, size_t cap) {
    return copy_out(make_color(c).stringify(), buf, cap);
}

// --- string ---

size_t ref_to_string_double(double v, int decimal, char* buf, size_t cap) {
    return copy_out(mln::util::toString(v, decimal != 0), buf, cap);
}

size_t ref_to_string_i64(int64_t v, char* buf, size_t cap) {
    return copy_out(mln::util::toString(v), buf, cap);
}

size_t ref_to_string_u64(uint64_t v, char* buf, size_t cap) {
    return copy_out(mln::util::toString(v), buf, cap);
}

// --- rapidjson::Writer<StringBuffer> ---

// A byte program: 'n' null, 't'/'f' bool, 'i' i32, 'u' u32, 'I' i64, 'U' u64,
// 'd' double (little-endian payloads), 's'/'k' u32 length + bytes, '{' '}'
// '[' ']'. Writes the output; returns its length. `results` gets one byte
// per op: the handler's return value.
size_t ref_writer(const uint8_t* prog, size_t len, char* buf, size_t cap, uint8_t* results) {
    rapidjson::StringBuffer s;
    rapidjson::Writer<rapidjson::StringBuffer> writer(s);
    size_t i = 0;
    size_t op = 0;
    auto read = [&](void* dst, size_t n) {
        std::memcpy(dst, prog + i, n);
        i += n;
    };
    while (i < len) {
        const uint8_t c = prog[i++];
        bool r = false;
        switch (c) {
            case 'n':
                r = writer.Null();
                break;
            case 't':
                r = writer.Bool(true);
                break;
            case 'f':
                r = writer.Bool(false);
                break;
            case 'i': {
                int32_t v;
                read(&v, 4);
                r = writer.Int(v);
                break;
            }
            case 'u': {
                uint32_t v;
                read(&v, 4);
                r = writer.Uint(v);
                break;
            }
            case 'I': {
                int64_t v;
                read(&v, 8);
                r = writer.Int64(v);
                break;
            }
            case 'U': {
                uint64_t v;
                read(&v, 8);
                r = writer.Uint64(v);
                break;
            }
            case 'd': {
                double v;
                read(&v, 8);
                r = writer.Double(v);
                break;
            }
            case 's':
            case 'k': {
                uint32_t n;
                read(&n, 4);
                const std::string str(reinterpret_cast<const char*>(prog + i), n);
                i += n;
                r = c == 's' ? writer.String(str) : writer.Key(str);
                break;
            }
            case '{':
                r = writer.StartObject();
                break;
            case '}':
                r = writer.EndObject();
                break;
            case '[':
                r = writer.StartArray();
                break;
            case ']':
                r = writer.EndArray();
                break;
            default:
                std::abort();
        }
        results[op++] = r;
    }
    return copy_out(std::string(s.GetString(), s.GetSize()), buf, cap);
}

// --- interpolate ---

float ref_interpolate_f_d(float a, float b, double t) {
    return mln::util::interpolate(a, b, t);
}
float ref_interpolate_f_f(float a, float b, float t) {
    return mln::util::interpolate(a, b, t);
}
double ref_interpolate_d_d(double a, double b, double t) {
    return mln::util::interpolate(a, b, t);
}
void ref_interpolate_arr2_d(const float a[2], const float b[2], double t, float out[2]) {
    const auto r = mln::util::interpolate(std::array<float, 2>{a[0], a[1]}, std::array<float, 2>{b[0], b[1]}, t);
    out[0] = r[0];
    out[1] = r[1];
}
void ref_interpolate_padding(const float a[4], const float b[4], double td, float tf, float out_d[4], float out_f[4]) {
    const mln::Padding pa(a[0], a[1], a[2], a[3]);
    const mln::Padding pb(b[0], b[1], b[2], b[3]);
    const auto rd = mln::util::interpolate(pa, pb, td);
    const auto rf = mln::util::interpolate(pa, pb, tf);
    const float vd[4] = {rd.top, rd.right, rd.bottom, rd.left};
    const float vf[4] = {rf.top, rf.right, rf.bottom, rf.left};
    std::memcpy(out_d, vd, sizeof vd);
    std::memcpy(out_f, vf, sizeof vf);
}
float ref_interpolation_factor(float base, float min, float max, float z) {
    return mln::util::interpolationFactor(base, mln::Range<float>(min, max), z);
}

// --- geo ---

// 0: ok (out = lat, lon), 1: std::domain_error (message in err)
int ref_latlng_new(double lat, double lon, int wrapped, double out[2], char* err, size_t cap, size_t* err_len) {
    try {
        const mln::LatLng ll(lat, lon, wrapped ? mln::LatLng::Wrapped : mln::LatLng::Unwrapped);
        out[0] = ll.latitude();
        out[1] = ll.longitude();
        return 0;
    } catch (const std::domain_error& e) {
        *err_len = copy_out(e.what(), err, cap);
        return 1;
    }
}

void ref_latlng_unwrap_for_shortest_path(double lat, double lon, double end_lat, double end_lon, double out[2]) {
    mln::LatLng ll(lat, lon);
    ll.unwrapForShortestPath(mln::LatLng(end_lat, end_lon));
    out[0] = ll.latitude();
    out[1] = ll.longitude();
}

void ref_latlng_tile(uint8_t z, uint32_t x, uint32_t y, int16_t wrap, double out[4]) {
    const mln::CanonicalTileID canonical(z, x, y);
    const mln::LatLng a(canonical);
    const mln::LatLng b(mln::UnwrappedTileID(wrap, canonical));
    out[0] = a.latitude();
    out[1] = a.longitude();
    out[2] = b.latitude();
    out[3] = b.longitude();
}

struct BoundsSpec {
    // 0 world, 1 empty, 2 infinite (default), 3 singleton(a), 4 hull(a, b),
    // 5 hull(a, b) extended by c, 6 tile(z, x, y), 7 empty extended by a, b
    int32_t kind;
    double a_lat, a_lon, b_lat, b_lon, c_lat, c_lon;
    uint32_t z, x, y;
};

struct BoundsQuery {
    double sw_lat, sw_lon, ne_lat, ne_lon;
    double se_lat, se_lon, nw_lat, nw_lon;
    double center_lat, center_lon;
    double constrained_lat, constrained_lon;
    uint8_t valid, is_empty, crosses;
    uint8_t contains_point, contains_point_wrapped;
    uint8_t contains_area, contains_area_wrapped;
    uint8_t intersects, intersects_wrapped;
    uint8_t equal;
};

static mln::LatLngBounds make_bounds(const BoundsSpec& s) {
    const mln::LatLng a(s.a_lat, s.a_lon);
    const mln::LatLng b(s.b_lat, s.b_lon);
    const mln::LatLng c(s.c_lat, s.c_lon);
    switch (s.kind) {
        case 0:
            return mln::LatLngBounds::world();
        case 1:
            return mln::LatLngBounds::empty();
        case 2:
            return mln::LatLngBounds();
        case 3:
            return mln::LatLngBounds::singleton(a);
        case 4:
            return mln::LatLngBounds::hull(a, b);
        case 5: {
            auto r = mln::LatLngBounds::hull(a, b);
            r.extend(c);
            return r;
        }
        case 6:
            return mln::LatLngBounds(mln::CanonicalTileID(static_cast<uint8_t>(s.z), s.x, s.y));
        default: {
            auto r = mln::LatLngBounds::empty();
            r.extend(a);
            r.extend(mln::LatLngBounds::singleton(b));
            return r;
        }
    }
}

void ref_bounds_query(
    const BoundsSpec* spec, const BoundsSpec* area_spec, double p_lat, double p_lon, int with_center, BoundsQuery* q) {
    const auto bounds = make_bounds(*spec);
    const auto area = make_bounds(*area_spec);
    const mln::LatLng p(p_lat, p_lon);
    q->sw_lat = bounds.southwest().latitude();
    q->sw_lon = bounds.southwest().longitude();
    q->ne_lat = bounds.northeast().latitude();
    q->ne_lon = bounds.northeast().longitude();
    q->se_lat = bounds.southeast().latitude();
    q->se_lon = bounds.southeast().longitude();
    q->nw_lat = bounds.northwest().latitude();
    q->nw_lon = bounds.northwest().longitude();
    if (with_center) {
        q->center_lat = bounds.center().latitude();
        q->center_lon = bounds.center().longitude();
    }
    const auto constrained = bounds.constrain(p);
    q->constrained_lat = constrained.latitude();
    q->constrained_lon = constrained.longitude();
    q->valid = bounds.valid();
    q->is_empty = bounds.isEmpty();
    q->crosses = bounds.crossesAntimeridian();
    q->contains_point = bounds.contains(p);
    q->contains_point_wrapped = bounds.contains(p, mln::LatLng::Wrapped);
    q->contains_area = bounds.contains(area);
    q->contains_area_wrapped = bounds.contains(area, mln::LatLng::Wrapped);
    q->intersects = bounds.intersects(area);
    q->intersects_wrapped = bounds.intersects(area, mln::LatLng::Wrapped);
    q->equal = bounds == area;
}

// 0: ok (out = top, left, bottom, right, center x, center y), 1: error
int ref_edge_insets(double t, double l, double b, double r, uint16_t w, uint16_t h, double out[6], char* err, size_t cap, size_t* err_len) {
    try {
        const mln::EdgeInsets e(t, l, b, r);
        const auto c = e.getCenter(w, h);
        out[0] = e.top();
        out[1] = e.left();
        out[2] = e.bottom();
        out[3] = e.right();
        out[4] = c.x;
        out[5] = c.y;
        return e.isFlush() ? 2 : 0;
    } catch (const std::domain_error& e) {
        *err_len = copy_out(e.what(), err, cap);
        return 1;
    }
}

} // extern "C"
