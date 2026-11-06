// extern "C" wrappers around the C nunicode (@maplibre//vendor:nunicode) for
// nunicode_difftest.rs. The string functions re-implement, line for line,
// the loops of the C++ platform code that the Rust crate ports:
// `uppercase`/`lowercase` from platform/default/src/mln/util/string_stdlib.cpp
// and `unaccent` from platform/default/src/mln/i18n/collator.cpp.

#include <libnu/casemap.h>
#include <libnu/ducet.h>
#include <libnu/strcoll.h>
#include <libnu/unaccent.h>
#include <libnu/utf8.h>

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <sstream>
#include <string>

namespace {

size_t copyOut(const std::string& s, char* out, size_t cap) {
    std::memcpy(out, s.data(), s.size() < cap ? s.size() : cap);
    return s.size();
}

using Transform = const char* (*)(const char*, const char*, nu_read_iterator_t, uint32_t*, const char**, void*);

// string_stdlib.cpp `uppercase` / `lowercase`, collator.cpp `unaccent`.
std::string transform(const std::string& str, Transform fn) {
    std::stringstream output;
    char const* itr = str.c_str();
    char const* nitr;
    char const* end = itr + str.length();
    char lo[5] = {0};

    for (; itr < end; itr = nitr) {
        uint32_t code_point = 0;
        char const* buf = nullptr;

        nitr = fn(itr, end, nu_utf8_read, &code_point, &buf, nullptr);
        if (buf != nullptr) {
            do {
                buf = NU_CASEMAP_DECODING_FUNCTION(buf, &code_point);
                if (code_point == 0) break;
                output.write(lo, nu_utf8_write(code_point, lo) - lo);
            } while (code_point != 0);
        } else {
            output.write(itr, nitr - itr);
        }
    }

    return output.str();
}

long mapped(const char* m, char* out, size_t cap) {
    if (m == nullptr) {
        return -1;
    }
    return static_cast<long>(copyOut(std::string(m), out, cap));
}

} // namespace

extern "C" {

int ref_nu_strcoll(const char* a, size_t alen, const char* b, size_t blen) {
    const std::string sa(a, alen), sb(b, blen);
    return nu_strcoll(sa.c_str(), sb.c_str(), nu_utf8_read, nu_utf8_read);
}

int ref_nu_strcasecoll(const char* a, size_t alen, const char* b, size_t blen) {
    const std::string sa(a, alen), sb(b, blen);
    return nu_strcasecoll(sa.c_str(), sb.c_str(), nu_utf8_read, nu_utf8_read);
}

int32_t ref_nu_ducet_weight(uint32_t codepoint) {
    return nu_ducet_weight(codepoint, nullptr, nullptr);
}

int32_t ref_nu_ducet_weight_reentry(uint32_t codepoint, int32_t state, int32_t* rollback) {
    int32_t w = state;
    const int32_t result = nu_ducet_weight(codepoint, &w, nullptr);
    *rollback = w;
    return result;
}

long ref_nu_toupper(uint32_t codepoint, char* out, size_t cap) {
    return mapped(nu_toupper(codepoint), out, cap);
}

long ref_nu_tolower(uint32_t codepoint, char* out, size_t cap) {
    return mapped(nu_tolower(codepoint), out, cap);
}

long ref_nu_tounaccent(uint32_t codepoint, char* out, size_t cap) {
    return mapped(nu_tounaccent(codepoint), out, cap);
}

size_t ref_uppercase(const char* s, size_t len, char* out, size_t cap) {
    return copyOut(transform(std::string(s, len), _nu_toupper), out, cap);
}

size_t ref_lowercase(const char* s, size_t len, char* out, size_t cap) {
    return copyOut(transform(std::string(s, len), _nu_tolower), out, cap);
}

size_t ref_unaccent(const char* s, size_t len, char* out, size_t cap) {
    return copyOut(transform(std::string(s, len), _nu_tounaccent), out, cap);
}

} // extern "C"
