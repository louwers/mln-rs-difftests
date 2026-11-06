// extern "C" wrappers around mapbox::geojson (geojson.hpp's geojson_impl.hpp,
// as src/mln/util/geojson_impl.cpp compiles it into the core) and
// mapbox::geojsonvt (geojson-vt-cpp).
//
// Inputs are JSON texts, parsed with rapidjson's full-precision flag (so the
// doubles are the correctly rounded ones serde_json reads on the Rust side)
// unless noted. Results are returned as a canonical JSON text (malloc'ed;
// free with ref_geojson_free) in which every double is written as its IEEE
// bit pattern, objects are sorted by key and values carry their variant
// type, so the Rust side can compare exactly:
//   geometry: null | {"t": type, "c": coordinates} | {"t": "GeometryCollection", "g": [...]}
//   value:    ["n"] | ["b", bool] | ["u", uint] | ["i", int] | ["d", bits] | ["s", str]
//             | ["a", [value...]] | ["o", [[key, value]...]]
//   feature:  {"g": geometry, "p": [[key, value]...], "id": value}
// Exceptions become {"error": what()}.

#include <mapbox/geojson.hpp>
#include <mapbox/geojson_impl.hpp>
#include <mapbox/geojsonvt.hpp>

#include <rapidjson/document.h>
#include <rapidjson/stringbuffer.h>
#include <rapidjson/writer.h>

#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <map>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

#include "canonical.hpp"

namespace {

using namespace canonical;

void writeTile(Writer& w, const mapbox::geojsonvt::Tile& tile) {
    w.StartObject();
    w.Key("features");
    writeFeatures(w, tile.features);
    w.Key("num_points");
    w.Uint(tile.num_points);
    w.Key("num_simplified");
    w.Uint(tile.num_simplified);
    w.EndObject();
}

mapbox::geojsonvt::TileOptions tileOptions(const JSValue& o) {
    mapbox::geojsonvt::TileOptions options;
    options.tolerance = o["tolerance"].GetDouble();
    options.extent = static_cast<uint16_t>(o["extent"].GetUint());
    options.buffer = static_cast<uint16_t>(o["buffer"].GetUint());
    options.lineMetrics = o["line_metrics"].GetBool();
    return options;
}

} // namespace

extern "C" void ref_geojson_free(char* p) {
    std::free(p);
}

// `mapbox::geojson::convert` of a full-precision rapidjson document.
extern "C" char* ref_geojson_convert(const char* text, std::size_t len) {
    rapidjson::StringBuffer buffer;
    Writer w(buffer);
    try {
        Document d = parseRequest(text, len);
        writeGeoJSON(w, mapbox::geojson::convert(d));
    } catch (const std::exception& e) {
        buffer.Clear();
        Writer ew(buffer);
        writeError(ew, e.what());
    }
    return result(buffer);
}

// `mapbox::geojson::parse(std::string)` (rapidjson's default precision).
extern "C" char* ref_geojson_parse(const char* text, std::size_t len) {
    rapidjson::StringBuffer buffer;
    Writer w(buffer);
    try {
        writeGeoJSON(w, mapbox::geojson::parse(std::string(text, len)));
    } catch (const std::exception& e) {
        buffer.Clear();
        Writer ew(buffer);
        writeError(ew, e.what());
    }
    return result(buffer);
}

// `stringify(convert(document))`: the bytes of mapbox::geojson::stringify.
extern "C" char* ref_geojson_stringify(const char* text, std::size_t len) {
    try {
        Document d = parseRequest(text, len);
        const std::string s = mapbox::geojson::stringify(mapbox::geojson::convert(d));
        char* out = static_cast<char*>(std::malloc(s.size() + 1));
        std::memcpy(out, s.c_str(), s.size() + 1);
        return out;
    } catch (const std::exception& e) {
        const std::string s = std::string("error: ") + e.what();
        char* out = static_cast<char*>(std::malloc(s.size() + 1));
        std::memcpy(out, s.c_str(), s.size() + 1);
        return out;
    }
}

