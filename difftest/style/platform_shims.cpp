// The platform/default functions mbgl-core's style and expression code links
// against, for //difftest/style. platform/default's mbgl-default would bring
// the GL headless backend and system libraries (GL, X11, uv, ICU, curl, ...)
// that a Rust test binary cannot link in this environment, so this file
// provides them instead:
// - ported as in platform/default (they run on the style path): the layer
//   manager (background factory only: the difftest compares background
//   layers), Log::platformRecord, the thread-name and thread-local helpers,
//   MonotonicTimer, UTF-8 -> UTF-16;
// - stubs that throw or return a placeholder (never reached by the style
//   paths this difftest exercises): image decoding, zlib inflate, ICU number
//   formatting and BiDi, nunicode collation and case mapping, the run loop.

#include <mln/i18n/collator.hpp>
#include <mln/i18n/number_format.hpp>
#include <mln/layermanager/layer_factory.hpp>
#include <mln/layermanager/layer_manager.hpp>
#include <mln/style/layers/background_layer.hpp>
#include <mln/style/layers/background_layer_impl.hpp>
#include <mln/platform/thread.hpp>
#include <mln/text/bidi.hpp>
#include <mln/util/image.hpp>
#include <mln/util/logging.hpp>
#include <mln/util/monotonic_timer.hpp>
#include <mln/util/platform.hpp>
#include <mln/util/run_loop.hpp>
#include <mln/util/thread_local.hpp>
#include <mln/util/utf.hpp>
#include <mln/util/enum.hpp>

#include <chrono>
#include <codecvt>
#include <cstdlib>
#include <iostream>
#include <locale>
#include <map>
#include <memory>
#include <pthread.h>
#include <stdexcept>
#include <vector>

namespace mln {

namespace {

// BackgroundLayerFactory (layermanager/background_layer_factory.cpp) without
// createRenderLayer, which would link the renderer and the GL backend.
class DifftestBackgroundLayerFactory final : public LayerFactory {
public:
    const style::LayerTypeInfo* getTypeInfo() const noexcept final {
        return style::BackgroundLayer::Impl::staticTypeInfo();
    }

    std::unique_ptr<style::Layer> createLayer(const std::string& id, const style::conversion::Convertible&) noexcept final {
        return std::unique_ptr<style::Layer>(new (std::nothrow) style::BackgroundLayer(id));
    }

    std::unique_ptr<RenderLayer> createRenderLayer(Immutable<style::Layer::Impl>) noexcept final { return nullptr; }
};

class DifftestLayerManager final : public LayerManager {
public:
    DifftestLayerManager() { addLayerType(std::make_unique<DifftestBackgroundLayerFactory>()); }

private:
    void addLayerType(std::unique_ptr<LayerFactory> factory) {
        std::string type{factory->getTypeInfo()->type};
        typeToFactory.emplace(std::move(type), factory.get());
        factories.emplace_back(std::move(factory));
    }

    LayerFactory* getFactory(const std::string& type) noexcept final {
        auto search = typeToFactory.find(type);
        return (search != typeToFactory.end()) ? search->second : nullptr;
    }

    LayerFactory* getFactory(const style::LayerTypeInfo* typeInfo) noexcept final {
        for (const auto& factory : factories) {
            if (style::layerTypeInfoEquals(factory->getTypeInfo(), typeInfo)) {
                return factory.get();
            }
        }
        return nullptr;
    }

    std::vector<std::unique_ptr<LayerFactory>> factories;
    std::map<std::string, LayerFactory*> typeToFactory;
};

} // namespace

LayerManager* LayerManager::get() noexcept {
    static DifftestLayerManager instance;
    return &instance;
}

const bool LayerManager::annotationsEnabled = true;

void Log::platformRecord(EventSeverity severity, const std::string& msg) {
    std::cerr << "[" << Enum<EventSeverity>::toString(severity) << "] " << msg << '\n';
}

PremultipliedImage decodeImage(const std::string&) {
    throw std::runtime_error("decodeImage is not linked into the style difftest");
}

namespace platform {

std::string getCurrentThreadName() {
    char name[32] = "unknown";
    pthread_getname_np(pthread_self(), name, sizeof(name));
    return name;
}

void setCurrentThreadName(const std::string& name) {
    if (name.size() < 16) {
#if defined(__APPLE__)
        pthread_setname_np(name.c_str());  // macOS: current thread only
#else
        pthread_setname_np(pthread_self(), name.c_str());
#endif
    }
}

void setCurrentThreadPriority(double) {}

void attachThread() {}

void detachThread() {}

void runTask(const std::function<void()>& task) {
    task();
}

std::string uppercase(const std::string& string) {
    throw std::runtime_error("uppercase is not linked into the style difftest: " + string);
}

std::string lowercase(const std::string& string) {
    throw std::runtime_error("lowercase is not linked into the style difftest: " + string);
}

std::string formatNumber(double, const std::string&, const std::string&, uint8_t, uint8_t) {
    throw std::runtime_error("formatNumber is not linked into the style difftest");
}

class Collator::Impl {};

Collator::Collator(bool, bool, const std::optional<std::string>&) {
    throw std::runtime_error("Collator is not linked into the style difftest");
}

int Collator::compare(const std::string&, const std::string&) const {
    return 0;
}

std::string Collator::resolvedLocale() const {
    return "";
}

bool Collator::operator==(const Collator&) const {
    return true;
}

} // namespace platform

class BiDiImpl {};

BiDi::~BiDi() = default;

namespace util {

std::chrono::duration<double> MonotonicTimer::now() {
    return std::chrono::duration_cast<std::chrono::duration<double>>(
        std::chrono::steady_clock::now().time_since_epoch());
}

std::u16string convertUTF8ToUTF16(const std::string& str) {
    std::wstring_convert<std::codecvt_utf8_utf16<char16_t>, char16_t> conv;
    return conv.from_bytes(str);
}

class RunLoop::Impl {};

RunLoop::RunLoop(Type) {
    throw std::runtime_error("RunLoop is not linked into the style difftest");
}

RunLoop::~RunLoop() = default;

void RunLoop::wake() {}

void RunLoop::waitForEmpty(const SimpleIdentity) {}

namespace impl {

ThreadLocalBase::ThreadLocalBase() {
    if (pthread_key_create(&reinterpret_cast<pthread_key_t&>(storage), nullptr) != 0) {
        abort();
    }
}

ThreadLocalBase::~ThreadLocalBase() {
    pthread_key_delete(reinterpret_cast<pthread_key_t&>(storage));
}

void* ThreadLocalBase::get() const {
    return pthread_getspecific(reinterpret_cast<const pthread_key_t&>(storage));
}

void ThreadLocalBase::set(void* ptr) {
    if (pthread_setspecific(reinterpret_cast<pthread_key_t&>(storage), ptr) != 0) {
        abort();
    }
}

} // namespace impl
} // namespace util
} // namespace mln

// zlib (vendor/geojson-vt / tile decompression); not on the style path.
extern "C" {
int inflateInit2_(void*, int, const char*, int) {
    return -2; // Z_STREAM_ERROR
}
int inflate(void*, int) {
    return -2;
}
int inflateEnd(void*) {
    return -2;
}
int inflateReset(void*) {
    return -2;
}
}
