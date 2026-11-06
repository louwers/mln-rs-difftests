// extern "C" wrappers around the C++ storage read path (OfflineDatabase,
// DatabaseFileSource) and the sprite parser, for //difftest/storage.
//
// Values cross the boundary in a small little-endian encoding (see
// storage_difftest.rs): u8/u32/i32/i64/u64 as fixed-size integers, strings
// and byte strings as u32 length + bytes.

#include <mln/sprite/sprite_parser.hpp>
#include <mln/storage/database_file_source.hpp>
#include <mln/storage/file_source.hpp>
#include <mln/storage/offline_database.hpp>
#include <mln/storage/resource.hpp>
#include <mln/storage/resource_options.hpp>
#include <mln/storage/response.hpp>
#include <mln/style/image_impl.hpp>
#include <mln/util/async_request.hpp>
#include <mln/util/client_options.hpp>
#include <mln/util/logging.hpp>
#include <mln/util/run_loop.hpp>
#include <mln/util/tile_server_options.hpp>

#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

using namespace mln;

namespace {

struct Writer {
    std::string out;
    void u8(uint8_t v) { out.push_back(static_cast<char>(v)); }
    template <class T>
    void fixed(T v) {
        char buf[sizeof(T)];
        std::memcpy(buf, &v, sizeof(T));
        out.append(buf, sizeof(T));
    }
    void u32(uint32_t v) { fixed(v); }
    void i64(int64_t v) { fixed(v); }
    void f32(float v) { fixed(v); }
    void str(const std::string& s) {
        u32(static_cast<uint32_t>(s.size()));
        out.append(s);
    }
};

struct Reader {
    const uint8_t* p;
    const uint8_t* end;
    uint8_t u8() { return *p++; }
    template <class T>
    T fixed() {
        T v;
        std::memcpy(&v, p, sizeof(T));
        p += sizeof(T);
        return v;
    }
    std::string str() {
        auto n = fixed<uint32_t>();
        std::string s(reinterpret_cast<const char*>(p), n);
        p += n;
        return s;
    }
};

Resource readResource(Reader& r) {
    auto kind = static_cast<Resource::Kind>(r.u8());
    auto loadingMethod = static_cast<Resource::LoadingMethod>(r.u8());
    auto storagePolicy = static_cast<Resource::StoragePolicy>(r.u8());
    std::string url = r.str();
    std::optional<Resource::TileData> tileData;
    if (r.u8()) {
        Resource::TileData t;
        t.urlTemplate = r.str();
        t.pixelRatio = r.u8();
        t.x = r.fixed<int32_t>();
        t.y = r.fixed<int32_t>();
        t.z = r.fixed<int8_t>();
        tileData = t;
    }
    Resource resource(kind, url, tileData, loadingMethod);
    resource.storagePolicy = storagePolicy;
    if (r.u8()) {
        auto first = r.fixed<uint64_t>();
        auto second = r.fixed<uint64_t>();
        resource.dataRange = std::make_pair(first, second);
    }
    return resource;
}

void writeTimestamp(Writer& w, const std::optional<Timestamp>& t) {
    w.u8(t ? 1 : 0);
    if (t) w.i64(t->time_since_epoch().count());
}

void writeResponse(Writer& w, const std::optional<Response>& response) {
    w.u8(response ? 1 : 0);
    if (!response) return;
    w.u8(response->error ? 1 : 0);
    if (response->error) {
        w.u8(static_cast<uint8_t>(response->error->reason));
        w.str(response->error->message);
    }
    w.u8(response->noContent);
    w.u8(response->notModified);
    w.u8(response->mustRevalidate);
    w.u8(response->data ? 1 : 0);
    if (response->data) w.str(*response->data);
    writeTimestamp(w, response->modified);
    writeTimestamp(w, response->expires);
    w.u8(response->etag ? 1 : 0);
    if (response->etag) w.str(*response->etag);
}

void finish(Writer& w, uint8_t** out, size_t* outLen) {
    *outLen = w.out.size();
    *out = static_cast<uint8_t*>(std::malloc(w.out.size() + 1));
    std::memcpy(*out, w.out.data(), w.out.size());
}

// Log records of the calling thread while a call runs.
struct Record {
    EventSeverity severity;
    Event event;
    int64_t code;
    std::string msg;
};
std::mutex recordsMutex;
std::vector<Record> records;

struct CaptureObserver : Log::Observer {
    bool onRecord(EventSeverity severity, Event event, int64_t code, const std::string& msg) override {
        std::scoped_lock lock(recordsMutex);
        records.push_back({severity, event, code, msg});
        return true;
    }
};

void startCapture() {
    static std::once_flag flag;
    std::call_once(flag, [] {
        Log::useLogThread(false);
        Log::setObserver(std::make_unique<CaptureObserver>());
    });
    std::scoped_lock lock(recordsMutex);
    records.clear();
}

void writeRecords(Writer& w) {
    std::scoped_lock lock(recordsMutex);
    w.u32(static_cast<uint32_t>(records.size()));
    for (const auto& r : records) {
        w.u8(static_cast<uint8_t>(r.severity));
        w.u8(static_cast<uint8_t>(r.event));
        w.i64(r.code);
        w.str(r.msg);
    }
    records.clear();
}

} // namespace

