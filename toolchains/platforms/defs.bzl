load("@prelude//cfg/exec_platform:marker.bzl", "get_exec_platform_marker")

def _execution_platform_impl(ctx):
    constraints = {}
    for dep in ctx.attrs.constraint_values:
        constraints.update(dep[ConfigurationInfo].constraints)
    cfg = ConfigurationInfo(constraints = constraints, values = {})
    remote = bool(ctx.attrs.remote_execution_properties)
    platform = ExecutionPlatformInfo(
        label = ctx.label.raw_target(),
        configuration = cfg,
        executor_config = CommandExecutorConfig(
            local_enabled = not remote,
            remote_enabled = remote,
            remote_execution_properties = ctx.attrs.remote_execution_properties,
            remote_execution_use_case = "buck2-default",
            remote_output_paths = "output_paths",
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
        "remote_execution_properties": attrs.dict(attrs.string(), attrs.string(), default = {}),
    },
)
