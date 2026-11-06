#!/usr/bin/env just --justfile

# How to call the current just executable. Note that just_executable() may have `\` in Windows paths, so we need to quote it.
just := quote(just_executable())

# The C++ core the difftests compare against (MODULE.bazel's local_path_override).
cpp := justfile_directory() / '..' / 'maplibre-native'

# On NixOS, Bazel and the binaries it builds run inside the C++ repo's #linux dev shell (clang, zlib, icu, the Vulkan
# loader, ...), with bazelisk from nixpkgs. Every local Bazel invocation goes through `just bazel` so that each one sees
# the same environment: a changed environment makes the Bazel server reconfigure the C++ toolchain and rebuild the C++
# core. Elsewhere (macOS, CI) the system's tools are the environment.
nixos := path_exists('/etc/NIXOS')
dev_shell := if nixos == 'true' {'nix develop ' + quote('path:' + justfile_directory() / '.nix-shell#linux') + ' --command'} else {''}
bazel := if nixos == 'true' {'nix shell nixpkgs#bazelisk --command bazelisk'} else {'"$(command -v bazelisk || command -v bazel)"'}

@_default:
    {{just}} --list

# Run Bazel with the given arguments, e.g. `just bazel test //difftest/tile/...`
[positional-arguments]
bazel *args: _dev-shell
    {{dev_shell}} {{bazel}} "$@"

# Run the difftests as CI does (Linux and macOS)
ci-test:  env-info (bazel 'test' '--config=ci' '//...')

# Print environment info
env-info:
    @echo "PWD {{justfile_directory()}}"
    {{just}} --version
    @echo "Bazel $(cat .bazelversion) (.bazelversion){{if nixos == 'true' {' in the NixOS dev shell'} else {''} }}"
    @echo "mln-rs $(grep -m1 -o '"=[0-9.]*"' Cargo.toml | tr -d '"=') (Cargo.toml)"
    @echo "maplibre-native $(git -C {{quote(cpp)}} rev-parse HEAD 2>/dev/null || echo 'not found') ({{cpp}})"

# Reformat the Rust files with rustfmt
fmt *args:
    git ls-files -z '*.rs' | xargs -0 rustfmt --edition 2024 {{args}}

# Run every difftest. Arguments go to `bazel test`, e.g. `just test --config=oracle-opt`
[positional-arguments]
test *args:  _dev-shell
    {{dev_shell}} {{bazel}} test "$@" //...

# Cargo.lock sets Bazel's crates, so MODULE.bazel.lock is refreshed too.
[doc('Update the crates within their Cargo.toml requirements (e.g. after changing the mln-rs version)')]
update:
    cargo update
    {{just}} bazel query @next_crates//:all > /dev/null

# The C++ repo's dev shell flake, used from a copy in .nix-shell: `nix develop path:<repo>` would copy the whole C++ working
# tree into the store on every call. Nothing to do outside NixOS.
_dev-shell:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "{{nixos}}" == true ]] || exit 0
    mkdir -p .nix-shell
    for f in flake.nix flake.lock; do
        cmp -s {{quote(cpp)}}/"$f" ".nix-shell/$f" || cp {{quote(cpp)}}/"$f" ".nix-shell/$f"
    done
