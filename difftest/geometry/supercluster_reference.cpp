// extern "C" wrappers around kdbush::KDBush (vendor/kdbush.hpp) and
// mapbox::supercluster::Supercluster (vendor/supercluster), returning the
// canonical JSON of canonical.hpp (free with ref_geojson_free).
//
// mapbox::geojson::convert comes from geojson_reference.cpp (geojson_impl.hpp
// must be compiled once per binary).

// supercluster.hpp uses M_PI and std::unordered_map without including
// their headers; the core includes them first, too.
#include <cmath>
#include <unordered_map>

#include <kdbush.hpp>
#include <supercluster.hpp>

#include "canonical.hpp"

#include <cstdint>
#include <set>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

using namespace canonical;

namespace {

template <class TPoint, class TIndex, class Number>
void runKDBush(Writer& w, const JSValue& request, Number (*number)(const JSValue&)) {
    std::vector<TPoint> points;
    for (const auto& p : request["points"].GetArray()) {
        points.emplace_back(number(p[0]), number(p[1]));
    }
    kdbush::KDBush<TPoint, TIndex> index(points, static_cast<uint8_t>(request["node_size"].GetUint()));

    w.StartObject();
    w.Key("results");
    w.StartArray();
    for (const auto& q : request["queries"].GetArray()) {
        std::vector<TIndex> result;
        const std::string kind = q[0].GetString();
        if (kind == "range") {
            index.range(number(q[1]), number(q[2]), number(q[3]), number(q[4]), [&](const auto id) {
                result.push_back(id);
            });
        } else {
            index.within(number(q[1]), number(q[2]), number(q[3]), [&](const auto id) { result.push_back(id); });
        }
        w.StartArray();
        for (auto id : result) w.Uint64(id);
        w.EndArray();
    }
    w.EndArray();
    w.EndObject();
}

double toDouble(const JSValue& v) {
    return v.GetDouble();
}

int toInt(const JSValue& v) {
    return v.GetInt();
}

using mapbox::feature::property_map;
using mapbox::feature::value;

// The map/reduce options of the supercluster difftest (identical in
// supercluster_difftest.rs).
void setMapReduce(mapbox::supercluster::Options& options, unsigned mode) {
    switch (mode) {
        case 1: // supercluster's own test: sum of "n"
            options.map = [](const property_map& properties) -> property_map {
                property_map ret{};
                auto iter = properties.find("n");
                if (iter != properties.end()) {
                    ret["sum"] = iter->second.get<std::uint64_t>();
                }
                return ret;
            };
            options.reduce = [](property_map& toUpdate, const property_map& toFill) {
                auto iter1 = toUpdate.find("sum");
                auto iter2 = toFill.find("sum");
                if (iter1 != toUpdate.end() && iter2 != toFill.end()) {
                    iter1->second.set<std::uint64_t>(iter1->second.get<std::uint64_t>() +
                                                     iter2->second.get<std::uint64_t>());
                }
            };
            break;
        case 2: // order-sensitive: concatenated names, a non-commutative double
            options.map = [](const property_map& properties) -> property_map {
                property_map ret{};
                auto name = properties.find("name");
                if (name == properties.end()) return ret;
                ret["names"] = name->second.get<std::string>();
                auto n = properties.find("n");
                ret["w"] = n != properties.end() ? double(n->second.get<std::uint64_t>()) : 0.5;
                return ret;
            };
            options.reduce = [](property_map& toUpdate, const property_map& toFill) {
                auto names1 = toUpdate.find("names");
                auto names2 = toFill.find("names");
                if (names1 != toUpdate.end() && names2 != toFill.end()) {
                    names1->second = names1->second.get<std::string>() + "," + names2->second.get<std::string>();
                }
                auto w1 = toUpdate.find("w");
                auto w2 = toFill.find("w");
                if (w1 != toUpdate.end() && w2 != toFill.end()) {
                    w1->second = w1->second.get<double>() * 0.5 + w2->second.get<double>();
                }
            };
            break;
        case 3: // default (identity) map, counting reduce
            options.reduce = [](property_map& toUpdate, const property_map&) {
                auto it = toUpdate.find("count");
                const std::uint64_t count = it != toUpdate.end() ? it->second.get<std::uint64_t>() : 0;
                toUpdate["count"] = count + 1;
            };
            break;
        default:
            break;
    }
}

template <class F>
void tryWrite(Writer& w, F&& f) {
    // Write into a side buffer so an exception leaves no partial output.
    rapidjson::StringBuffer side;
    Writer sw(side);
    try {
        f(sw);
        w.RawValue(side.GetString(), side.GetSize(), rapidjson::kObjectType);
    } catch (const std::exception& e) {
        writeError(w, e.what());
    }
}

} // namespace

