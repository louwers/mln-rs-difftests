// The C++ oracle for //difftest/sources, as a process (the tile pyramid and
// the GeoJSON source pull in the thread pool and the Linux platform, which
// link completely only into C++ executables). sources_difftest.rs writes one
// request per line and reads one response line per request:
//
//   {"cmd": "source", "id": "<id>", "json": "<source JSON>"}
//     -> the source conversion: "E <error>" or the GeoJSON source's
//        options and URL (see dumpSource).
//   {"cmd": "tiles", "source": "<source JSON>", "size": [w, h],
//    "camera": {"lng", "lat", "zoom", "bearing", "pitch"}, "mode": "static" | "continuous"}
//     -> the tiles a TilePyramid creates and renders for that view: the
//        first update with loading tiles, then again with all tiles loaded.
//   {"cmd": "features", "source": "<source JSON>", "tiles": [[z, x, y], ...]}
//     -> per tile, the features GeoJSONTileData hands to the worker: type,
//        id, properties and geometries (after fixupPolygons).
//
// The text format is shared with dump_* in sources_difftest.rs.

#include <mln/map/transform.hpp>
#include <mln/renderer/tile_parameters.hpp>
#include <mln/renderer/tile_pyramid.hpp>
#include <mln/renderer/tile_render_data.hpp>
#include <mln/style/conversion/source.hpp>
#include <mln/style/rapidjson_conversion.hpp>
#include <mln/style/sources/geojson_source.hpp>
#include <mln/style/sources/geojson_source_impl.hpp>
#include <mln/tile/geojson_tile_data.hpp>
#include <mln/tile/tile.hpp>
#include <mln/util/constants.hpp>
#include <mln/util/run_loop.hpp>

#include <rapidjson/document.h>

#include <algorithm>
#include <bit>
#include <cstdint>
#include <iostream>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

using namespace mln;
using namespace mln::style;

namespace {

std::string hexBits(double d) {
    std::ostringstream ss;
    ss << std::hex << std::bit_cast<uint64_t>(d);
    return ss.str();
}

std::string str(const std::string& s) {
    return "s" + std::to_string(s.size()) + ":" + s;
}

std::string dumpValue(const mln::Value& value);

struct ValueDumper {
    std::string operator()(const mapbox::feature::null_value_t&) const { return "null"; }
    std::string operator()(bool b) const { return b ? "true" : "false"; }
    std::string operator()(uint64_t u) const { return "u" + std::to_string(u); }
    std::string operator()(int64_t i) const { return "i" + std::to_string(i); }
    std::string operator()(double d) const { return "d" + hexBits(d); }
    std::string operator()(const std::string& s) const { return str(s); }
    std::string operator()(const std::vector<mln::Value>& array) const {
        std::string out = "[";
        for (std::size_t i = 0; i < array.size(); ++i) {
            if (i) out += ",";
            out += dumpValue(array[i]);
        }
        return out + "]";
    }
    std::string operator()(const std::unordered_map<std::string, mln::Value>& object) const {
        std::vector<std::string> keys;
        for (const auto& entry : object) keys.push_back(entry.first);
        std::sort(keys.begin(), keys.end());
        std::string out = "{";
        bool first = true;
        for (const auto& key : keys) {
            if (!first) out += ",";
            first = false;
            out += str(key) + "=" + dumpValue(object.at(key));
        }
        return out + "}";
    }
};

std::string dumpValue(const mln::Value& value) {
    return mapbox::util::apply_visitor(ValueDumper{}, value);
}

struct IdentifierDumper {
    std::string operator()(const mapbox::feature::null_value_t&) const { return "null"; }
    std::string operator()(uint64_t u) const { return "u" + std::to_string(u); }
    std::string operator()(int64_t i) const { return "i" + std::to_string(i); }
    std::string operator()(double d) const { return "d" + hexBits(d); }
    std::string operator()(const std::string& s) const { return str(s); }
};

std::string dumpOptions(const GeoJSONOptions& o) {
    std::ostringstream ss;
    ss << "minzoom=" << int(o.minzoom) << " maxzoom=" << int(o.maxzoom) << " tileSize=" << o.tileSize
       << " buffer=" << o.buffer << " tolerance=" << hexBits(o.tolerance) << " lineMetrics=" << o.lineMetrics
       << " cluster=" << o.cluster << " clusterRadius=" << o.clusterRadius
       << " clusterMaxZoom=" << int(o.clusterMaxZoom) << " clusterMinPoints=" << o.clusterMinPoints
       << " synchronousUpdate=" << o.synchronousUpdate << " clusterProperties=[";
    bool first = true;
    for (const auto& p : o.clusterProperties) {
        if (!first) ss << ",";
        first = false;
        ss << str(p.first);
    }
    ss << "]";
    return ss.str();
}

std::unique_ptr<Source> convertSource(const std::string& json, const std::string& id, std::string& error) {
    JSDocument document;
    document.Parse<0>(json.c_str());
    if (document.HasParseError()) {
        error = "bad JSON";
        return nullptr;
    }
    conversion::Error err;
    const JSValue* value = &document;
    try {
        std::optional<std::unique_ptr<Source>> converted = conversion::convert<std::unique_ptr<Source>>(
            conversion::Convertible(value), err, id);
        if (!converted) {
            error = err.message;
            return nullptr;
        }
        return std::move(*converted);
    } catch (const std::exception& e) {
        error = std::string("exception: ") + e.what();
        return nullptr;
    }
}

std::string dumpSource(const std::string& json, const std::string& id) {
    std::string error;
    auto source = convertSource(json, id, error);
    if (!source) {
        return "E " + error;
    }
    auto* geojson = source->as<GeoJSONSource>();
    if (!geojson) {
        return "T " + std::to_string(int(source->getType()));
    }
    std::string out = "G id=" + str(source->getID()) + " " + dumpOptions(geojson->getOptions());
    const auto url = geojson->getURL();
    out += " url=" + (url ? str(*url) : std::string("none"));
    out += " data=" + std::string(geojson->impl().getData().lock() ? "yes" : "no");
    out += " sync=" + std::to_string(geojson->isUpdateSynchronous());
    return out;
}

class FakeTile final : public Tile {
public:
    FakeTile(const OverscaledTileID& id_, TileObserver* observer_)
        : Tile(Kind::Geometry, id_, "source", observer_) {}

