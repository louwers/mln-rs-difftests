// extern "C" wrappers around the C++ expression engine for the Rust
// difftests in this package.
//
// Types cross the boundary in a byte encoding: one byte with the variant
// index of type::Type, and for Array (index 9) one byte N (0xFF = no N)
// followed by the item type.

#include "type_encoding.hpp"

#include <mln/style/expression/check_subtype.hpp>
#include <mln/style/expression/type.hpp>

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>

using namespace mln::style::expression;



extern "C" {

size_t ref_type_to_string(const uint8_t* enc, char* out, size_t cap) {
    const uint8_t* p = enc;
    return copyOut(type::toString(decodeType(p)), out, cap);
}

// Returns -1 if `t` is a subtype of `expected`, else the message length.
long ref_check_subtype(const uint8_t* expected, const uint8_t* t, char* out, size_t cap) {
    const uint8_t* pe = expected;
    const uint8_t* pt = t;
    const type::Type e = decodeType(pe);
    const type::Type tt = decodeType(pt);
    const auto err = type::checkSubtype(e, tt);
    if (!err) {
        return -1;
    }
    return static_cast<long>(copyOut(*err, out, cap));
}

} // extern "C"

