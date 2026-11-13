"""North's Rust test targets: one per nextest binary, each case in its own process, cached by its declared inputs."""

def _north_files_impl(ctx: AnalysisContext) -> list[Provider]:
    return [DefaultInfo(default_outputs = ctx.attrs.srcs)]

north_files = rule(
    impl = _north_files_impl,
    doc = "A package's source files handed on unchanged, so a test reads each at its repository path; filegroup would copy them under buck-out.",
    attrs = {
        "srcs": attrs.list(attrs.source(), default = []),
    },
)

def _north_tree_impl(ctx: AnalysisContext) -> list[Provider]:
    return [DefaultInfo(default_outputs = [output for part in ctx.attrs.parts for output in part[DefaultInfo].default_outputs])]

north_tree = rule(
    impl = _north_tree_impl,
    doc = "Every package's files as one target, the declared input of a test that reads the whole repository.",
    attrs = {
        "parts": attrs.list(attrs.dep(), default = []),
    },
)

NorthHostCasesInfo = provider(
    doc = "What the invoking host needs to run a test binary's host cases itself: the case runner, the binary and the environment its test target sets, less the workers' PATH, since a Cargo run keeps the invoking host's.",
    fields = ["binary", "binary_id", "env", "runner"],
)

def _test_executor():
    if not read_root_config("buck2_re_client", "engine_address"):
        return None
    return CommandExecutorConfig(
        local_enabled = False,
        remote_enabled = True,
        remote_execution_properties = {"OSFamily": "linux"},
        remote_execution_use_case = "buck2-default",
        remote_output_paths = "output_paths",
        use_windows_path_separators = False,
    )

def _runner_command(ctx: AnalysisContext) -> cmd_args:
    resources = [resource[DefaultInfo].default_outputs for resource in ctx.attrs.resources]
    selection = [flag for name in ctx.attrs.skip for flag in ["--skip", name]]
    return cmd_args(
        ctx.attrs._runner[RunInfo],
        "--binary-id",
        ctx.attrs.binary_id,
        selection,
        "--",
        ctx.attrs.binary[RunInfo],
        hidden = resources,
    )

def _north_test_impl(ctx: AnalysisContext) -> list[Provider]:
    command = _runner_command(ctx)
    return [
        DefaultInfo(default_outputs = ctx.attrs.binary[DefaultInfo].default_outputs),
        RunInfo(args = command),
        ExternalRunnerTestInfo(
            type = "rust",
            command = [command],
            env = ctx.attrs.env,
            labels = ctx.attrs.labels,
            run_from_project_root = True,
            use_project_relative_paths = True,
            supports_test_execution_caching = True,
            default_executor = _test_executor(),
        ),
        NorthHostCasesInfo(
            binary = ctx.attrs.binary[RunInfo],
            binary_id = ctx.attrs.binary_id,
            env = {name: value for name, value in ctx.attrs.env.items() if name != "PATH"},
            runner = ctx.attrs._runner[RunInfo],
        ),
    ]

_north_test = rule(
    impl = _north_test_impl,
    attrs = {
        "binary": attrs.dep(providers = [RunInfo]),
        "binary_id": attrs.string(),
        "env": attrs.dict(key = attrs.string(), value = attrs.arg(), default = {}),
        "labels": attrs.list(attrs.string(), default = []),
        "resources": attrs.list(attrs.dep(), default = []),
        "skip": attrs.list(attrs.string(), default = []),
        "_runner": attrs.exec_dep(default = "//crates/north-test-cases:north-test-cases", providers = [RunInfo]),
    },
)

def _test_path():
    path = read_root_config("north", "test_path", "")
    return {"PATH": path} if path else {}

def manifest_dir_flags():
    """`CARGO_MANIFEST_DIR` as the package's path from the repository root, set through rustc rather than `env`, where the prelude would make it an absolute path into one checkout's buck-out."""
    return ["-Zunstable-options", "--env-set=CARGO_MANIFEST_DIR=" + (native.package_name() or ".")]

def north_rust_test(name, binary_id, env = {}, run_env = {}, resources = [], labels = [], rustc_flags = [], skip = [], **kwargs):
    """The test binary `name`-bin, built as `cargo test` builds it, and the test target `name` that runs its cases."""
    if "CARGO_MANIFEST_DIR" in env:
        fail("north_rust_test sets CARGO_MANIFEST_DIR itself; drop it from env")
    native.rust_binary(
        name = name + "-bin",
        env = env,
        rustc_flags = ["--test"] + manifest_dir_flags() + rustc_flags,
        **kwargs
    )
    _north_test(
        name = name,
        binary = ":" + name + "-bin",
        binary_id = binary_id,
        env = _test_path() | env | {"CARGO_MANIFEST_DIR": native.package_name() or "."} | run_env,
        labels = labels,
        resources = ([":files"] if native.package_name() else []) + ["//:files"] + resources,
        skip = skip,
    )

def _north_host_cases_impl(ctx: AnalysisContext) -> list[Provider]:
    runs = [test[NorthHostCasesInfo] for test in ctx.attrs.targets]
    plan = ctx.actions.write_json(
        "host-cases.json",
        [{"binary": cmd_args(run.binary), "binary_id": run.binary_id, "env": run.env, "runner": cmd_args(run.runner)} for run in runs],
        absolute = True,
    )
    needed = [cmd_args(run.binary, run.runner, run.env.values()) for run in runs]
    return [DefaultInfo(default_output = plan, other_outputs = needed)]

north_host_cases = rule(
    impl = _north_host_cases_impl,
    doc = "A package's host cases as the invoking host runs them: one JSON plan naming, for each test target that holds some, its case runner, its binary and its environment, with those files left in this checkout by the build.",
    attrs = {
        "targets": attrs.list(attrs.dep(providers = [NorthHostCasesInfo]), default = []),
    },
)
