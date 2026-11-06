// extern "C" wrappers for //difftest/style_codegen: the C++ side of every
// case, as canonical text (see `canonical` below; style_codegen_difftest.rs
// writes the same format for the Rust side).

#include "difftest/style_codegen/generated.hpp"

#include <mln/style/conversion/layer.hpp>
#include <mln/style/conversion_impl.hpp>
#include <mln/style/layer.hpp>
#include <mln/style/light.hpp>
#include <mln/style/rapidjson_conversion.hpp>
#include <mln/util/rapidjson.hpp>

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <string>
#include <utility>
#include <vector>

using namespace mln;
using namespace mln::style;
using namespace mln::style::conversion;

namespace {

void appendString(std::string& out, const std::string& s) {
    out += '"';
    for (const unsigned char c : s) {
        if (c == '"' || c == '\\') {
            out += '\\';
            out += static_cast<char>(c);
        } else if (c < 0x20) {
            char buffer[8];
            std::snprintf(buffer, sizeof(buffer), "\\x%02x", c);
            out += buffer;
        } else {
            out += static_cast<char>(c);
        }
    }
    out += '"';
}

// null | true | false | u<n> | i<n> | d<16 hex digits of the bits> | dnan |
// "<string>" | [a,b] | {"k":v,...} with keys sorted by bytes.
void canonical(std::string& out, const Value& value) {
    if (value.is<mapbox::feature::null_value_t>()) {
        out += "null";
    } else if (const auto* b = value.getBool()) {
        out += *b ? "true" : "false";
    } else if (const auto* u = value.getUint()) {
        out += "u" + std::to_string(*u);
    } else if (const auto* i = value.getInt()) {
        out += "i" + std::to_string(*i);
    } else if (const auto* d = value.getDouble()) {
        if (*d != *d) {
            out += "dnan";
        } else {
            uint64_t bits;
            std::memcpy(&bits, d, sizeof(bits));
            char buffer[24];
            std::snprintf(buffer, sizeof(buffer), "d%016llx", static_cast<unsigned long long>(bits));
            out += buffer;
        }
    } else if (const auto* s = value.getString()) {
        appendString(out, *s);
    } else if (const auto* a = value.getArray()) {
        out += '[';
        for (std::size_t n = 0; n < a->size(); ++n) {
            if (n) out += ',';
            canonical(out, (*a)[n]);
        }
        out += ']';
    } else if (const auto* o = value.getObject()) {
        std::vector<const std::pair<const std::string, Value>*> members;
        for (const auto& member : *o) members.push_back(&member);
        std::sort(members.begin(), members.end(), [](auto* l, auto* r) { return l->first < r->first; });
        out += '{';
        for (std::size_t n = 0; n < members.size(); ++n) {
            if (n) out += ',';
            appendString(out, members[n]->first);
            out += ':';
            canonical(out, members[n]->second);
        }
        out += '}';
    } else {
        out += "?";
    }
}


std::string canonicalProperty(const StyleProperty& property) {
    return std::to_string(static_cast<int>(property.getKind())) + ":" + codegen_difftest::canonical(property.getValue());
}

char* dup(const std::string& s) {
    char* p = static_cast<char*>(std::malloc(s.size() + 1));
    std::memcpy(p, s.data(), s.size());
    p[s.size()] = '\0';
    return p;
}

// The zoom the cases evaluate at (style_codegen_difftest.rs: EVALUATION_ZOOM).
constexpr float kEvaluationZoom = 10.5f;

// Style JSON is parsed with `Parse<0>`, as style::Parser does.
bool parse(JSDocument& document, const char* json) {
    document.Parse<0>(json);
    return !document.HasParseError();
}

template <class F>
char* guarded(F&& f) {
    try {
        return dup(f());
    } catch (const std::exception& e) {
        return dup(std::string("exception:") + e.what());
    } catch (...) {
        return dup("exception:");
    }
}

} // namespace

std::string codegen_difftest::canonical(const Value& value) {
    std::string out;
    ::canonical(out, value);
    return out;
}

using codegen_difftest::canonical;

extern "C" {

void mln_codegen_ref_free(char* p) {
    std::free(p);
}

// `makeStyleProperty(XLayer::getDefaultYyy())`, or "none".
char* mln_codegen_ref_default(const char* type, const char* name) {
    return guarded([&] {
        const auto property = codegen_difftest::layerDefault(type, name);
        return property ? canonicalProperty(*property) : std::string("none");
    });
}

// `makeValue(Light::getDefaultYyy())`, or "none".
char* mln_codegen_ref_light_default(const char* name) {
    return guarded([&] {
        const auto value = codegen_difftest::lightDefault(name);
        return value ? canonical(*value) : std::string("none");
    });
}

// A fresh layer of the type, `setProperty(name, json)`; then
// "ok:<getProperty(name)>|<serialize()>" or "error:<message>".
char* mln_codegen_ref_set_property(const char* type, const char* name, const char* json, int32_t paint) {
    return guarded([&] {
        JSDocument document;
        if (!parse(document, json)) return std::string("json-error");
        auto layer = codegen_difftest::makeLayer(type);
        if (!layer) return std::string("no-layer");
        const JSValue* value = &document;
        const auto error = layer->setProperty(
            name, Convertible(value), paint ? Layer::PropertyScope::Paint : Layer::PropertyScope::Layout);
        if (error) return "error:" + error->message;
        return "ok:" + canonicalProperty(layer->getProperty(name)) + "|" + canonical(layer->serialize()) + "|" +
               codegen_difftest::evaluateLayer(*layer, kEvaluationZoom);
    });
}

// `Light().setProperty(name, json)`; then "ok:<getProperty(name)>" or
// "error:<message>".
char* mln_codegen_ref_light_set_property(const char* name, const char* json) {
    return guarded([&] {
        JSDocument document;
        if (!parse(document, json)) return std::string("json-error");
        Light light;
        const JSValue* value = &document;
        const auto error = light.setProperty(name, Convertible(value));
        if (error) return "error:" + error->message;
        return "ok:" + canonicalProperty(light.getProperty(name));
    });
}

// `convert<std::unique_ptr<Layer>>(json)`; then "ok:<serialize()>" or
// "error:<message>".
char* mln_codegen_ref_convert_layer(const char* json) {
    return guarded([&] {
        JSDocument document;
        if (!parse(document, json)) return std::string("json-error");
        Error error;
        const JSValue* value = &document;
        auto layer = convert<std::unique_ptr<Layer>>(Convertible(value), error);
        if (!layer) return "error:" + error.message;
        return "ok:" + canonical((*layer)->serialize()) + "|" + codegen_difftest::evaluateLayer(**layer, kEvaluationZoom);
    });
}

} // extern "C"