// stringify of a Point geometry and of a Feature with a double property:
// number formatting, including the NaN/infinity truncation.
extern "C" char* ref_geojson_stringify_doubles(double x, double y, double property) {
    mapbox::geojson::feature f{mapbox::geojson::point{x, y}};
    f.properties.emplace("v", property);
    const std::string s = mapbox::geojson::stringify(mapbox::geojson::geometry{mapbox::geojson::point{x, y}}) +
                          "|" + mapbox::geojson::stringify(f);
    char* out = static_cast<char*>(std::malloc(s.size() + 1));
    std::memcpy(out, s.c_str(), s.size() + 1);
    return out;
}

// Request: {"geojson": GeoJSON, "options": {tolerance, extent, buffer,
// line_metrics, max_zoom, index_max_zoom, index_max_points, generate_id},
// "queries": [[z, x, y], ...]}. Result: {"tiles": [tile or error...],
// "total": n, "stats": [[z, count]...], "internal": [[z, x, y]...] (sorted)}.
extern "C" char* ref_geojsonvt(const char* text, std::size_t len) {
    rapidjson::StringBuffer buffer;
    Writer w(buffer);
    try {
        Document d = parseRequest(text, len);
        const auto geojson = mapbox::geojson::convert(d["geojson"]);
        const auto& o = d["options"];
        mapbox::geojsonvt::Options options;
        static_cast<mapbox::geojsonvt::TileOptions&>(options) = tileOptions(o);
        options.maxZoom = static_cast<uint8_t>(o["max_zoom"].GetUint());
        options.indexMaxZoom = static_cast<uint8_t>(o["index_max_zoom"].GetUint());
        options.indexMaxPoints = o["index_max_points"].GetUint();
        options.generateId = o["generate_id"].GetBool();

        mapbox::geojsonvt::GeoJSONVT index{geojson, options};

        w.StartObject();
        w.Key("tiles");
        w.StartArray();
        for (const auto& q : d["queries"].GetArray()) {
            try {
                const auto& tile = index.getTile(
                    static_cast<uint8_t>(q[0].GetUint()), q[1].GetUint(), q[2].GetUint());
                w.StartObject();
                w.Key("empty_tile");
                w.Bool(&tile == &mapbox::geojsonvt::empty_tile);
                w.Key("tile");
                writeTile(w, tile);
                w.EndObject();
            } catch (const std::exception& e) {
                writeError(w, e.what());
            }
        }
        w.EndArray();
        w.Key("total");
        w.Uint(index.total);
        w.Key("stats");
        w.StartArray();
        for (const auto& s : index.stats) {
            w.StartArray();
            w.Uint(s.first);
            w.Uint(s.second);
            w.EndArray();
        }
        w.EndArray();
        w.Key("internal");
        std::set<std::vector<uint32_t>> internal;
        for (const auto& t : index.getInternalTiles()) {
            internal.insert({t.second.z, t.second.x, t.second.y, static_cast<uint32_t>(t.second.source_features.size())});
        }
        w.StartArray();
        for (const auto& t : internal) {
            w.StartArray();
            for (auto v : t) w.Uint(v);
            w.EndArray();
        }
        w.EndArray();
        w.EndObject();
    } catch (const std::exception& e) {
        buffer.Clear();
        Writer ew(buffer);
        writeError(ew, e.what());
    }
    return result(buffer);
}

// Request: {"geojson": GeoJSON, "z", "x", "y", "options": {tolerance,
// extent, buffer, line_metrics}, "wrap", "clip"}. Result: the tile.
extern "C" char* ref_geojson_to_tile(const char* text, std::size_t len) {
    rapidjson::StringBuffer buffer;
    Writer w(buffer);
    try {
        Document d = parseRequest(text, len);
        const auto geojson = mapbox::geojson::convert(d["geojson"]);
        const auto tile = mapbox::geojsonvt::geoJSONToTile(geojson,
                                                           static_cast<uint8_t>(d["z"].GetUint()),
                                                           d["x"].GetUint(),
                                                           d["y"].GetUint(),
                                                           tileOptions(d["options"]),
                                                           d["wrap"].GetBool(),
                                                           d["clip"].GetBool());
        writeTile(w, tile);
    } catch (const std::exception& e) {
        buffer.Clear();
        Writer ew(buffer);
        writeError(ew, e.what());
    }
    return result(buffer);
}
