// The C++ side of //difftest/layers: builds buckets the way the tile worker
// does (style layer JSON -> RenderLayer evaluate -> LayerManager::createLayout
// -> Layout::createBucket), runs the polyline and fill generators and the
// line atlas, and reports the results as text that layers_difftest.rs
// produces from the Rust port the same way.
//
// The core here is built for the OpenGL drawable renderer (the C++ Bazel
// default), so its FillBucket has no triangulated outlines
// (MLN_TRIANGULATE_FILL_OUTLINES is 0); the "fillgen" request runs the
// generator overload the WebGPU FillBucket uses directly.

#include <mln/geometry/feature_index.hpp>
#include <mln/gfx/fill_generator.hpp>
#include <mln/gfx/polyline_generator.hpp>
#include <mln/layermanager/layer_manager.hpp>
#include <mln/layout/layout.hpp>
#include <mln/renderer/bucket_parameters.hpp>
#include <mln/renderer/buckets/circle_bucket.hpp>
#include <mln/renderer/buckets/fill_bucket.hpp>
#include <mln/renderer/buckets/line_bucket.hpp>
#include <mln/renderer/property_evaluation_parameters.hpp>
#include <mln/renderer/render_layer.hpp>
#include <mln/renderer/transition_parameters.hpp>
#include <mln/style/conversion/layer.hpp>
#include <mln/style/conversion_impl.hpp>
#include <mln/style/layer.hpp>
#include <mln/style/layer_impl.hpp>
#include <mln/style/rapidjson_conversion.hpp>
#include <mln/tile/geometry_tile_data.hpp>
#include <mln/util/rapidjson.hpp>

#include <mln/gfx/context.hpp>
#include <mln/gfx/texture2d.hpp>
#include <mln/util/image.hpp>

#include <bit>
#include <cstdint>
#include <cstdio>
#include <map>
#include <memory>
#include <set>
#include <sstream>
#include <string>
#include <variant>
#include <vector>

// The DashPatternTexture image is private; the oracle reads it. Everything
// line_atlas.hpp includes is included above, so only its classes change.
#define private public
#include <mln/geometry/line_atlas.hpp>
#undef private

using namespace mln;

