// extern "C" wrappers around mapbox::earcut (vendor/earcut.hpp), with the
// point types and nth<> specializations the core uses:
// - mln::GeometryCoordinate = point<int16_t> read as int64_t
//   (gfx/fill_generator.cpp, renderer/buckets/fill_extrusion_bucket.cpp),
// - mln::GeometryCoordinateFloat = point<float> read as float
//   (fill_extrusion_bucket.cpp),
// - std::pair<double, double> (the generic nth, as earcut's own tests use).

// earcut.hpp uses uint32_t without including <cstdint>; the core's
// translation units include it first, too.
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <utility>
#include <vector>

#include <mapbox/earcut.hpp>
#include <mapbox/geometry/point.hpp>

namespace mapbox {
namespace util {
template <>
struct nth<0, mapbox::geometry::point<int16_t>> {
    static int64_t get(const mapbox::geometry::point<int16_t>& t) { return t.x; };
};

template <>
struct nth<1, mapbox::geometry::point<int16_t>> {
    static int64_t get(const mapbox::geometry::point<int16_t>& t) { return t.y; };
};

template <>
struct nth<0, mapbox::geometry::point<float>> {
    static float get(const mapbox::geometry::point<float>& t) { return t.x; };
};

template <>
struct nth<1, mapbox::geometry::point<float>> {
    static float get(const mapbox::geometry::point<float>& t) { return t.y; };
};
} // namespace util
} // namespace mapbox

namespace {

template <class Point, class Coord>
std::vector<std::vector<Point>> makePolygon(const Coord* xy, const uint32_t* ringSizes, std::size_t numRings) {
    std::vector<std::vector<Point>> polygon;
    std::size_t k = 0;
    for (std::size_t r = 0; r < numRings; ++r) {
        std::vector<Point> ring;
        for (uint32_t i = 0; i < ringSizes[r]; ++i, k += 2) {
            ring.push_back(Point{xy[k], xy[k + 1]});
        }
        polygon.push_back(std::move(ring));
    }
    return polygon;
}

std::size_t copyOut(const std::vector<uint32_t>& indices, uint32_t* out, std::size_t cap) {
    if (indices.size() <= cap && !indices.empty()) {
        std::memcpy(out, indices.data(), indices.size() * sizeof(uint32_t));
    }
    return indices.size();
}

} // namespace

// Each returns the number of indices; they are written to `out` only if
// they fit in `cap`.
extern "C" std::size_t ref_earcut_i16(
    const int16_t* xy, const uint32_t* ringSizes, std::size_t numRings, uint32_t* out, std::size_t cap) {
    const auto polygon = makePolygon<mapbox::geometry::point<int16_t>>(xy, ringSizes, numRings);
    return copyOut(mapbox::earcut(polygon), out, cap);
}

extern "C" std::size_t ref_earcut_f32(
    const float* xy, const uint32_t* ringSizes, std::size_t numRings, uint32_t* out, std::size_t cap) {
    const auto polygon = makePolygon<mapbox::geometry::point<float>>(xy, ringSizes, numRings);
    return copyOut(mapbox::earcut(polygon), out, cap);
}

extern "C" std::size_t ref_earcut_f64(
    const double* xy, const uint32_t* ringSizes, std::size_t numRings, uint32_t* out, std::size_t cap) {
    const auto polygon = makePolygon<std::pair<double, double>>(xy, ringSizes, numRings);
    return copyOut(mapbox::earcut(polygon), out, cap);
}
