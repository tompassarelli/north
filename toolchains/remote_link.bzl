load("@prelude//cxx:cxx_toolchain_types.bzl", "CxxToolchainInfo", "LinkerInfo")

def _fields(value):
    return {name: getattr(value, name) for name in dir(value)}

def _remote_link_cxx_toolchain_impl(ctx):
    base = ctx.attrs.base[CxxToolchainInfo]
    linker = _fields(base.linker_info)
    linker["link_binaries_locally"] = False
    cxx = _fields(base)
    cxx["linker_info"] = LinkerInfo(**linker)
    return [CxxToolchainInfo(**cxx)] + [p for p in ctx.attrs.base.providers if not isinstance(p, CxxToolchainInfo)]

remote_link_cxx_toolchain = rule(
    impl = _remote_link_cxx_toolchain_impl,
    attrs = {
        "base": attrs.toolchain_dep(providers = [CxxToolchainInfo]),
    },
    is_toolchain_rule = True,
)