namespace {

struct FeatureData {
    FeatureType type = FeatureType::Point;
    FeatureIdentifier id;
    PropertyMap properties;
    GeometryCollection geometry;
};

class StubFeature : public GeometryTileFeature {
public:
    explicit StubFeature(const FeatureData& data_)
        : data(data_) {}
    // The layer owns the data (GeometryCollection is not copyable).
    FeatureType getType() const override { return data.type; }
    FeatureIdentifier getID() const override { return data.id; }
    const PropertyMap& getProperties() const override { return data.properties; }
    const GeometryCollection& getGeometries() const override { return data.geometry; }
    std::optional<Value> getValue(const std::string& key) const override {
        const auto it = data.properties.find(key);
        return (it != data.properties.end()) ? it->second : std::optional<Value>();
    }

private:
    const FeatureData& data;
};

class StubLayer : public GeometryTileLayer {
public:
    explicit StubLayer(std::vector<FeatureData> features_)
        : features(std::move(features_)) {}
    std::size_t featureCount() const override { return features.size(); }
    std::unique_ptr<GeometryTileFeature> getFeature(std::size_t i) const override {
        return std::make_unique<StubFeature>(features[i]);
    }
    std::string getName() const override { return "layer"; }

private:
    std::vector<FeatureData> features;
};

std::string bits(float f) {
    char buf[16];
    std::snprintf(buf, sizeof buf, "%08x", std::bit_cast<uint32_t>(f));
    return buf;
}

std::string hex(const uint8_t* data, std::size_t n) {
    static const char* digits = "0123456789abcdef";
    std::string out;
    out.reserve(n * 2);
    for (std::size_t i = 0; i < n; ++i) {
        out.push_back(digits[data[i] >> 4]);
        out.push_back(digits[data[i] & 15]);
    }
    return out;
}

GeometryCollection parseGeometry(const JSValue& rings) {
    GeometryCollection geometry;
    for (const auto& ring : rings.GetArray()) {
        GeometryCoordinates coordinates;
        for (const auto& point : ring.GetArray()) {
            coordinates.emplace_back(static_cast<int16_t>(point[0].GetInt()), static_cast<int16_t>(point[1].GetInt()));
        }
        geometry.push_back(std::move(coordinates));
    }
    return geometry;
}

Value parseValue(const JSValue& v) {
    const std::string type = v[1].GetString();
    if (type == "d") return v[2].GetDouble();
    if (type == "i") return static_cast<int64_t>(v[2].GetInt64());
    if (type == "u") return static_cast<uint64_t>(v[2].GetUint64());
    if (type == "b") return v[2].GetBool();
    return std::string(v[2].GetString(), v[2].GetStringLength());
}

FeatureIdentifier parseId(const JSValue& v) {
    if (v.IsNull()) return NullValue();
    const std::string type = v[0].GetString();
    if (type == "d") return v[1].GetDouble();
    if (type == "i") return static_cast<int64_t>(v[1].GetInt64());
    if (type == "u") return static_cast<uint64_t>(v[1].GetUint64());
    return std::string(v[1].GetString(), v[1].GetStringLength());
}

std::vector<FeatureData> parseFeatures(const JSValue& array) {
    std::vector<FeatureData> features;
    for (const auto& f : array.GetArray()) {
        FeatureData data;
        data.type = static_cast<FeatureType>(f["type"].GetInt());
        data.id = parseId(f["id"]);
        for (const auto& p : f["props"].GetArray()) {
            data.properties[std::string(p[0].GetString(), p[0].GetStringLength())] = parseValue(p);
        }
        data.geometry = parseGeometry(f["geom"]);
        features.push_back(std::move(data));
    }
    return features;
}

void dumpSegments(std::ostringstream& out, const char* name, const SegmentVector& segments) {
    out << name;
    for (const auto& s : segments) {
        out << " " << s.vertexOffset << "," << s.indexOffset << "," << s.vertexLength << "," << s.indexLength << ","
            << bits(s.sortKey);
    }
    out << "\n";
}

void dumpIndexes(std::ostringstream& out, const char* name, const std::vector<uint16_t>& indexes) {
    out << name;
    for (auto i : indexes) out << " " << i;
    out << "\n";
}

template <class Binder>
void dumpBinder(std::ostringstream& out, const char* name, const Binder& binder, const std::vector<float>& zooms) {
    out << "binder " << name << " offset=";
    if (binder->vertexOffset == std::numeric_limits<std::size_t>::max()) {
        out << "none";
    } else {
        out << binder->vertexOffset;
    }
    out << " stats=";
    const auto max = binder->statistics.max();
    if constexpr (std::is_same_v<std::decay_t<decltype(max)>, std::optional<float>>) {
        if (max) {
            out << bits(*max);
        } else {
            out << "none";
        }
    } else {
        out << "none";
    }
    out << " interp=";
    for (float z : zooms) out << bits(std::get<0>(binder->interpolationFactor(z))) << "/";
    out << " count=" << binder->getVertexCount() << "\n";
}

template <class Binders>
void dumpInterleaved(std::ostringstream& out, const std::string& id, const Binders& binders) {
    const auto& buffer = binders.interleavedVertexBuffer;
    out << "binders " << id << " stride=" << buffer.stride << " count=" << buffer.vertexCount
        << " data=" << hex(buffer.sharedVertexVector->data(), buffer.sharedVertexVector->elements()) << "\n";
}

void dumpCircle(std::ostringstream& out, const CircleBucket& bucket, const std::vector<float>& zooms) {
    out << "kind circle\nvertices";
    for (const auto& v : bucket.vertices.vector()) out << " " << v.a1[0] << " " << v.a1[1];
    out << "\n";
    dumpIndexes(out, "triangles", bucket.triangles.vector());
    dumpSegments(out, "segments", bucket.segments);
    for (const auto& [id, binders] : bucket.paintPropertyBinders) {
        using namespace style;
        dumpInterleaved(out, id, binders);
        dumpBinder(out, "circle-blur", binders.template get<CircleBlur>(), zooms);
        dumpBinder(out, "circle-color", binders.template get<CircleColor>(), zooms);
        dumpBinder(out, "circle-opacity", binders.template get<CircleOpacity>(), zooms);
        dumpBinder(out, "circle-radius", binders.template get<CircleRadius>(), zooms);
        dumpBinder(out, "circle-stroke-color", binders.template get<CircleStrokeColor>(), zooms);
        dumpBinder(out, "circle-stroke-opacity", binders.template get<CircleStrokeOpacity>(), zooms);
        dumpBinder(out, "circle-stroke-width", binders.template get<CircleStrokeWidth>(), zooms);
    }
}

void dumpLineVertices(std::ostringstream& out, const char* name, const std::vector<LineLayoutVertex>& vertices) {
    out << name;
    for (const auto& v : vertices) {
        out << " " << v.a1[0] << " " << v.a1[1] << " " << int(v.a2[0]) << " " << int(v.a2[1]) << " " << int(v.a2[2])
            << " " << int(v.a2[3]);
    }
    out << "\n";
}

void dumpFill(std::ostringstream& out, const FillBucket& bucket, const std::vector<float>& zooms) {
    out << "kind fill\nvertices";
    for (const auto& v : bucket.vertices.vector()) out << " " << v.a1[0] << " " << v.a1[1];
    out << "\n";
    dumpIndexes(out, "triangles", bucket.triangles.vector());
    dumpSegments(out, "segments", bucket.triangleSegments);
    dumpIndexes(out, "basiclines", bucket.basicLines.vector());
    dumpSegments(out, "basicsegments", bucket.basicLineSegments);
    for (const auto& [id, binders] : bucket.paintPropertyBinders) {
        using namespace style;
        dumpInterleaved(out, id, binders);
        dumpBinder(out, "fill-color", binders.template get<FillColor>(), zooms);
        dumpBinder(out, "fill-opacity", binders.template get<FillOpacity>(), zooms);
        dumpBinder(out, "fill-outline-color", binders.template get<FillOutlineColor>(), zooms);
        dumpBinder(out, "fill-pattern", binders.template get<FillPattern>(), zooms);
    }
}

void dumpLine(std::ostringstream& out, const LineBucket& bucket, const std::vector<float>& zooms) {
    out << "kind line\n";
    dumpLineVertices(out, "vertices", bucket.vertices.vector());
    dumpIndexes(out, "triangles", bucket.triangles.vector());
    dumpSegments(out, "segments", bucket.segments);
    for (const auto& [id, binders] : bucket.paintPropertyBinders) {
        using namespace style;
        dumpInterleaved(out, id, binders);
        dumpBinder(out, "line-blur", binders.template get<LineBlur>(), zooms);
        dumpBinder(out, "line-color", binders.template get<LineColor>(), zooms);
        dumpBinder(out, "line-floor-width", binders.template get<LineFloorWidth>(), zooms);
        dumpBinder(out, "line-gap-width", binders.template get<LineGapWidth>(), zooms);
        dumpBinder(out, "line-offset", binders.template get<LineOffset>(), zooms);
        dumpBinder(out, "line-opacity", binders.template get<LineOpacity>(), zooms);
        dumpBinder(out, "line-pattern", binders.template get<LinePattern>(), zooms);
        dumpBinder(out, "line-width", binders.template get<LineWidth>(), zooms);
    }
}

std::string bucketReport(const JSValue& request) {
    std::ostringstream out;
    const float zoom = request["zoom"].GetFloat();
    std::vector<float> zooms;
    for (const auto& z : request["zooms"].GetArray()) zooms.push_back(z.GetFloat());

    std::vector<std::unique_ptr<RenderLayer>> renderLayers;
    std::vector<Immutable<style::LayerProperties>> group;
    for (const auto& layerJSON : request["layers"].GetArray()) {
        style::conversion::Error error;
        auto layer = style::conversion::convert<std::unique_ptr<style::Layer>>(
            style::conversion::Convertible(&layerJSON), error);
        if (!layer) {
            out << "layer-error " << error.message << "\n";
            return out.str();
        }
        auto renderLayer = LayerManager::get()->createRenderLayer((*layer)->baseImpl);
        if (!renderLayer) {
            out << "no-render-layer\n";
            return out.str();
        }
        renderLayer->transition(TransitionParameters{.now = TimePoint::max(), .transition = {}});
        renderLayer->evaluate(PropertyEvaluationParameters(zoom));
        group.push_back(renderLayer->evaluatedProperties);
        renderLayers.push_back(std::move(renderLayer));
    }

    const auto& tile = request["tile"];
    const OverscaledTileID tileID(static_cast<uint8_t>(request["oz"].GetUint()),
                                  0,
                                  static_cast<uint8_t>(tile[0].GetUint()),
                                  tile[1].GetUint(),
                                  tile[2].GetUint());
    const MapMode mode = std::string(request["mode"].GetString()) == "continuous" ? MapMode::Continuous
                                                                                     : MapMode::Static;
    const BucketParameters parameters{.tileID = tileID,
                                      .mode = mode,
                                      .pixelRatio = 1.0f,
                                      .layerType = group.front()->baseImpl->getTypeInfo(),
                                      .globalState = nullptr,
                                      .retainFeaturesById = false};
    GlyphDependencies glyphDependencies;
    ImageDependencies imageDependencies;
    const std::set<std::string> availableImages;
    const LayoutParameters layoutParameters{.bucketParameters = parameters,
                                            .fontFaces = nullptr,
                                            .glyphDependencies = glyphDependencies,
                                            .imageDependencies = imageDependencies,
                                            .availableImages = availableImages};
    auto layout = LayerManager::get()->createLayout(
        layoutParameters, std::make_unique<StubLayer>(parseFeatures(request["features"])), group);
    if (!layout) {
        out << "no-layout\n";
        return out.str();
    }
    out << "dependencies " << (layout->hasDependencies() ? 1 : 0) << "\n";
    out << "imagedeps " << imageDependencies.size() << "\n";

    auto featureIndex = std::make_unique<FeatureIndex>(std::unique_ptr<const GeometryTileData>());
    mln::unordered_map<std::string, LayerRenderData> renderData;
    try {
        layout->createBucket({}, featureIndex, renderData, false, false, tileID.canonical);
    } catch (const std::exception& e) {
        out << "exception " << e.what() << "\n";
        return out.str();
    }

    std::map<std::string, const LayerRenderData*> sorted;
    for (const auto& [id, data] : renderData) sorted[id] = &data;
    std::vector<const Bucket*> buckets;
    for (const auto& [id, data] : sorted) {
        const Bucket* bucket = data->bucket.get();
        std::size_t n = 0;
        while (n < buckets.size() && buckets[n] != bucket) ++n;
        if (n == buckets.size()) buckets.push_back(bucket);
        out << "layer " << id << " bucket " << n << " passes " << int(data->layerProperties->renderPasses) << "\n";
    }
    for (std::size_t n = 0; n < buckets.size(); ++n) {
        out << "bucket " << n << "\n";
        const auto type = group.front()->baseImpl->getTypeInfo()->type;
        if (std::string(type) == "circle") {
            dumpCircle(out, static_cast<const CircleBucket&>(*buckets[n]), zooms);
        } else if (std::string(type) == "fill") {
            dumpFill(out, static_cast<const FillBucket&>(*buckets[n]), zooms);
        } else if (std::string(type) == "line") {
            dumpLine(out, static_cast<const LineBucket&>(*buckets[n]), zooms);
        }
    }
    return out.str();
}

style::LineCapType parseCap(const std::string& s) {
    if (s == "round") return style::LineCapType::Round;
    if (s == "square") return style::LineCapType::Square;
    return style::LineCapType::Butt;
}

style::LineJoinType parseJoin(const std::string& s) {
    if (s == "round") return style::LineJoinType::Round;
    if (s == "bevel") return style::LineJoinType::Bevel;
    return style::LineJoinType::Miter;
}

std::string polylineReport(const JSValue& request) {
    gfx::VertexVector<LineLayoutVertex> vertices;
    gfx::IndexVector<gfx::Triangles> indexes;
    SegmentVector segments;
    gfx::PolylineGenerator<LineLayoutVertex, SegmentBase> generator(
        vertices,
        LineBucket::layoutVertex,
        segments,
        [](std::size_t vertexOffset, std::size_t indexOffset) -> SegmentBase {
            return SegmentBase(vertexOffset, indexOffset);
        },
        [](auto& seg) -> SegmentBase& { return seg; },
        indexes);
    gfx::PolylineGeneratorOptions options;
    options.type = static_cast<FeatureType>(request["type"].GetInt());
    options.joinType = parseJoin(request["join"].GetString());
    options.miterLimit = request["miterLimit"].GetFloat();
    options.beginCap = parseCap(request["beginCap"].GetString());
    options.endCap = parseCap(request["endCap"].GetString());
    options.roundLimit = request["roundLimit"].GetFloat();
    options.overscaling = request["overscaling"].GetUint();
    if (request["clip"].IsArray()) {
        const auto& clip = request["clip"];
        options.clipDistances = gfx::PolylineGeneratorDistances{
            clip[0].GetDouble(), clip[1].GetDouble(), clip[2].GetDouble()};
    }
    for (const auto& line : request["lines"].GetArray()) {
        GeometryCoordinates coordinates;
        for (const auto& point : line.GetArray()) {
            coordinates.emplace_back(static_cast<int16_t>(point[0].GetInt()), static_cast<int16_t>(point[1].GetInt()));
        }
        generator.generate(coordinates, options);
    }
    std::ostringstream out;
    dumpLineVertices(out, "vertices", vertices.vector());
    dumpIndexes(out, "indexes", indexes.vector());
    dumpSegments(out, "segments", segments);
    return out.str();
}

std::string fillgenReport(const JSValue& request) {
    gfx::VertexVector<FillLayoutVertex> fillVertices;
    gfx::IndexVector<gfx::Triangles> fillIndexes;
    SegmentVector fillSegments;
    gfx::VertexVector<LineLayoutVertex> lineVertices;
    gfx::IndexVector<gfx::Triangles> lineIndexes;
    SegmentVector lineSegments;
    gfx::IndexVector<gfx::Lines> basicLineIndexes;
    SegmentVector basicLineSegments;
    std::ostringstream out;
    try {
        for (const auto& geometry : request["geometries"].GetArray()) {
            gfx::generateFillAndOutineBuffers(parseGeometry(geometry),
                                              fillVertices,
                                              fillIndexes,
                                              fillSegments,
                                              lineVertices,
                                              lineIndexes,
                                              lineSegments,
                                              basicLineIndexes,
                                              basicLineSegments);
        }
    } catch (...) {
        out << "exception\n";
    }
    out << "vertices";
    for (const auto& v : fillVertices.vector()) out << " " << v.a1[0] << " " << v.a1[1];
    out << "\n";
    dumpIndexes(out, "triangles", fillIndexes.vector());
    dumpSegments(out, "segments", fillSegments);
    dumpLineVertices(out, "linevertices", lineVertices.vector());
    dumpIndexes(out, "lineindexes", lineIndexes.vector());
    dumpSegments(out, "linesegments", lineSegments);
    dumpIndexes(out, "basiclines", basicLineIndexes.vector());
    dumpSegments(out, "basicsegments", basicLineSegments);
    return out.str();
}

std::string dashReport(const JSValue& request) {
    std::vector<float> from;
    std::vector<float> to;
    for (const auto& v : request["from"].GetArray()) from.push_back(v.GetFloat());
    for (const auto& v : request["to"].GetArray()) to.push_back(v.GetFloat());
    const auto cap = std::string(request["cap"].GetString()) == "round" ? LinePatternCap::Round
                                                                         : LinePatternCap::Square;
    DashPatternTexture texture(from, to, cap);
    std::ostringstream out;
    const auto& a = texture.getFrom();
    const auto& b = texture.getTo();
    out << "from " << bits(a.width) << " " << bits(a.height) << " " << bits(a.y) << "\n";
    out << "to " << bits(b.width) << " " << bits(b.height) << " " << bits(b.y) << "\n";
    const auto& image = std::get<AlphaImage>(texture.texture);
    out << "size " << image.size.width << "x" << image.size.height << "\n";
    out << "image " << hex(image.data.get(), image.bytes()) << "\n";
    return out.str();
}

} // namespace

std::string layersReport(const std::string& json) {
    JSDocument document;
    document.Parse<0>(json.c_str(), json.size());
    if (document.HasParseError()) return "bad request\n";
    const std::string kind = document["kind"].GetString();
    try {
        if (kind == "bucket") return bucketReport(document);
        if (kind == "polyline") return polylineReport(document);
        if (kind == "fillgen") return fillgenReport(document);
        if (kind == "dash") return dashReport(document);
    } catch (const std::exception& e) {
        return std::string("exception ") + e.what() + "\n";
    }
    return "unknown kind\n";
}
