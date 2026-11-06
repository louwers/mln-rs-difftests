// extern "C" wrapper around the C++ pixelmatch the render-test runner uses
// (vendor/maplibre-native-base/deps/pixelmatch-cpp/include/mapbox/pixelmatch.hpp).
// The header defines its functions non-inline, so it is included in this one
// translation unit only.

#include <mapbox/pixelmatch.hpp>

#include <cstddef>
#include <cstdint>

extern "C" uint64_t ref_pixelmatch(const uint8_t* img1,
                                   const uint8_t* img2,
                                   std::size_t width,
                                   std::size_t height,
                                   uint8_t* output,
                                   double threshold,
                                   bool includeAA) {
    return mapbox::pixelmatch(img1, img2, width, height, output, threshold, includeAA);
}
