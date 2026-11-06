"""SQLite for the difftests that link both the C++ oracle and rusqlite."""

# Put first in the link_deps of a difftest whose oracle links mbgl-core and
# whose Rust side depends on rusqlite (through mln_storage), so Linux links
# one SQLite (BUILD.bazel here).
ORACLE_SQLITE = select({
    "@platforms//os:linux": ["//difftest/sqlite"],
    "//conditions:default": [],
})
