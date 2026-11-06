// The kind of a C++ exception for the //difftest/tile dumps. Built with
// RTTI (mbgl-core is -fno-rtti) to read the dynamic type name: catching
// std::out_of_range or std::runtime_error by type would reference their
// typeinfo, whose weak copies in renderer objects of mbgl-core would drag the
// GL backend into the link.

#include <protozero/exception.hpp>

#include <exception>
#include <string>
#include <typeinfo>

std::string exceptionKind(std::exception_ptr e, std::string& what) {
    try {
        std::rethrow_exception(e);
    } catch (const protozero::exception& ex) {
        what = ex.what();
        return "pbf";
    } catch (const std::exception& ex) {
        what = ex.what();
        const std::string type = typeid(ex).name();
        if (type == "St12out_of_range") {
            return "out_of_range";
        }
        if (type == "St13runtime_error") {
            return "runtime";
        }
        return "other(" + type + ")";
    }
}
