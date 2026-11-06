// extern "C" wrappers around the C++ tile decoding for //difftest/tile:
// mln::VectorMVTTileData (and the mapbox::vector_tile layer/feature under
// it), the geometry_tile_data.cpp functions, and wagyu.
//
// Results are written as a line-based text dump; byte strings are written
// as `x<hex>` so the Rust side can normalize them (UTF-8-lossy) before
// comparing. The Rust port writes the same dump from its decoding.

#include <mln/tile/geometry_tile_data.hpp>
#include <mln/tile/tile_id.hpp>
#include <mln/tile/vector_mvt_tile_data.hpp>
#include <mln/util/logging.hpp>

#include <mapbox/geometry/wagyu/wagyu.hpp>
#include <mapbox/vector_tile.hpp>
#include <protozero/pbf_reader.hpp>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

using namespace mln;

std::string exceptionKind(std::exception_ptr e, std::string& what);

namespace {

thread_local std::string tileLog;

class CaptureObserver : public Log::Observer {
public:
    bool onRecord(EventSeverity severity, Event event, int64_t, const std::string& msg) override {
        if (event == Event::ParseTile) {
            tileLog += "log ";
            tileLog += std::to_string(static_cast<int>(severity));
            tileLog += " x";
            static const char* digits = "0123456789abcdef";
            for (unsigned char c : msg) {
                tileLog += digits[c >> 4];
                tileLog += digits[c & 15];
            }
            tileLog += "\n";
            return true;
        }
        return false;
    }
};

void installObserver() {
    static bool installed = [] {
        Log::setObserver(std::make_unique<CaptureObserver>());
        return true;
    }();
    (void)installed;
}

void hex(std::string& out, const std::string& s) {
    static const char* digits = "0123456789abcdef";
    out += 'x';
    for (unsigned char c : s) {
        out += digits[c >> 4];
        out += digits[c & 15];
    }
}

void bits(std::string& out, double d) {
    uint64_t b;
    std::memcpy(&b, &d, sizeof b);
    char buf[24];
    std::snprintf(buf, sizeof buf, "%016llx", static_cast<unsigned long long>(b));
    out += buf;
}

void value(std::string& out, const Value& v) {
    v.match([&](const NullValue&) { out += "n"; },
            [&](bool b) { out += b ? "b1" : "b0"; },
            [&](uint64_t u) { out += "u" + std::to_string(u); },
            [&](int64_t i) { out += "i" + std::to_string(i); },
            [&](double d) {
                out += "d";
                bits(out, d);
            },
            [&](const std::string& s) {
                out += "s";
                hex(out, s);
            },
            [&](const auto&) { out += "?"; });
}

void identifier(std::string& out, const FeatureIdentifier& id) {
    id.match([&](const NullValue&) { out += "n"; },
             [&](uint64_t u) { out += "u" + std::to_string(u); },
             [&](int64_t i) { out += "i" + std::to_string(i); },
             [&](double d) {
                 out += "d";
                 bits(out, d);
             },
             [&](const std::string& s) {
                 out += "s";
                 hex(out, s);
             });
}

void rings(std::string& out, const GeometryCollection& g) {
    out += std::to_string(g.size());
    out += "\n";
    for (const auto& r : g) {
        out += "r " + std::to_string(r.size());
        for (const auto& p : r) {
            out += " " + std::to_string(p.x) + " " + std::to_string(p.y);
        }
        out += "\n";
    }
}

// The exception, as "<prefix>! <kind> x<what>\n".
void exception(std::string& out, const std::string& prefix, std::exception_ptr e) {
    std::string what;
    out += prefix + "! " + exceptionKind(e, what) + " ";
    hex(out, what);
    out += "\n";
}

char* finish(const std::string& out, size_t* len) {
    char* result = static_cast<char*>(std::malloc(out.size() + 1));
    std::memcpy(result, out.data(), out.size());
    result[out.size()] = 0;
    *len = out.size();
    return result;
}

const char* const extraKeys[] = {"name", "type", "class", "zzz-missing", ""};

void dumpFeature(std::string& out,
                 const GeometryTileLayer& layer,
                 const mapbox::vector_tile::layer* rawLayer,
                 std::size_t i) {
    out += "f " + std::to_string(i) + "\n";
    std::unique_ptr<GeometryTileFeature> feature;
    try {
        feature = layer.getFeature(i);
    } catch (...) {
        exception(out, "f", std::current_exception());
        return;
    }
    out += "type " + std::to_string(static_cast<int>(feature->getType())) + "\n";
    out += "id ";
    identifier(out, feature->getID());
    out += "\n";

    std::vector<std::string> keys;
    try {
        const PropertyMap& props = feature->getProperties();
        std::vector<std::pair<std::string, Value>> sorted(props.begin(), props.end());
        std::sort(sorted.begin(), sorted.end(), [](const auto& a, const auto& b) { return a.first < b.first; });
        out += "props " + std::to_string(sorted.size()) + "\n";
        for (const auto& [k, v] : sorted) {
            out += "p ";
            hex(out, k);
            out += " ";
            value(out, v);
            out += "\n";
            keys.push_back(k);
        }
    } catch (...) {
        exception(out, "props", std::current_exception());
    }
    for (const char* k : extraKeys) {
        keys.emplace_back(k);
    }
    for (const auto& k : keys) {
        try {
            auto v = feature->getValue(k);
            out += "v ";
            hex(out, k);
            out += " ";
            if (v) {
                value(out, *v);
            } else {
                out += "none";
            }
            out += "\n";
        } catch (...) {
            std::string prefix = "v ";
            hex(prefix, k);
            exception(out, prefix, std::current_exception());
        }
    }

    if (rawLayer) {
        try {
            mapbox::vector_tile::feature raw(rawLayer->getFeature(i), *rawLayer);
            auto g = raw.getGeometries<GeometryCollection>(1.0f);
            out += "raw_geom ";
            rings(out, g);
        } catch (...) {
            exception(out, "raw_geom", std::current_exception());
        }
    }

    tileLog.clear();
    try {
        const GeometryCollection& g = feature->getGeometries();
        out += tileLog;
        out += "geom ";
        rings(out, g);
    } catch (...) {
        out += tileLog;
        exception(out, "geom", std::current_exception());
        // A second call: retried, or the lines cached before fixupPolygons threw.
        try {
            const GeometryCollection& g = feature->getGeometries();
            out += "geom2 ";
            rings(out, g);
        } catch (...) {
            exception(out, "geom2", std::current_exception());
        }
    }
}

} // namespace

