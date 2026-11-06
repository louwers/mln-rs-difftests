// extern "C" wrappers around the libstdc++ algorithms that //vendor/libstdcxx
// ports. Each sorts/heaps a permutation of indices by external keys, so the
// result shows where equal keys end up.

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <queue>
#include <vector>

extern "C" void ref_std_sort(const int32_t* keys, uint32_t* perm, std::size_t n) {
    std::sort(perm, perm + n, [keys](uint32_t a, uint32_t b) { return keys[a] < keys[b]; });
}

extern "C" void ref_std_stable_sort(const int32_t* keys, uint32_t* perm, std::size_t n) {
    std::stable_sort(perm, perm + n, [keys](uint32_t a, uint32_t b) { return keys[a] < keys[b]; });
}

// Not a strict weak ordering (equivalence within `tolerance` is not
// transitive), like wagyu's comparators built on values_are_equal.
extern "C" void ref_std_stable_sort_almost(const double* keys, uint32_t* perm, std::size_t n, double tolerance) {
    std::stable_sort(perm, perm + n, [keys, tolerance](uint32_t a, uint32_t b) {
        if (std::fabs(keys[a] - keys[b]) <= tolerance) {
            return false;
        }
        return keys[a] < keys[b];
    });
}

extern "C" void ref_std_make_sort_heap(const int32_t* keys, uint32_t* perm, std::size_t n, uint32_t* heap) {
    auto comp = [keys](uint32_t a, uint32_t b) { return keys[a] < keys[b]; };
    std::make_heap(perm, perm + n, comp);
    std::copy(perm, perm + n, heap);
    std::sort_heap(perm, perm + n, comp);
}

// Pushes every index, interleaving a pop after each push where `popAfter[i]`
// is set, then pops the rest. Writes the popped indices in order.
extern "C" std::size_t ref_priority_queue(const int32_t* keys,
                                          const uint8_t* popAfter,
                                          std::size_t n,
                                          uint32_t* popped) {
    auto comp = [keys](uint32_t a, uint32_t b) { return keys[a] < keys[b]; };
    std::priority_queue<uint32_t, std::vector<uint32_t>, decltype(comp)> queue(comp);
    std::size_t k = 0;
    for (std::size_t i = 0; i < n; ++i) {
        queue.push(static_cast<uint32_t>(i));
        if (popAfter[i]) {
            popped[k++] = queue.top();
            queue.pop();
        }
    }
    while (!queue.empty()) {
        popped[k++] = queue.top();
        queue.pop();
    }
    return k;
}