    std::unique_ptr<TileRenderData> createRenderData() override { return nullptr; }
    void cancel() override {}
    bool layerPropertiesUpdated(const Immutable<style::LayerProperties>&) override { return false; }

    void complete() {
        loaded = true;
        renderable = true;
        pending = false;
    }
};

std::string tileString(const OverscaledTileID& id) {
    return std::to_string(int(id.overscaledZ)) + "/" + std::to_string(id.wrap) + "/" +
           std::to_string(int(id.canonical.z)) + "/" + std::to_string(id.canonical.x) + "/" +
           std::to_string(id.canonical.y);
}

std::string unwrappedString(const UnwrappedTileID& id) {
    return std::to_string(id.wrap) + "/" + std::to_string(int(id.canonical.z)) + "/" +
           std::to_string(id.canonical.x) + "/" + std::to_string(id.canonical.y);
}

std::string dumpTiles(const rapidjson::Value& request) {
    std::string error;
    const std::string json(request["source"].GetString(), request["source"].GetStringLength());
    auto source = convertSource(json, "source", error);
    if (!source) return "E " + error;
    auto* geojson = source->as<GeoJSONSource>();
    if (!geojson) return "E not geojson";
    const auto& impl = geojson->impl();

    Transform transform;
    transform.resize(Size(request["size"][0].GetUint(), request["size"][1].GetUint()));
    const auto& camera = request["camera"];
    CameraOptions cameraOptions;
    cameraOptions.center = LatLng(camera["lat"].GetDouble(), camera["lng"].GetDouble());
    cameraOptions.zoom = camera["zoom"].GetDouble();
    cameraOptions.bearing = camera["bearing"].GetDouble();
    cameraOptions.pitch = camera["pitch"].GetDouble();
    transform.jumpTo(cameraOptions);

    const bool continuous = std::string(request["mode"].GetString()) == "continuous";
    const util::SimpleIdentity uniqueID;
    TileParameters parameters{.pixelRatio = 1.0f,
                              .debugOptions = MapDebugOptions::NoDebug,
                              .transformState = transform.getState(),
                              .fileSource = nullptr,
                              .mode = continuous ? MapMode::Continuous : MapMode::Static,
                              .annotationManager = {},
                              .imageManager = nullptr,
                              .glyphManager = nullptr,
                              .prefetchZoomDelta = util::DEFAULT_PREFETCH_ZOOM_DELTA,
                              .threadPool = TaggedScheduler{Scheduler::GetBackground(), uniqueID}};

    std::string out;
    {
        TilePyramid pyramid(parameters.threadPool);
        std::vector<FakeTile*> created;
        auto createTile = [&](const OverscaledTileID& id, TileObserver* observer) -> std::unique_ptr<Tile> {
            auto tile = std::make_unique<FakeTile>(id, observer);
            created.push_back(tile.get());
            out += " C" + tileString(id);
            return tile;
        };
        std::vector<Immutable<LayerProperties>> layers;
        pyramid.update(layers,
                       true,
                       false,
                       parameters,
                       impl,
                       util::tileSize_I,
                       impl.getZoomRange(),
                       std::nullopt,
                       createTile);
        out += " |";
        for (const auto& entry : pyramid.getRenderedTiles()) out += " R" + unwrappedString(entry.first);
        out += " |";
        for (auto* tile : created) tile->complete();
        pyramid.update(layers,
                       true,
                       false,
                       parameters,
                       impl,
                       util::tileSize_I,
                       impl.getZoomRange(),
                       std::nullopt,
                       createTile);
        out += " |";
        for (const auto& entry : pyramid.getRenderedTiles()) {
            out += " R" + unwrappedString(entry.first) + "=" + tileString(entry.second.get().id);
        }
        out += " |";
        for (const auto& entry : pyramid.getTiles()) out += " T" + tileString(entry.first);
        out += " | loaded=" + std::to_string(pyramid.isLoaded());
    }
    return "OK" + out;
}

std::string dumpFeatures(const rapidjson::Value& request) {
    std::string error;
    const std::string json(request["source"].GetString(), request["source"].GetStringLength());
    auto source = convertSource(json, "source", error);
    if (!source) return "E " + error;
    auto* geojson = source->as<GeoJSONSource>();
    if (!geojson) return "E not geojson";
    auto data = geojson->impl().getData().lock();
    if (!data) return "E no data";

    std::string out = "OK";
    for (const auto& t : request["tiles"].GetArray()) {
        const CanonicalTileID id(static_cast<uint8_t>(t[0].GetUint()), t[1].GetUint(), t[2].GetUint());
        out += " TILE " + std::to_string(int(id.z)) + "/" + std::to_string(id.x) + "/" + std::to_string(id.y);
        try {
            data->getTile(
                id,
                [&](GeoJSONData::TileFeatures features) {
                    GeoJSONTileData tileData(std::move(features));
                    auto layer = tileData.getLayer({});
                    out += " n=" + std::to_string(layer->featureCount()) + " name=" + str(layer->getName());
                    for (std::size_t i = 0; i < layer->featureCount(); ++i) {
                        auto feature = layer->getFeature(i);
                        out += " F type=" + std::to_string(int(feature->getType()));
                        out += " id=" + mapbox::util::apply_visitor(IdentifierDumper{}, feature->getID());
                        out += " props=" + ValueDumper{}(feature->getProperties());
                        try {
                            const auto& geometries = feature->getGeometries();
                            out += " geom=[";
                            for (const auto& ring : geometries) {
                                out += "(";
                                for (const auto& p : ring) {
                                    out += std::to_string(p.x) + "," + std::to_string(p.y) + ";";
                                }
                                out += ")";
                            }
                            out += "]";
                        } catch (const std::exception& e) {
                            out += std::string(" geomerror=") + e.what();
                        }
                    }
                },
                true);
        } catch (const std::exception& e) {
            out += std::string(" error=") + e.what();
        }
    }
    return out;
}

} // namespace

int main() {
    // No std::ios::sync_with_stdio(false): the core logs to std::cerr from
    // other threads, which flushes the tied std::cout, and unsynchronized
    // streams are not thread-safe (see difftest/layers/oracle_main.cpp).
    util::RunLoop loop;
    std::string line;
    while (std::getline(std::cin, line)) {
        rapidjson::Document request;
        request.Parse(line.c_str(), line.size());
        if (request.HasParseError() || !request.IsObject()) {
            std::cout << "bad request\n" << std::flush;
            continue;
        }
        const std::string cmd = request["cmd"].GetString();
        std::string response;
        if (cmd == "source") {
            response = dumpSource(std::string(request["json"].GetString(), request["json"].GetStringLength()),
                                  request["id"].GetString());
        } else if (cmd == "tiles") {
            response = dumpTiles(request);
        } else if (cmd == "features") {
            response = dumpFeatures(request);
        } else {
            response = "bad command";
        }
        // One line per response.
        std::replace(response.begin(), response.end(), '\n', ' ');
        std::cout << response << "\n" << std::flush;
    }
    return 0;
}