extern "C" {

int32_t mln_tile_ref_ndebug() {
#ifdef NDEBUG
    return 1;
#else
    return 0;
#endif
}

void mln_tile_ref_free(char* p) {
    std::free(p);
}

// Decodes a tile with VectorMVTTileData and dumps everything.
char* mln_tile_ref_decode(const uint8_t* data, size_t size, size_t* out_len) {
    installObserver();
    std::string out;
    auto bytes = std::make_shared<const std::string>(reinterpret_cast<const char*>(data), size);
    VectorMVTTileData tile(bytes);

    std::vector<std::string> names;
    try {
        names = tile.layerNames();
    } catch (...) {
        exception(out, "names", std::current_exception());
        return finish(out, out_len);
    }
    out += "names " + std::to_string(names.size()) + "\n";

    std::map<std::string, const protozero::data_view> rawLayers;
    try {
        rawLayers = mapbox::vector_tile::buffer(*bytes).getLayers();
    } catch (...) {
        exception(out, "raw_buffer", std::current_exception());
    }

    for (const auto& name : names) {
        out += "layer ";
        hex(out, name);
        out += "\n";
        std::unique_ptr<GeometryTileLayer> layer;
        try {
            layer = tile.getLayer(name);
        } catch (...) {
            exception(out, "layer", std::current_exception());
            continue;
        }
        if (!layer) {
            out += "nolayer\n";
            continue;
        }
        out += "info ";
        hex(out, layer->getName());
        out += " " + std::to_string(layer->featureCount()) + "\n";

        std::unique_ptr<mapbox::vector_tile::layer> rawLayer;
        auto it = rawLayers.find(name);
        if (it != rawLayers.end()) {
            try {
                rawLayer = std::make_unique<mapbox::vector_tile::layer>(it->second);
                out += "raw " + std::to_string(rawLayer->getExtent()) + " " + std::to_string(rawLayer->getVersion()) +
                       "\n";
            } catch (...) {
                exception(out, "raw", std::current_exception());
            }
        }

        for (std::size_t i = 0; i <= layer->featureCount(); ++i) {
            dumpFeature(out, *layer, rawLayer.get(), i);
        }
    }

    try {
        auto missing = tile.getLayer("zzz-missing-layer");
        out += missing ? "missing found\n" : "missing none\n";
    } catch (...) {
        exception(out, "missing", std::current_exception());
    }
    return finish(out, out_len);
}

// GeometryCollection from (coords, ring lengths).
static GeometryCollection collection(const int16_t* coords, const uint32_t* ring_lens, size_t nrings) {
    GeometryCollection g;
    size_t k = 0;
    for (size_t r = 0; r < nrings; ++r) {
        GeometryCoordinates ring;
        for (uint32_t i = 0; i < ring_lens[r]; ++i, k += 2) {
            ring.emplace_back(coords[k], coords[k + 1]);
        }
        g.push_back(std::move(ring));
    }
    return g;
}

char* mln_tile_ref_fixup_polygons(const int16_t* coords, const uint32_t* ring_lens, size_t nrings, size_t* out_len) {
    std::string out;
    try {
        auto g = fixupPolygons(collection(coords, ring_lens, nrings));
        out += "geom ";
        rings(out, g);
    } catch (...) {
        exception(out, "geom", std::current_exception());
    }
    return finish(out, out_len);
}

char* mln_tile_ref_classify_rings(const int16_t* coords, const uint32_t* ring_lens, size_t nrings, size_t* out_len) {
    std::string out;
    auto polygons = classifyRings(collection(coords, ring_lens, nrings));
    out += "polygons " + std::to_string(polygons.size()) + "\n";
    for (const auto& p : polygons) {
        out += "geom ";
        rings(out, p);
    }
    return finish(out, out_len);
}

char* mln_tile_ref_limit_holes(
    const int16_t* coords, const uint32_t* ring_lens, size_t nrings, uint32_t max_holes, size_t* out_len) {
    std::string out;
    auto g = collection(coords, ring_lens, nrings);
    limitHoles(g, max_holes);
    out += "geom ";
    rings(out, g);
    return finish(out, out_len);
}

// wagyu<int32_t> on int32 rings; output as int32 (fixupPolygons uses int16).
char* mln_tile_ref_wagyu(const int32_t* coords,
                         const uint32_t* ring_lens,
                         const uint8_t* ring_types,
                         size_t nrings,
                         uint8_t clip_type_,
                         uint8_t subject_fill,
                         uint8_t clip_fill,
                         uint8_t reverse,
                         size_t* out_len) {
    using namespace mapbox::geometry::wagyu;
    std::string out;
    try {
        wagyu<int32_t> clipper;
        clipper.reverse_rings(reverse != 0);
        size_t k = 0;
        std::string added;
        for (size_t r = 0; r < nrings; ++r) {
            mapbox::geometry::linear_ring<int32_t> ring;
            for (uint32_t i = 0; i < ring_lens[r]; ++i, k += 2) {
                ring.emplace_back(coords[k], coords[k + 1]);
            }
            added += clipper.add_ring(ring, static_cast<polygon_type>(ring_types[r])) ? '1' : '0';
        }
        out += "added " + added + "\n";
        mapbox::geometry::multi_polygon<int32_t> solution;
        bool ok = clipper.execute(static_cast<clip_type>(clip_type_),
                                  solution,
                                  static_cast<fill_type>(subject_fill),
                                  static_cast<fill_type>(clip_fill));
        out += ok ? "ok " : "empty ";
        out += std::to_string(solution.size()) + "\n";
        for (const auto& polygon : solution) {
            out += "polygon " + std::to_string(polygon.size()) + "\n";
            for (const auto& ring : polygon) {
                out += "r " + std::to_string(ring.size());
                for (const auto& p : ring) {
                    out += " " + std::to_string(p.x) + " " + std::to_string(p.y);
                }
                out += "\n";
            }
        }
    } catch (...) {
        exception(out, "wagyu", std::current_exception());
    }
    return finish(out, out_len);
}

namespace {
class TestFeature : public GeometryTileFeature {
public:
    FeatureType type;
    GeometryCollection geometries;
    FeatureType getType() const override { return type; }
    std::optional<Value> getValue(const std::string&) const override { return std::nullopt; }
    const GeometryCollection& getGeometries() const override { return geometries; }
};

void latlng(std::string& out, const Point<double>& p) {
    out += " ";
    bits(out, p.x);
    out += " ";
    bits(out, p.y);
}

void dumpGeometry(std::string& out, const Feature::geometry_type& g) {
    g.match(
        [&](const Point<double>& p) {
            out += "point";
            latlng(out, p);
            out += "\n";
        },
        [&](const MultiPoint<double>& ps) {
            out += "multipoint " + std::to_string(ps.size());
            for (const auto& p : ps) latlng(out, p);
            out += "\n";
        },
        [&](const LineString<double>& ps) {
            out += "linestring " + std::to_string(ps.size());
            for (const auto& p : ps) latlng(out, p);
            out += "\n";
        },
        [&](const MultiLineString<double>& ls) {
            out += "multilinestring " + std::to_string(ls.size()) + "\n";
            for (const auto& l : ls) {
                out += "l " + std::to_string(l.size());
                for (const auto& p : l) latlng(out, p);
                out += "\n";
            }
        },
        [&](const Polygon<double>& pg) {
            out += "polygon " + std::to_string(pg.size()) + "\n";
            for (const auto& l : pg) {
                out += "l " + std::to_string(l.size());
                for (const auto& p : l) latlng(out, p);
                out += "\n";
            }
        },
        [&](const MultiPolygon<double>& mp) {
            out += "multipolygon " + std::to_string(mp.size()) + "\n";
            for (const auto& pg : mp) {
                out += "pg " + std::to_string(pg.size()) + "\n";
                for (const auto& l : pg) {
                    out += "l " + std::to_string(l.size());
                    for (const auto& p : l) latlng(out, p);
                    out += "\n";
                }
            }
        },
        [&](const auto&) { out += "other\n"; });
}
} // namespace

// convertGeometry(GeometryTileFeature, CanonicalTileID); type 0 (Unknown) is
// not allowed (the C++ asserts).
char* mln_tile_ref_convert_geometry(uint8_t type,
                                    const int16_t* coords,
                                    const uint32_t* ring_lens,
                                    size_t nrings,
                                    uint8_t z,
                                    uint32_t x,
                                    uint32_t y,
                                    size_t* out_len) {
    std::string out;
    TestFeature feature;
    feature.type = static_cast<FeatureType>(type);
    feature.geometries = collection(coords, ring_lens, nrings);
    try {
        dumpGeometry(out, convertGeometry(feature, CanonicalTileID(z, x, y)));
    } catch (...) {
        exception(out, "convert", std::current_exception());
    }
    return finish(out, out_len);
}

// convertGeometry(Feature::geometry_type, CanonicalTileID) for a
// multi-linestring (kind 0), point (1), multipoint (2), linestring (3),
// polygon (4) or multipolygon of one polygon (5) of the given lng/lat rings.
char* mln_tile_ref_convert_geometry_to_tile(uint8_t kind,
                                            const double* coords,
                                            const uint32_t* ring_lens,
                                            size_t nrings,
                                            uint8_t z,
                                            uint32_t x,
                                            uint32_t y,
                                            size_t* out_len) {
    std::string out;
    std::vector<LineString<double>> lines;
    size_t k = 0;
    for (size_t r = 0; r < nrings; ++r) {
        LineString<double> line;
        for (uint32_t i = 0; i < ring_lens[r]; ++i, k += 2) {
            line.emplace_back(coords[k], coords[k + 1]);
        }
        lines.push_back(std::move(line));
    }
    Feature::geometry_type g;
    switch (kind) {
        case 1:
            g = lines.at(0).at(0);
            break;
        case 2:
            g = MultiPoint<double>(lines.at(0).begin(), lines.at(0).end());
            break;
        case 3:
            g = lines.at(0);
            break;
        case 4: {
            Polygon<double> pg;
            for (auto& l : lines) pg.emplace_back(l.begin(), l.end());
            g = pg;
            break;
        }
        case 5: {
            Polygon<double> pg;
            for (auto& l : lines) pg.emplace_back(l.begin(), l.end());
            g = MultiPolygon<double>{pg};
            break;
        }
        default:
            g = MultiLineString<double>(lines.begin(), lines.end());
    }
    out += "geom ";
    rings(out, convertGeometry(g, CanonicalTileID(z, x, y)));
    return finish(out, out_len);
}

} // extern "C"
