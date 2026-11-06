// The platform/default functions that the mbgl-core objects on the tile
// decoding path link against (logging and the thread helpers used by the
// log scheduler), for //difftest/tile. platform/default's mbgl-default would
// bring the GL headless backend, which a test binary cannot link here (see
// //difftest/style/platform_shims.cpp for the same approach).

#include <mln/platform/thread.hpp>
#include <mln/util/enum.hpp>
#include <mln/util/logging.hpp>
#include <mln/util/thread_local.hpp>

#include <cstdlib>
#include <functional>
#include <iostream>
#include <pthread.h>
#include <stdexcept>
#include <string>

namespace mln {

void Log::platformRecord(EventSeverity severity, const std::string& msg) {
    std::cerr << "[" << Enum<EventSeverity>::toString(severity) << "] " << msg << '\n';
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

} // namespace platform

namespace util {
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

// The scheduler objects that logging.cpp pulls in reference the run loop;
// the tile path never runs one.
#include <mln/util/run_loop.hpp>

namespace mln {
namespace platform {

void runTask(const std::function<void()>& task) {
    task();
}

} // namespace platform

namespace util {

class RunLoop::Impl {};

RunLoop::RunLoop(Type) {
    throw std::runtime_error("RunLoop is not linked into the tile difftest");
}

RunLoop::~RunLoop() = default;

void RunLoop::wake() {}

void RunLoop::waitForEmpty(const SimpleIdentity) {}

} // namespace util
} // namespace mln
