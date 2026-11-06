// The canonical JSON form of the //difftest/geometry references (see
// geojson_reference.cpp): doubles as IEEE bit patterns, objects sorted by
// key, values tagged with their variant type.

#pragma once

#include <mapbox/feature.hpp>
#include <mapbox/geojson.hpp>
#include <mapbox/geojson/rapidjson.hpp>

#include <rapidjson/document.h>
#include <rapidjson/stringbuffer.h>
#include <rapidjson/writer.h>

#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <map>
#include <stdexcept>
#include <string>
#include <vector>

namespace canonical {

using Writer = rapidjson::Writer<rapidjson::StringBuffer>;
using Document = mapbox::geojson::rapidjson_document;
using JSValue = mapbox::geojson::rapidjson_value;

inline uint64_t bits(double d) {
    uint64_t b;
    std::memcpy(&b, &d, sizeof b);
    return b;
}

inline void writeCoord(Writer& w, double d) {
    w.Uint64(bits(d));
}

inline void writeCoord(Writer& w, int16_t i) {
    w.Int(i);
}

template <class T>
void writePoint(Writer& w, const mapbox::geometry::point<T>& p) {
    w.StartArray();
    writeCoord(w, p.x);
    writeCoord(w, p.y);
    w.EndArray();
}

template <class T>
void writePoints(Writer& w, const std::vector<mapbox::geometry::point<T>>& points) {
    w.StartArray();
    for (const auto& p : points) writePoint(w, p);
    w.EndArray();
}

template <class T>
void writeGeometry(Writer& w, const mapbox::geometry::geometry<T>& g);

template <class T>
struct GeometryWriter {
    Writer& w;
    void type(const char* t) {
        w.StartObject();
        w.Key("t");
        w.String(t);
        w.Key("c");
    }
    void operator()(const mapbox::geometry::empty&) { w.Null(); }
    void operator()(const mapbox::geometry::point<T>& p) {
        type("Point");
        writePoint(w, p);
        w.EndObject();
    }
    void operator()(const mapbox::geometry::multi_point<T>& g) {
        type("MultiPoint");
        writePoints(w, g);
        w.EndObject();
    }
    void operator()(const mapbox::geometry::line_string<T>& g) {
        type("LineString");
        writePoints(w, g);
        w.EndObject();
    }
    void operator()(const mapbox::geometry::multi_line_string<T>& g) {
        type("MultiLineString");
        w.StartArray();
        for (const auto& l : g) writePoints(w, l);
        w.EndArray();
        w.EndObject();
    }
    void operator()(const mapbox::geometry::polygon<T>& g) {
        type("Polygon");
        w.StartArray();
        for (const auto& r : g) writePoints(w, r);
        w.EndArray();
        w.EndObject();
    }
    void operator()(const mapbox::geometry::multi_polygon<T>& g) {
        type("MultiPolygon");
        w.StartArray();
        for (const auto& p : g) {
            w.StartArray();
            for (const auto& r : p) writePoints(w, r);
            w.EndArray();
        }
        w.EndArray();
        w.EndObject();
    }
    void operator()(const mapbox::geometry::geometry_collection<T>& g) {
        w.StartObject();
        w.Key("t");
        w.String("GeometryCollection");
        w.Key("g");
        w.StartArray();
        for (const auto& e : g) writeGeometry(w, e);
        w.EndArray();
        w.EndObject();
    }
};

template <class T>
void writeGeometry(Writer& w, const mapbox::geometry::geometry<T>& g) {
    mapbox::geometry::geometry<T>::visit(g, GeometryWriter<T>{w});
}

inline void writeValue(Writer& w, const mapbox::feature::value& v);

inline void writePropertyMap(Writer& w, const mapbox::feature::property_map& map) {
    std::map<std::string, const mapbox::feature::value*> sorted;
    for (const auto& p : map) sorted.emplace(p.first, &p.second);
    w.StartArray();
    for (const auto& p : sorted) {
        w.StartArray();
        w.String(p.first.data(), static_cast<rapidjson::SizeType>(p.first.size()));
        writeValue(w, *p.second);
        w.EndArray();
    }
    w.EndArray();
}

struct ValueWriter {
    Writer& w;
    void tag(const char* t) {
        w.StartArray();
        w.String(t);
    }
    void operator()(mapbox::feature::null_value_t) {
        tag("n");
        w.EndArray();
    }
    void operator()(bool b) {
        tag("b");
        w.Bool(b);
        w.EndArray();
    }
    void operator()(uint64_t u) {
        tag("u");
        w.Uint64(u);
        w.EndArray();
    }
    void operator()(int64_t i) {
        tag("i");
        w.Int64(i);
        w.EndArray();
    }
    void operator()(double d) {
        tag("d");
        w.Uint64(bits(d));
        w.EndArray();
    }
    void operator()(const std::string& s) {
        tag("s");
        w.String(s.data(), static_cast<rapidjson::SizeType>(s.size()));
        w.EndArray();
    }
    void operator()(const std::vector<mapbox::feature::value>& a) {
        tag("a");
        w.StartArray();
        for (const auto& v : a) writeValue(w, v);
        w.EndArray();
        w.EndArray();
    }
    void operator()(const mapbox::feature::property_map& m) {
        tag("o");
        writePropertyMap(w, m);
        w.EndArray();
    }
};

inline void writeValue(Writer& w, const mapbox::feature::value& v) {
    mapbox::feature::value::visit(v, ValueWriter{w});
}

inline void writeIdentifier(Writer& w, const mapbox::feature::identifier& id) {
    mapbox::feature::identifier::visit(id, ValueWriter{w});
}

template <class T>
void writeFeature(Writer& w, const mapbox::feature::feature<T>& f) {
    w.StartObject();
    w.Key("g");
    writeGeometry(w, f.geometry);
    w.Key("p");
    writePropertyMap(w, f.properties);
    w.Key("id");
    writeIdentifier(w, f.id);
    w.EndObject();
}

template <class T>
void writeFeatures(Writer& w, const std::vector<mapbox::feature::feature<T>>& features) {
    w.StartArray();
    for (const auto& f : features) writeFeature(w, f);
    w.EndArray();
}

inline void writeGeoJSON(Writer& w, const mapbox::geojson::geojson& g) {
    w.StartObject();
    g.match(
        [&](const mapbox::geojson::geometry& geometry) {
            w.Key("geometry");
            writeGeometry(w, geometry);
        },
        [&](const mapbox::geojson::feature& feature) {
            w.Key("feature");
            writeFeature(w, feature);
        },
        [&](const mapbox::geojson::feature_collection& collection) {
            w.Key("feature_collection");
            writeFeatures(w, collection);
        });
    w.EndObject();
}

inline void writeError(Writer& w, const char* what) {
    w.StartObject();
    w.Key("error");
    w.String(what);
    w.EndObject();
}

inline char* result(const rapidjson::StringBuffer& buffer) {
    char* out = static_cast<char*>(std::malloc(buffer.GetSize() + 1));
    std::memcpy(out, buffer.GetString(), buffer.GetSize() + 1);
    return out;
}

/// A request document, parsed with full precision (correctly rounded, as
/// serde_json reads it).
inline Document parseRequest(const char* text, std::size_t len) {
    Document d;
    d.Parse<rapidjson::kParseFullPrecisionFlag>(text, len);
    if (d.HasParseError()) {
        throw std::runtime_error("reference: request is not valid JSON");
    }
    return d;
}

} // namespace canonical
