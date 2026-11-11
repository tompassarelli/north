load("@prelude//cfg/exec_platform:marker.bzl", "get_exec_platform_marker")

def _execution_platform_impl(ctx):
    constraints = {}
    for dep in ctx.attrs.constraint_values:
        constraints.update(dep[ConfigurationInfo].constraints)
    cfg = ConfigurationInfo(constraints = constraints, values = {})
    platform = ExecutionPlatformInfo(
        label = ctx.label.raw_target(),
        configuration = cfg,
        executor_config = CommandExecutorConfig(
            local_enabled = True,
            remote_enabled = False,
            use_windows_path_separators = False,
        ),
    )
    return [
        DefaultInfo(),
        platform,
        PlatformInfo(label = str(ctx.label.raw_target()), configuration = cfg),
        ExecutionPlatformRegistrationInfo(
            platforms = [platform],
            exec_marker_constraint = get_exec_platform_marker(),
        ),
    ]

execution_platform = rule(
    impl = _execution_platform_impl,
    attrs = {
        "constraint_values": attrs.list(attrs.dep(providers = [ConfigurationInfo])),
    },
)