// Request: {"type": "f64" | "i32", "node_size": n, "points": [[x, y]...],
// "queries": [["range", minX, minY, maxX, maxY] | ["within", qx, qy, r]...]}.
// KDBush<pair<double, double>, uint32_t> (supercluster's arithmetic) or
// KDBush<pair<int, int>, size_t> (kdbush's own test).
extern "C" char* ref_kdbush(const char* text, std::size_t len) {
    rapidjson::StringBuffer buffer;
    Writer w(buffer);
    Document d = parseRequest(text, len);
    if (std::string(d["type"].GetString()) == "f64") {
        runKDBush<std::pair<double, double>, uint32_t>(w, d, toDouble);
    } else {
        runKDBush<std::pair<int, int>, std::size_t>(w, d, toInt);
    }
    return result(buffer);
}

// Cluster::getProperties()["point_count_abbreviated"]
extern "C" char* ref_supercluster_abbreviation(uint32_t numPoints) {
    const mapbox::supercluster::Cluster c({0, 0}, numPoints, 0);
    const auto s = c.getProperties()["point_count_abbreviated"].get<std::string>();
    rapidjson::StringBuffer buffer;
    buffer.Put('"');
    for (char ch : s) buffer.Put(ch);
    buffer.Put('"');
    return result(buffer);
}

// Request: {"features": FeatureCollection, "options": {min_zoom, max_zoom,
// radius, extent, min_points, generate_id}, "mode": map/reduce mode,
// "tiles": [[z, x, y]...], "extra_ids": [...], "leaf_args": [[limit,
// offset]...]}. Result: {"tiles": [features...], "clusters": [{"id",
// "children", "leaves": [...], "expansion"}...]} for every cluster id in the
// tiles (first appearance order) and the extra ids.
extern "C" char* ref_supercluster(const char* text, std::size_t len) {
    rapidjson::StringBuffer buffer;
    Writer w(buffer);
    try {
        Document d = parseRequest(text, len);
        const auto geojson = mapbox::geojson::convert(d["features"]);
        const auto& features = geojson.get<mapbox::geojson::feature_collection>();
        const auto& o = d["options"];
        mapbox::supercluster::Options options;
        options.minZoom = static_cast<uint8_t>(o["min_zoom"].GetUint());
        options.maxZoom = static_cast<uint8_t>(o["max_zoom"].GetUint());
        options.radius = static_cast<uint16_t>(o["radius"].GetUint());
        options.extent = static_cast<uint16_t>(o["extent"].GetUint());
        options.minPoints = o["min_points"].GetUint();
        options.generateId = o["generate_id"].GetBool();
        setMapReduce(options, d["mode"].GetUint());

        mapbox::supercluster::Supercluster index(features, options);

        std::vector<uint32_t> ids;
        std::set<uint32_t> seen;
        w.StartObject();
        w.Key("tiles");
        w.StartArray();
        for (const auto& q : d["tiles"].GetArray()) {
            const auto tile = index.getTile(static_cast<uint8_t>(q[0].GetUint()), q[1].GetUint(), q[2].GetUint());
            writeFeatures(w, tile);
            for (const auto& f : tile) {
                auto it = f.properties.find("cluster_id");
                if (it != f.properties.end() && it->second.is<std::uint64_t>()) {
                    const auto id = static_cast<uint32_t>(it->second.get<std::uint64_t>());
                    if (seen.insert(id).second) ids.push_back(id);
                }
            }
        }
        w.EndArray();
        for (const auto& id : d["extra_ids"].GetArray()) ids.push_back(id.GetUint());

        w.Key("clusters");
        w.StartArray();
        for (const auto id : ids) {
            w.StartObject();
            w.Key("id");
            w.Uint(id);
            w.Key("children");
            tryWrite(w, [&](Writer& sw) {
                sw.StartObject();
                sw.Key("features");
                writeFeatures(sw, index.getChildren(id));
                sw.EndObject();
            });
            w.Key("leaves");
            w.StartArray();
            for (const auto& a : d["leaf_args"].GetArray()) {
                tryWrite(w, [&](Writer& sw) {
                    sw.StartObject();
                    sw.Key("features");
                    writeFeatures(sw, index.getLeaves(id, a[0].GetUint(), a[1].GetUint()));
                    sw.EndObject();
                });
            }
            w.EndArray();
            w.Key("expansion");
            tryWrite(w, [&](Writer& sw) {
                sw.StartObject();
                sw.Key("zoom");
                sw.Uint(index.getClusterExpansionZoom(id));
                sw.EndObject();
            });
            w.EndObject();
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
