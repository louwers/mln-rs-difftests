// extern "C" wrappers around what Manifest::doShuffle
// (render-test/manifest_parser.cpp) uses from the C++ standard library.

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <random>
#include <vector>

// The first `n` outputs of std::mt19937 seeded with std::seed_seq{seed}.
extern "C" void ref_mt19937_seed_seq(uint32_t seed, uint32_t* out, std::size_t n) {
    std::seed_seq sequence{seed};
    std::mt19937 generator(sequence);
    for (std::size_t i = 0; i < n; ++i) {
        out[i] = static_cast<uint32_t>(generator());
    }
}

// Manifest::doShuffle on [0, n): writes the permutation.
extern "C" void ref_shuffle(uint32_t seed, uint32_t* out, std::size_t n) {
    std::vector<uint32_t> values(n);
    for (std::size_t i = 0; i < n; ++i) values[i] = static_cast<uint32_t>(i);
    std::seed_seq sequence{seed};
    std::mt19937 shuffler(sequence);
    std::shuffle(values.begin(), values.end(), shuffler);
    std::copy(values.begin(), values.end(), out);
}
