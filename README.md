# mln-rs-difftests

[![CI](https://github.com/louwers/mln-rs-difftests/actions/workflows/ci.yml/badge.svg)](https://github.com/louwers/mln-rs-difftests/actions/workflows/ci.yml)

Differential tests of [mln-rs](https://crates.io/crates/mln-rs), the Rust
port of the MapLibre Native core
([maplibre-native-next](https://github.com/louwers/maplibre-native-next)),
against the C++ core. Each one calls a released mln-rs crate and the C++
code it ports on the same inputs, and requires identical results.

They were `//difftest/<area>` in maplibre-native-next (moved at
louwers/maplibre-native-next@b69fd02, which has their history), so that
repository needs neither Bazel nor C++. They test the release that
`Cargo.toml` pins: every `mln-rs-*` crate at one version (from crates.io, or
a git commit of maplibre-native-next).

## Running

The difftests build the C++ core with Bazel from a checkout next to this
one, `../maplibre-native`: the `maplibre-native-next-support` branch of
[louwers/maplibre-native](https://github.com/louwers/maplibre-native) (CI
pins `4e03abe94cf562b10562c528578d109c7273694d`), with its vendored
submodules:

```sh
git clone -b maplibre-native-next-support https://github.com/louwers/maplibre-native ../maplibre-native
git -C ../maplibre-native submodule update --init --recursive --depth 1 -- vendor
```

The tasks are in the [justfile](justfile) (`just` lists them):

```sh
just test                                    # every difftest
just bazel test //difftest/tile/...          # one area
just test --config=oracle-opt                # with an optimized C++ oracle
```

Bazel always runs through `just bazel`: on NixOS that runs it inside the
C++ repo's `#linux` dev shell. Many comparisons are pinned to the Linux
oracle and are skipped on macOS (`target_compatible_with`).

## Testing another mln-rs version

Change every `mln-rs-*` entry in `Cargo.toml` and run `just update`, which
refreshes `Cargo.lock` and `MODULE.bazel.lock`: a release from crates.io
(`mln-rs-util = "=0.0.9"`), or, for unreleased changes, the same git
dependency for each crate
(`mln-rs-util = { git = "https://github.com/louwers/maplibre-native-next", rev = "<commit>" }`).

## Writing a difftest

Rust must behave exactly like C++. For a ported function, add a difftest
in `difftest/<area>`:

```starlark
cc_library(
    name = "reference",
    srcs = ["reference.cpp"],          # extern "C" wrappers around C++ calls
    copts = ["-std=c++20"],
    deps = ["@maplibre//:mbgl-core"],
)

rust_test(
    name = "util_difftest",
    srcs = ["util_difftest.rs"],
    link_deps = [":reference"],
    deps = [
        "@next_crates//:mln-rs-util",
        "@next_crates//:proptest",
    ],
)
```

- Pass POD values and `(ptr, len)` byte strings across the boundary, or JSON
  for structured values. C++ exceptions are caught in the wrapper and returned
  as error strings.
- Inputs: proptest strategies plus a hand-written list of edge cases
  (±0, NaN, ±inf, subnormals, boundaries, malformed strings).
- Compare bit-exactly (`to_bits`); any NaN equals any NaN.
- The oracle is built at `-c fastbuild`, so C++ `assert`s are live. Don't
  generate inputs that violate a C++ precondition; note the case instead.
- Also run them once with `--config=oracle-opt`.
- A comparison pinned to the Linux oracle (glibc libm/strtod, libstdc++
  ordering and hashing, x86-64 conversions, arm64 FMA contraction, or an
  oracle linking `@maplibre//platform/linux`) gets
  `target_compatible_with = ["@platforms//os:linux"]`, so wildcard runs skip
  it on macOS; leave genuinely cross-platform comparisons unrestricted — they
  run on macOS CI too.
- A crate the tests use goes into `Cargo.toml`; Bazel names it
  `@next_crates//:<package>`.

## The C++ fork

Changes made in `louwers/maplibre-native` so it works as a Bazel dependency
(to upstream as one PR):

1. `MODULE.bazel`: `node`/`npm` extensions are no longer dev dependencies
   (`//:BUILD.bazel` loads `@npm`); the rustutils `crate` extension is (a
   non-root crate_universe call would have to ship a generated lockfile).
2. `BUILD.bazel`: the style and shader code generators get their root from
   `repo_name()`, because in a dependency their files live under
   `external/<repo>`.
3. `metrics/BUILD.bazel`: the render and expression test filegroups are
   public.
