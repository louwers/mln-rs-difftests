// extern "C" protozero::pbf_reader walk for //difftest/tile:pbf_difftest:
// every field with every getter of its wire type (on copies of the reader)
// and every packed iterator type, recursing into length-delimited fields.
// The same walk is in pbf_difftest.rs.

#include <protozero/pbf_reader.hpp>

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

namespace {

void hex(std::string& out, const char* data, std::size_t size) {
    static const char* digits = "0123456789abcdef";
    out += 'x';
    for (std::size_t i = 0; i < size; ++i) {
        const auto c = static_cast<unsigned char>(data[i]);
        out += digits[c >> 4];
        out += digits[c & 15];
    }
}

template <typename T>
void num(std::string& out, T v) {
    out += ' ';
    out += std::to_string(v);
}

template <typename T>
void fbits(std::string& out, T v) {
    char buf[24];
    if constexpr (sizeof(T) == 4) {
        uint32_t b;
        std::memcpy(&b, &v, 4);
        std::snprintf(buf, sizeof buf, " f%08x", b);
    } else {
        uint64_t b;
        std::memcpy(&b, &v, 8);
        std::snprintf(buf, sizeof buf, " d%016llx", static_cast<unsigned long long>(b));
    }
    out += buf;
}

template <typename F>
void attempt(std::string& out, F f) {
    try {
        f();
    } catch (const protozero::exception& e) {
        out += " !";
        out += e.what();
    }
}

template <typename Range, typename Out>
void packed(std::string& out, const char* name, Range range, Out emit) {
    out += " ";
    out += name;
    out += "[";
    out += std::to_string(range.size());
    out += "]";
    try {
        auto it = range.begin();
        const auto end = range.end();
        while (it != end) {
            emit(*it++);
        }
    } catch (const protozero::exception& e) {
        out += " !";
        out += e.what();
    }
}

void walk(std::string& out, const char* data, std::size_t size, int depth) {
    protozero::pbf_reader r(data, size);
    while (true) {
        try {
            if (!r.next()) {
                out += "end\n";
                return;
            }
        } catch (const protozero::exception& e) {
            out += "next! ";
            out += e.what();
            out += " tag ";
            out += std::to_string(r.tag());
            out += " wt ";
            out += std::to_string(static_cast<uint32_t>(r.wire_type()));
            out += "\n";
            return;
        }
        out += "f " + std::to_string(r.tag()) + " " + std::to_string(static_cast<uint32_t>(r.wire_type())) + " " +
               std::to_string(r.tag_and_type());
        std::string nested;
        switch (r.wire_type()) {
            case protozero::pbf_wire_type::varint:
                attempt(out, [&] { auto c = r; num(out, c.get_uint64()); });
                attempt(out, [&] { auto c = r; num(out, c.get_int64()); });
                attempt(out, [&] { auto c = r; num(out, c.get_sint64()); });
                attempt(out, [&] { auto c = r; num(out, c.get_uint32()); });
                attempt(out, [&] { auto c = r; num(out, c.get_int32()); });
                attempt(out, [&] { auto c = r; num(out, c.get_sint32()); });
                attempt(out, [&] { auto c = r; num(out, c.get_enum()); });
                attempt(out, [&] { auto c = r; num(out, c.get_bool() ? 1 : 0); });
                break;
            case protozero::pbf_wire_type::fixed64:
                attempt(out, [&] { auto c = r; num(out, c.get_fixed64()); });
                attempt(out, [&] { auto c = r; num(out, c.get_sfixed64()); });
                attempt(out, [&] { auto c = r; fbits(out, c.get_double()); });
                break;
            case protozero::pbf_wire_type::fixed32:
                attempt(out, [&] { auto c = r; num(out, c.get_fixed32()); });
                attempt(out, [&] { auto c = r; num(out, c.get_sfixed32()); });
                attempt(out, [&] { auto c = r; fbits(out, c.get_float()); });
                break;
            case protozero::pbf_wire_type::length_delimited:
                attempt(out, [&] {
                    auto c = r;
                    auto v = c.get_view();
                    out += " ";
                    hex(out, v.data(), v.size());
                    if (depth < 3) {
                        walk(nested, v.data(), v.size(), depth + 1);
                    }
                    out += " rest " + std::to_string(c.length());
                });
                attempt(out, [&] { auto c = r; packed(out, "u32", c.get_packed_uint32(), [&](uint32_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "i32", c.get_packed_int32(), [&](int32_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "s32", c.get_packed_sint32(), [&](int32_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "u64", c.get_packed_uint64(), [&](uint64_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "i64", c.get_packed_int64(), [&](int64_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "s64", c.get_packed_sint64(), [&](int64_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "bool", c.get_packed_bool(), [&](int32_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "enum", c.get_packed_enum(), [&](int32_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "f32", c.get_packed_fixed32(), [&](uint32_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "sf32", c.get_packed_sfixed32(), [&](int32_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "f64", c.get_packed_fixed64(), [&](uint64_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "sf64", c.get_packed_sfixed64(), [&](int64_t v) { num(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "float", c.get_packed_float(), [&](float v) { fbits(out, v); }); });
                attempt(out, [&] { auto c = r; packed(out, "double", c.get_packed_double(), [&](double v) { fbits(out, v); }); });
                break;
            default:
                break;
        }
        out += "\n";
        out += nested;
        try {
            r.skip();
        } catch (const protozero::exception& e) {
            out += "skip! ";
            out += e.what();
            out += "\n";
            return;
        }
        out += "len " + std::to_string(r.length()) + "\n";
    }
}

} // namespace

extern "C" {

char* mln_pbf_ref_walk(const uint8_t* data, size_t size, size_t* out_len) {
    std::string out;
    walk(out, reinterpret_cast<const char*>(data), size, 0);
    char* result = static_cast<char*>(std::malloc(out.size() + 1));
    std::memcpy(result, out.data(), out.size());
    result[out.size()] = 0;
    *out_len = out.size();
    return result;
}

void mln_pbf_ref_free(char* p) {
    std::free(p);
}

// decode_varint / skip_varint on a raw buffer: "<value> <consumed>" or "!<what>".
char* mln_pbf_ref_varint(const uint8_t* data, size_t size, size_t* out_len) {
    std::string out;
    const char* begin = reinterpret_cast<const char*>(data);
    const char* d = begin;
    try {
        const auto v = protozero::decode_varint(&d, begin + size);
        out += std::to_string(v) + " " + std::to_string(d - begin);
    } catch (const protozero::exception& e) {
        out += "!";
        out += e.what();
        out += " " + std::to_string(d - begin);
    }
    d = begin;
    try {
        protozero::skip_varint(&d, begin + size);
        out += " skip " + std::to_string(d - begin);
    } catch (const protozero::exception& e) {
        out += " skip!";
        out += e.what();
        out += " " + std::to_string(d - begin);
    }
    char* result = static_cast<char*>(std::malloc(out.size() + 1));
    std::memcpy(result, out.data(), out.size());
    result[out.size()] = 0;
    *out_len = out.size();
    return result;
}

} // extern "C"
