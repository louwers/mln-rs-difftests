// extern "C" wrappers around mapbox::polylabel (vendor/polylabel),
// mapbox::cheap_ruler::CheapRuler (cheap-ruler-cpp) and mapbox::ShelfPack
// (shelf-pack-cpp). Requests and results are JSON (canonical.hpp; free
// results with ref_geojson_free); doubles travel as IEEE bit patterns.

#include <mapbox/cheap_ruler.hpp>
#include <mapbox/polylabel.hpp>
#include <mapbox/shelf-pack.hpp>

#include "canonical.hpp"

#include <cstdint>
#include <cstring>
#include <stdexcept>
#include <string>
#include <vector>

using namespace canonical;

namespace {

double fromBits(const JSValue& v) {
    const uint64_t b = v.GetUint64();
    double d;
    std::memcpy(&d, &b, sizeof d);
    return d;
}

void writeDouble(Writer& w, double d) {
    w.Uint64(bits(d));
}

namespace cr = mapbox::cheap_ruler;

cr::point point(const JSValue& v) {
    return {fromBits(v[0]), fromBits(v[1])};
}

cr::line_string line(const JSValue& v) {
    cr::line_string l;
    for (const auto& p : v.GetArray()) l.push_back(point(p));
    return l;
}

cr::box box(const JSValue& v) {
    return cr::box(point(v[0]), point(v[1]));
}

void writePoint(Writer& w, const cr::point& p) {
    w.StartArray();
    writeDouble(w, p.x);
    writeDouble(w, p.y);
    w.EndArray();
}

void writeLine(Writer& w, const cr::line_string& l) {
    w.StartArray();
    for (const auto& p : l) writePoint(w, p);
    w.EndArray();
}

void writeBox(Writer& w, const cr::box& b) {
    w.StartArray();
    writePoint(w, b.min);
    writePoint(w, b.max);
    w.EndArray();
}

void writeBin(Writer& w, const mapbox::Bin* bin) {
    if (!bin) {
        w.Null();
        return;
    }
    w.StartArray();
    for (int32_t v : {bin->id, bin->x, bin->y, bin->w, bin->h, bin->maxw, bin->maxh, bin->refcount()}) w.Int(v);
    w.EndArray();
}

} // namespace

// polylabel(polygon, precision) of `std::pair`-free mapbox::geometry
// polygons; writes the point, returns false on an exception.
extern "C" bool ref_polylabel(
    const double* xy, const uint32_t* ringSizes, std::size_t numRings, double precision, double* out) {
    mapbox::geometry::polygon<double> polygon;
    std::size_t k = 0;
    for (std::size_t r = 0; r < numRings; ++r) {
        mapbox::geometry::linear_ring<double> ring;
        for (uint32_t i = 0; i < ringSizes[r]; ++i, k += 2) ring.push_back({xy[k], xy[k + 1]});
        polygon.push_back(std::move(ring));
    }
    try {
        const auto p = mapbox::polylabel(polygon, precision);
        out[0] = p.x;
        out[1] = p.y;
        return true;
    } catch (const std::exception&) {
        return false;
    }
}

// Request: {"ruler": ["new", lat, unit] | ["fromTile", y, z], "ops": [[name,
// args...]...]}; doubles as bits. Result: one value per op.
extern "C" char* ref_cheap_ruler(const char* text, std::size_t len) {
    Document d = parseRequest(text, len);
    const auto& r = d["ruler"];
    const cr::CheapRuler ruler =
        std::string(r[0].GetString()) == "new"
            ? cr::CheapRuler(fromBits(r[1]), static_cast<cr::CheapRuler::Unit>(r[2].GetUint()))
            : cr::CheapRuler::fromTile(r[1].GetUint(), r[2].GetUint());
    cr::CheapRuler mutableRuler = ruler; // lineDistance is not const

    rapidjson::StringBuffer buffer;
    Writer w(buffer);
    w.StartArray();
    for (const auto& op : d["ops"].GetArray()) {
        const std::string name = op[0].GetString();
        if (name == "distance") {
            writeDouble(w, ruler.distance(point(op[1]), point(op[2])));
        } else if (name == "squareDistance") {
            writeDouble(w, ruler.squareDistance(point(op[1]), point(op[2])));
        } else if (name == "bearing") {
            writeDouble(w, ruler.bearing(point(op[1]), point(op[2])));
        } else if (name == "destination") {
            writePoint(w, ruler.destination(point(op[1]), fromBits(op[2]), fromBits(op[3])));
        } else if (name == "offset") {
            writePoint(w, ruler.offset(point(op[1]), fromBits(op[2]), fromBits(op[3])));
        } else if (name == "lineDistance") {
            writeDouble(w, mutableRuler.lineDistance(line(op[1])));
        } else if (name == "area") {
            cr::polygon poly;
            for (const auto& ring : op[1].GetArray()) poly.push_back(line(ring));
            writeDouble(w, ruler.area(poly));
        } else if (name == "along") {
            writePoint(w, ruler.along(line(op[1]), fromBits(op[2])));
        } else if (name == "pointToSegmentDistance") {
            writeDouble(w, ruler.pointToSegmentDistance(point(op[1]), point(op[2]), point(op[3])));
        } else if (name == "pointOnLine") {
            const auto t = ruler.pointOnLine(line(op[1]), point(op[2]));
            w.StartArray();
            writePoint(w, std::get<0>(t));
            w.Uint(std::get<1>(t));
            writeDouble(w, std::get<2>(t));
            w.EndArray();
        } else if (name == "lineSlice") {
            writeLine(w, ruler.lineSlice(point(op[1]), point(op[2]), line(op[3])));
        } else if (name == "lineSliceAlong") {
            writeLine(w, ruler.lineSliceAlong(fromBits(op[1]), fromBits(op[2]), line(op[3])));
        } else if (name == "bufferPoint") {
            writeBox(w, ruler.bufferPoint(point(op[1]), fromBits(op[2])));
        } else if (name == "bufferBBox") {
            writeBox(w, ruler.bufferBBox(box(op[1]), fromBits(op[2])));
        } else if (name == "insideBBox") {
            w.Bool(cr::CheapRuler::insideBBox(point(op[1]), box(op[2])));
        } else if (name == "interpolate") {
            writePoint(w, cr::CheapRuler::interpolate(point(op[1]), point(op[2]), fromBits(op[3])));
        } else if (name == "remainder") {
            writeDouble(w, std::remainder(fromBits(op[1]), fromBits(op[2])));
        } else {
            throw std::runtime_error("unknown op " + name);
        }
    }
    w.EndArray();
    return result(buffer);
}

