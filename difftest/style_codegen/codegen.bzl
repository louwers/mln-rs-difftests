"""Runs //tools/style-codegen to write the generated half of the difftest.

A plain Starlark rule instead of a genrule: genrules need a shell
(/bin/bash), which NixOS does not have at that path.
"""

def _difftest_sources_impl(ctx):
    cpp = ctx.outputs.cpp
    rs = ctx.outputs.rs
    ctx.actions.run(
        executable = ctx.executable.tool,
        arguments = ["--spec", ctx.file.spec.path, "--difftest", cpp.path, rs.path],
        inputs = [ctx.file.spec],
        outputs = [cpp, rs],
        mnemonic = "StyleCodegenDifftest",
        progress_message = "Generating the style codegen difftest into %s" % rs.short_path,
    )
    return [DefaultInfo(files = depset([cpp, rs]))]

difftest_sources = rule(
    implementation = _difftest_sources_impl,
    attrs = {
        "spec": attr.label(allow_single_file = [".json"], mandatory = True),
        "cpp": attr.output(mandatory = True),
        "rs": attr.output(mandatory = True),
        "tool": attr.label(executable = True, cfg = "exec", mandatory = True),
    },
)
