// Shared by the difftest references: the byte encoding of type::Type (one
// byte with the variant index; Array (9) is followed by N (0xFF = none) and
// the item type) and copying a result string out.

#pragma once

#include <mln/style/expression/type.hpp>

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>

namespace {

using namespace mln::style::expression;

inline type::Type decodeType(const uint8_t*& p) {
    const uint8_t tag = *p++;
    switch (tag) {
        case 0:
            return type::Null;
        case 1:
            return type::Number;
        case 2:
            return type::Boolean;
        case 3:
            return type::String;
        case 4:
            return type::Color;
        case 5:
            return type::Padding;
        case 6:
            return type::VariableAnchorOffsetCollection;
        case 7:
            return type::Object;
        case 8:
            return type::Value;
        case 9: {
            const uint8_t n = *p++;
            type::Type item = decodeType(p);
            if (n == 0xFF) {
                return type::Array(std::move(item));
            }
            return type::Array(std::move(item), std::size_t(n));
        }
        case 10:
            return type::Collator;
        case 11:
            return type::Formatted;
        case 12:
            return type::Error;
        default:
            return type::Image;
    }
}

inline size_t copyOut(const std::string& s, char* out, size_t cap) {
    std::memcpy(out, s.data(), s.size() < cap ? s.size() : cap);
    return s.size();
}

} // namespace
