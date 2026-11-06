// The C++ oracle of //difftest/storage as an executable (mbgl-core +
// mbgl-default + platform/linux link completely only into C++ executables,
// and the C++ core's vendored SQLite would clash with rusqlite's in one
// binary). storage_difftest.rs drives it over stdin/stdout: each request is
// u8 opcode + u32 length + payload, each answer u32 length + payload, both
// in reference.cpp's encoding.

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

extern "C" {
void ref_free(uint8_t* p);
void* ref_offline_db_open(const char* path);
void ref_offline_db_close(void* db);
void ref_offline_db_get(void* db, const uint8_t* req, size_t len, uint8_t** out, size_t* outLen);
void ref_dbfs_request_all(const char* path, const uint8_t* req, size_t len, uint8_t** out, size_t* outLen);
void ref_parse_sprite(const char* id,
                      const uint8_t* image,
                      size_t imageLen,
                      const uint8_t* json,
                      size_t jsonLen,
                      uint8_t** out,
                      size_t* outLen);
}

namespace {

bool readExact(void* buf, size_t n) {
    return std::fread(buf, 1, n, stdin) == n;
}

uint32_t u32(const uint8_t*& p) {
    uint32_t v;
    std::memcpy(&v, p, 4);
    p += 4;
    return v;
}

std::string str(const uint8_t*& p) {
    const uint32_t n = u32(p);
    std::string s(reinterpret_cast<const char*>(p), n);
    p += n;
    return s;
}

} // namespace

int main() {
    while (true) {
        uint8_t op;
        uint32_t len;
        if (!readExact(&op, 1) || !readExact(&len, 4)) return 0;
        std::vector<uint8_t> payload(len);
        if (len && !readExact(payload.data(), len)) return 1;
        const uint8_t* p = payload.data();
        const uint8_t* end = payload.data() + payload.size();

        uint8_t* out = nullptr;
        size_t outLen = 0;
        if (op == 1) {
            const std::string path = str(p);
            void* db = ref_offline_db_open(path.c_str());
            ref_offline_db_get(db, p, static_cast<size_t>(end - p), &out, &outLen);
            ref_offline_db_close(db);
        } else if (op == 2) {
            const std::string path = str(p);
            ref_dbfs_request_all(path.c_str(), p, static_cast<size_t>(end - p), &out, &outLen);
        } else if (op == 3) {
            const std::string id = str(p);
            const std::string image = str(p);
            const std::string json = str(p);
            ref_parse_sprite(id.c_str(),
                             reinterpret_cast<const uint8_t*>(image.data()),
                             image.size(),
                             reinterpret_cast<const uint8_t*>(json.data()),
                             json.size(),
                             &out,
                             &outLen);
        } else {
            return 2;
        }
        const uint32_t n = static_cast<uint32_t>(outLen);
        std::fwrite(&n, 1, 4, stdout);
        if (n) std::fwrite(out, 1, n, stdout);
        std::fflush(stdout);
        ref_free(out);
    }
}
