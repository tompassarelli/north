load("@prelude//rust:rust_toolchain.bzl", "PanicRuntime", "RustToolchainInfo")

_LINKER_FLAGS = {
    "rust-lld": ["-Zunstable-options", "-Clinker-features=+lld", "-Clink-self-contained=+linker"],
}

def _nix_rust_toolchain_impl(ctx):
    return [
        DefaultInfo(),
        RustToolchainInfo(
            allow_lints = ctx.attrs.allow_lints,
            clippy_driver = RunInfo(args = [ctx.attrs.clippy_driver]),
            compiler = RunInfo(args = [ctx.attrs.compiler]),
            default_edition = ctx.attrs.default_edition,
            deny_lints = ctx.attrs.deny_lints,
            nightly_features = True,
            panic_runtime = PanicRuntime("unwind"),
            rustc_binary_flags = ctx.attrs.rustc_binary_flags,
            rustc_flags = _LINKER_FLAGS[ctx.attrs.linker] + ctx.attrs.rustc_flags,
            rustc_target_triple = ctx.attrs.rustc_target_triple,
            rustc_test_flags = ctx.attrs.rustc_test_flags,
            rustdoc = RunInfo(args = [ctx.attrs.rustdoc]),
            rustdoc_flags = ctx.attrs.rustdoc_flags,
            warn_lints = ctx.attrs.warn_lints,
        ),
    ]

nix_rust_toolchain = rule(
    impl = _nix_rust_toolchain_impl,
    attrs = {
        "allow_lints": attrs.list(attrs.string(), default = []),
        "clippy_driver": attrs.string(),
        "compiler": attrs.string(),
        "default_edition": attrs.string(),
        "deny_lints": attrs.list(attrs.string(), default = []),
        "linker": attrs.enum(_LINKER_FLAGS.keys()),
        "rustc_binary_flags": attrs.list(attrs.arg(), default = []),
        "rustc_flags": attrs.list(attrs.arg(), default = []),
        "rustc_target_triple": attrs.string(),
        "rustc_test_flags": attrs.list(attrs.arg(), default = []),
        "rustdoc": attrs.string(),
        "rustdoc_flags": attrs.list(attrs.arg(), default = []),
        "warn_lints": attrs.list(attrs.string(), default = []),
    },
    is_toolchain_rule = True,
)
