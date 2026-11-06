// The C++ oracle of //difftest/layers as a process: layers_difftest.rs
// writes one JSON request per line and reads the report (reference.cpp)
// followed by a line "<<<END>>>". A process because the Linux platform
// (mbgl-default + platform/linux, which has the LayerManager with the render
// layer factories) only links completely into C++ executables.

#include <iostream>
#include <string>

std::string layersReport(const std::string& json);

int main() {
    // The standard streams stay synchronized with stdio: no
    // std::ios::sync_with_stdio(false). The core logs warnings to std::cerr on
    // its log thread, and std::cerr is tied to std::cout, so every log line
    // also flushes std::cout from that thread. Unsynchronized streams are not
    // thread-safe; that race wrote replies twice or dropped them, and
    // layers_difftest.rs then compared every later request against another
    // request's reply.
    std::string line;
    while (std::getline(std::cin, line)) {
        std::cout << layersReport(line) << "<<<END>>>\n" << std::flush;
    }
    return 0;
}