// Request: {"w", "h", "auto_resize", "ops": [["packOne", id, w, h] |
// ["pack", [[id, w, h]...], inPlace] | ["ref", k] | ["unref", k] |
// ["getBin", id] | ["shrink"] | ["clear"] | ["resize", w, h]...]}. Bins
// returned by packOne/pack/getBin are remembered as handles; ref/unref take
// handles[k % size]. Result per op: {"r": result, "w": width, "h": height,
// "bins": [every handle's bin]} (no "bins" once clear() left them
// dangling).
extern "C" char* ref_shelf_pack(const char* text, std::size_t len) {
    Document d = parseRequest(text, len);
    mapbox::ShelfPack::ShelfPackOptions options;
    options.autoResize = d["auto_resize"].GetBool();
    mapbox::ShelfPack sprite(d["w"].GetInt(), d["h"].GetInt(), options);
    std::vector<mapbox::Bin*> handles;

    rapidjson::StringBuffer buffer;
    Writer w(buffer);
    w.StartArray();
    for (const auto& op : d["ops"].GetArray()) {
        const std::string name = op[0].GetString();
        w.StartObject();
        w.Key("r");
        if (name == "packOne") {
            mapbox::Bin* bin = sprite.packOne(op[1].GetInt(), op[2].GetInt(), op[3].GetInt());
            writeBin(w, bin);
            if (bin) handles.push_back(bin);
        } else if (name == "pack") {
            std::vector<mapbox::Bin> bins;
            for (const auto& b : op[1].GetArray()) bins.emplace_back(b[0].GetInt(), b[1].GetInt(), b[2].GetInt());
            mapbox::ShelfPack::PackOptions packOptions;
            packOptions.inPlace = op[2].GetBool();
            const auto results = sprite.pack(bins, packOptions);
            w.StartObject();
            w.Key("results");
            w.StartArray();
            for (auto* bin : results) {
                writeBin(w, bin);
                handles.push_back(bin);
            }
            w.EndArray();
            w.Key("bins");
            w.StartArray();
            for (const auto& bin : bins) writeBin(w, &bin);
            w.EndArray();
            w.EndObject();
        } else if (name == "ref" || name == "unref") {
            if (handles.empty()) {
                w.Null();
            } else {
                mapbox::Bin& bin = *handles[op[1].GetUint() % handles.size()];
                w.Int(name == "ref" ? sprite.ref(bin) : sprite.unref(bin));
            }
        } else if (name == "getBin") {
            mapbox::Bin* bin = sprite.getBin(op[1].GetInt());
            writeBin(w, bin);
            if (bin) handles.push_back(bin);
        } else if (name == "shrink") {
            sprite.shrink();
            w.Null();
        } else if (name == "clear") {
            sprite.clear();
            handles.clear();
            w.Null();
        } else if (name == "resize") {
            w.Bool(sprite.resize(op[1].GetInt(), op[2].GetInt()));
        } else {
            throw std::runtime_error("unknown op " + name);
        }
        w.Key("w");
        w.Int(sprite.width());
        w.Key("h");
        w.Int(sprite.height());
        w.Key("bins");
        w.StartArray();
        for (auto* bin : handles) writeBin(w, bin);
        w.EndArray();
        w.EndObject();
    }
    w.EndArray();
    return result(buffer);
}
