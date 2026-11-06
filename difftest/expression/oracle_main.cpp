// The whole-engine oracle as a process: expression_difftest.rs writes one
// request per line,
//   {"e": "<expression JSON>", "t": [type encoding bytes] | null,
//    "p": "<feature properties JSON>", "z": [zoom | null, ...]}
// and reads the report (see expression_reference.cpp) followed by a line
// "<<<END>>>". A process instead of a linked library because the Linux
// platform (mbgl-default + platform/linux) only links completely into C++
// executables.

#include <rapidjson/document.h>

#include <cmath>
#include <cstdint>
#include <iostream>
#include <limits>
#include <string>
#include <vector>

extern "C" size_t ref_expression_report(const char* json,
                                        size_t jsonLen,
                                        const uint8_t* expected,
                                        const char* props,
                                        size_t propsLen,
                                        const float* zooms,
                                        size_t zoomCount,
                                        char* out,
                                        size_t cap);

int main() {
    // No std::ios::sync_with_stdio(false): the core logs to std::cerr from
    // other threads, which flushes the tied std::cout, and unsynchronized
    // streams are not thread-safe (see difftest/layers/oracle_main.cpp).
    std::string line;
    std::vector<char> buffer(1 << 16);
    while (std::getline(std::cin, line)) {
        rapidjson::Document request;
        request.Parse(line.c_str(), line.size());
        if (request.HasParseError() || !request.IsObject()) {
            std::cout << "bad request\n<<<END>>>\n" << std::flush;
            continue;
        }
        const std::string expression(request["e"].GetString(), request["e"].GetStringLength());
        const std::string props(request["p"].GetString(), request["p"].GetStringLength());
        std::vector<uint8_t> type;
        const bool hasType = request["t"].IsArray();
        if (hasType) {
            for (const auto& b : request["t"].GetArray()) type.push_back(static_cast<uint8_t>(b.GetUint()));
        }
        std::vector<float> zooms;
        for (const auto& z : request["z"].GetArray()) {
            zooms.push_back(z.IsNull() ? std::numeric_limits<float>::quiet_NaN() : static_cast<float>(z.GetDouble()));
        }
        size_t len = 0;
        while (true) {
            len = ref_expression_report(expression.data(),
                                        expression.size(),
                                        hasType ? type.data() : nullptr,
                                        props.data(),
                                        props.size(),
                                        zooms.data(),
                                        zooms.size(),
                                        buffer.data(),
                                        buffer.size());
            if (len <= buffer.size()) break;
            buffer.resize(len);
        }
        std::cout.write(buffer.data(), static_cast<std::streamsize>(len));
        std::cout << "<<<END>>>\n" << std::flush;
    }
    return 0;
}