extern "C" {

void ref_free(uint8_t* p) {
    std::free(p);
}

// OfflineDatabase(path, TileServerOptions()) reopened read-only, as the
// render-test ProxyFileSource sets READ_ONLY_MODE_KEY.
void* ref_offline_db_open(const char* path) {
    startCapture();
    auto* db = new OfflineDatabase(path, TileServerOptions());
    db->reopenDatabaseReadOnly(true);
    return db;
}

void ref_offline_db_close(void* db) {
    delete static_cast<OfflineDatabase*>(db);
}

// db->get(resource) for each encoded resource (u32 count + resources); out:
// u32 count + optional responses, then the log records.
void ref_offline_db_get(void* db, const uint8_t* req, size_t len, uint8_t** out, size_t* outLen) {
    startCapture();
    Reader r{req, req + len};
    Writer w;
    const auto count = r.fixed<uint32_t>();
    w.u32(count);
    for (uint32_t i = 0; i < count; i++) {
        Resource resource = readResource(r);
        writeResponse(w, static_cast<OfflineDatabase*>(db)->get(resource));
    }
    writeRecords(w);
    finish(w, out, outLen);
}

// DatabaseFileSource(ResourceOptions().withCachePath(path)) with
// READ_ONLY_MODE_KEY, one request per encoded resource, run on a RunLoop
// until all responded; out: u32 count + responses in request order.
void ref_dbfs_request_all(const char* path, const uint8_t* req, size_t len, uint8_t** out, size_t* outLen) {
    util::RunLoop loop;
    Reader r{req, req + len};
    const auto count = r.fixed<uint32_t>();
    std::vector<Resource> resources;
    for (uint32_t i = 0; i < count; i++) {
        resources.push_back(readResource(r));
    }

    std::vector<std::optional<Response>> responses(count);
    size_t received = 0;
    {
        DatabaseFileSource dbfs(ResourceOptions().withCachePath(path), ClientOptions());
        dbfs.setProperty(READ_ONLY_MODE_KEY, true);
        std::vector<std::unique_ptr<AsyncRequest>> requests;
        for (uint32_t i = 0; i < count; i++) {
            requests.push_back(dbfs.request(resources[i], [&, i](Response res) {
                responses[i] = res;
                if (++received == count) loop.stop();
            }));
        }
        if (count > 0) loop.run();
    }

    Writer w;
    w.u32(count);
    for (const auto& response : responses) {
        writeResponse(w, response);
    }
    finish(w, out, outLen);
}

// parseSprite(id, image, json); out: u8 ok, then the error message or the
// images, then the log records.
void ref_parse_sprite(const char* id,
                      const uint8_t* image,
                      size_t imageLen,
                      const uint8_t* json,
                      size_t jsonLen,
                      uint8_t** out,
                      size_t* outLen) {
    startCapture();
    Writer w;
    try {
        auto images = parseSprite(id,
                                  std::string(reinterpret_cast<const char*>(image), imageLen),
                                  std::string(reinterpret_cast<const char*>(json), jsonLen));
        w.u8(1);
        w.u32(static_cast<uint32_t>(images.size()));
        for (const auto& img : images) {
            w.str(img->id);
            w.u32(img->image.size.width);
            w.u32(img->image.size.height);
            w.f32(img->pixelRatio);
            w.u8(img->sdf);
            for (const auto* stretches : {&img->stretchX, &img->stretchY}) {
                w.u32(static_cast<uint32_t>(stretches->size()));
                for (const auto& s : *stretches) {
                    w.f32(s.first);
                    w.f32(s.second);
                }
            }
            w.u8(img->content ? 1 : 0);
            if (img->content) {
                w.f32(img->content->left);
                w.f32(img->content->top);
                w.f32(img->content->right);
                w.f32(img->content->bottom);
            }
            for (const auto* fit : {&img->textFitWidth, &img->textFitHeight}) {
                w.u8(*fit ? static_cast<uint8_t>(**fit) + 1 : 0);
            }
            w.str(std::string(reinterpret_cast<const char*>(img->image.data.get()), img->image.bytes()));
        }
    } catch (const std::exception& e) {
        w.u8(0);
        w.str(e.what());
    }
    writeRecords(w);
    finish(w, out, outLen);
}

} // extern "C"
