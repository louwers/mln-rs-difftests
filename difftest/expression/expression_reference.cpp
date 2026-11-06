// extern "C" wrapper around the whole C++ expression engine for
// expression_difftest.rs.

#include "type_encoding.hpp"

using namespace mln::style::expression;

// ---------------------------------------------------------------------------
// Whole-expression reports: parse, type, constness, serialization and
// evaluation, written in a canonical text form that expression_difftest.rs
// produces identically from the Rust engine.

#include <mln/style/expression/expression.hpp>
#include <mln/style/expression/formatted.hpp>
#include <mln/style/expression/is_constant.hpp>
#include <mln/style/expression/parsing_context.hpp>
#include <mln/style/expression/value.hpp>
#include <mln/style/rapidjson_conversion.hpp>
#include <mln/util/feature.hpp>
#include <mln/util/rapidjson.hpp>

#include <mapbox/geojson.hpp>
#include <mapbox/geojson/rapidjson.hpp>
#include <rapidjson/stringbuffer.h>
#include <rapidjson/writer.h>

#include <algorithm>
#include <cstdio>
#include <map>
#include <vector>

namespace {

template <class T>
std::string hexBits(T v) {
    using U = std::conditional_t<sizeof(T) == 8, uint64_t, uint32_t>;
    U bits;
    std::memcpy(&bits, &v, sizeof(T));
    char buf[24];
    std::snprintf(buf, sizeof(buf), sizeof(T) == 8 ? "%016llx" : "%08llx", static_cast<unsigned long long>(bits));
    return buf;
}

std::string quote(const std::string& s) {
    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.String(s);
    return buffer.GetString();
}

std::string canon(const mln::Value& v);

std::string canonObject(const std::unordered_map<std::string, mln::Value>& o) {
    std::map<std::string, const mln::Value*> sorted;
    for (const auto& e : o) sorted.emplace(e.first, &e.second);
    std::string s = "{";
    bool first = true;
    for (const auto& e : sorted) {
        if (!first) s += ",";
        first = false;
        s += quote(e.first) + ":" + canon(*e.second);
    }
    return s + "}";
}

std::string canon(const mln::Value& v) {
    return v.match([](const mln::NullValue&) -> std::string { return "null"; },
                   [](bool b) -> std::string { return b ? "b:true" : "b:false"; },
                   [](uint64_t u) -> std::string { return "u:" + std::to_string(u); },
                   [](int64_t i) -> std::string { return "i:" + std::to_string(i); },
                   [](double d) -> std::string { return "d:" + hexBits(d); },
                   [](const std::string& s) -> std::string { return "s:" + quote(s); },
                   [](const std::vector<mln::Value>& a) -> std::string {
                       std::string s = "[";
                       for (std::size_t i = 0; i < a.size(); ++i) {
                           if (i) s += ",";
                           s += canon(a[i]);
                       }
                       return s + "]";
                   },
                   [](const std::unordered_map<std::string, mln::Value>& o) -> std::string { return canonObject(o); });
}

std::string canonExpr(const Value& v);

std::string canonColor(const mln::Color& c) {
    return hexBits(c.r) + "," + hexBits(c.g) + "," + hexBits(c.b) + "," + hexBits(c.a);
}

std::string canonExpr(const Value& v) {
    return v.match(
        [](const mln::NullValue&) -> std::string { return "null"; },
        [](bool b) -> std::string { return b ? "b:true" : "b:false"; },
        [](double d) -> std::string { return "n:" + hexBits(d); },
        [](const std::string& s) -> std::string { return "s:" + quote(s); },
        [](const mln::Color& c) -> std::string { return "c:" + canonColor(c); },
        [](const mln::VariableAnchorOffsetCollection& o) -> std::string { return "v:" + quote(o.toString()); },
        [](const Collator&) -> std::string { return "collator"; },
        [](const Formatted& f) -> std::string { return "f:" + canon(f.toObject()); },
        [](const Image& i) -> std::string { return "i:" + quote(i.id()) + (i.isAvailable() ? ",1" : ",0"); },
        [](const mln::Padding& p) -> std::string {
            return "p:" + hexBits(p.top) + "," + hexBits(p.right) + "," + hexBits(p.bottom) + "," + hexBits(p.left);
        },
        [](const std::vector<Value>& a) -> std::string {
            std::string s = "[";
            for (std::size_t i = 0; i < a.size(); ++i) {
                if (i) s += ",";
                s += canonExpr(a[i]);
            }
            return s + "]";
        },
        [](const std::unordered_map<std::string, Value>& o) -> std::string {
            std::map<std::string, const Value*> sorted;
            for (const auto& e : o) sorted.emplace(e.first, &e.second);
            std::string s = "{";
            bool first = true;
            for (const auto& e : sorted) {
                if (!first) s += ",";
                first = false;
                s += quote(e.first) + ":" + canonExpr(*e.second);
            }
            return s + "}";
        });
}

} // namespace

extern "C" {

// `json`: the expression; `expected`: an encoded type (see decodeType) or
// null; `props`: a JSON object of feature properties; `zooms`: evaluation
// zooms, NaN meaning no zoom. Returns the report length (copied up to cap).
size_t ref_expression_report(const char* json,
                             size_t jsonLen,
                             const uint8_t* expected,
                             const char* props,
                             size_t propsLen,
                             const float* zooms,
                             size_t zoomCount,
                             char* out,
                             size_t cap) {
    using namespace mln::style;
    std::string report;

    mln::JSDocument document;
    document.Parse<rapidjson::kParseFullPrecisionFlag>(json, jsonLen);
    if (document.HasParseError()) {
        return copyOut("json error", out, cap);
    }

    std::optional<type::Type> expectedType;
    if (expected) {
        const uint8_t* p = expected;
        expectedType = decodeType(p);
    }
    ParsingContext ctx = expectedType ? ParsingContext(*expectedType) : ParsingContext();
    const mln::JSValue* value = &document;
    ParseResult parsed = ctx.parseLayerPropertyExpression(conversion::Convertible(value));

    if (!parsed) {
        report += "compiled: error\n";
        for (const auto& e : ctx.getErrors()) {
            report += "error: " + e.key + "|" + e.message + "\n";
        }
        return copyOut(report, out, cap);
    }

    const Expression& e = **parsed;
    report += "compiled: success\n";
    report += "type: " + type::toString(e.getType()) + "\n";
    report += std::string("featureConstant: ") + (isFeatureConstant(e) ? "1" : "0") + "\n";
    report += std::string("zoomConstant: ") + (isZoomConstant(e) ? "1" : "0") + "\n";
    report += "serialized: " + canon(e.serialize()) + "\n";

    mln::JSDocument propsDocument;
    propsDocument.Parse<rapidjson::kParseFullPrecisionFlag>(props, propsLen);
    mln::Feature feature(mapbox::geometry::point<double>(0.0, 0.0));
    feature.properties = mapbox::geojson::convert<mln::PropertyMap>(propsDocument);

    for (std::size_t i = 0; i < zoomCount; ++i) {
        std::optional<float> zoom;
        if (!std::isnan(zooms[i])) zoom = zooms[i];
        const EvaluationResult result = e.evaluate(zoom, feature, std::nullopt);
        if (result) {
            report += "out: " + canonExpr(*result) + "\n";
        } else {
            report += "err: " + result.error().message + "\n";
        }
    }
    return copyOut(report, out, cap);
}

} // extern "C"
