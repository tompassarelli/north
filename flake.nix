{
  description = "north — fact-native work coordination (CLI + MCP, on babashka)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # Codex moves faster than the general runtime package set. Keep its package
    # source explicit so consumers such as Firn can make this input follow their
    # own canonical nixpkgs-master pin without changing North's package graph.
    nixpkgs-master.url = "github:NixOS/nixpkgs/master";
    flake-utils.url = "github:numtide/flake-utils";

    # CI needs the Beagle-provided engine source for integration tests, not its
    # runtime closure. Keep that identity exact and inert in North's package graph.
    beagle-engine-source = {
      url = "github:tompassarelli/beagle/e55dbf48617aa71d85da9383b9cb2ac7230456bd";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, nixpkgs-master, flake-utils, beagle-engine-source }:
    assert builtins.pathExists (beagle-engine-source + "/store/out/store/rpc.clj");
    # nixpkgs' current Babashka no longer supports x86_64-darwin. Publish only
    # the three systems whose complete North runtime closure is evaluable.
    flake-utils.lib.eachSystem [
      "x86_64-linux"
      "aarch64-linux"
      "aarch64-darwin"
    ] (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        lib = pkgs.lib;
        codexPkgs = nixpkgs-master.legacyPackages.${system};
        codexExpectedIdentity = {
          version = "0.146.0";
          owner = "openai";
          repo = "codex";
          rev = "refs/tags/rust-v0.146.0";
          tag = "rust-v0.146.0";
          srcHash = "sha256-/kTIOX/klxm1nq2bJsBqS8f1jZZp2ilaTeULQFPJgDk=";
          cargoHash = "sha256-N9jbH/cgAyu2QxneSnpkdaF0MgV3ZtDmN9q6rr9u+hE=";
        };
        codexUpstreamPkg =
          codexPkgs.codex or
            (throw "nixpkgs-master does not provide Codex for North's supported system ${system}");
        codexObservedIdentity = {
          version = codexUpstreamPkg.version or null;
          owner = codexUpstreamPkg.src.owner or null;
          repo = codexUpstreamPkg.src.repo or null;
          rev = codexUpstreamPkg.src.rev or null;
          tag = codexUpstreamPkg.src.tag or null;
          srcHash = codexUpstreamPkg.src.outputHash or null;
          cargoHash = codexUpstreamPkg.cargoHash or null;
        };
        codexPkg =
          assert lib.assertMsg
            (codexObservedIdentity == codexExpectedIdentity)
            ("North's managed Codex patch source identity drifted; expected "
              + builtins.toJSON codexExpectedIdentity + "; observed "
              + builtins.toJSON codexObservedIdentity);
          codexUpstreamPkg;
        codexVersionSmoke = pkgs.runCommand
          "north-codex-version-smoke-${codexPkg.version}"
          { nativeBuildInputs = [ codexPkg ]; }
          ''
            expected='codex-cli ${codexPkg.version}'
            actual="$(${codexPkg}/bin/codex --version)"
            if [ "$actual" != "$expected" ]; then
              echo "North Codex package version mismatch" >&2
              echo "expected: $expected" >&2
              echo "actual:   $actual" >&2
              exit 1
            fi
            touch "$out"
          '';
        codexAppServerContractSmoke = pkgs.runCommand
          "north-codex-app-server-contract-smoke-${codexPkg.version}"
          {
            nativeBuildInputs = [ codexPkg pkgs.python3 ];
            CODEX_ADAPTER = ./sdk/src/providers/codex-app-server.ts;
            CODEX_BIN = "${codexPkg}/bin/codex";
            CODEX_EXPECTED_VERSION = codexPkg.version;
          }
          ''
            schema="$TMPDIR/codex-app-server-schema"
            "$CODEX_BIN" app-server generate-json-schema \
              --experimental --out "$schema"
            CODEX_SCHEMA="$schema/codex_app_server_protocol.schemas.json" \
              ${pkgs.python3}/bin/python3 <<'PY'
import json
import os
import re
import subprocess

adapter = open(os.environ["CODEX_ADAPTER"], encoding="utf-8").read()

version_match = re.search(
    r'export const MANAGED_CODEX_VERSION = "([0-9]+\.[0-9]+\.[0-9]+)";',
    adapter,
)
if version_match is None:
    raise SystemExit("managed Codex adapter version export is missing")
if version_match.group(1) != os.environ["CODEX_EXPECTED_VERSION"]:
    raise SystemExit(
        "managed Codex adapter/package version mismatch: "
        f"adapter={version_match.group(1)} package={os.environ['CODEX_EXPECTED_VERSION']}"
    )

def string_array(name):
    match = re.search(
        rf"export const {name} = \[(.*?)\]\s+as const;",
        adapter,
        re.DOTALL,
    )
    if match is None:
        raise SystemExit(f"managed Codex {name} export is missing")
    values = re.findall(r'"([a-z0-9_]+)"', match.group(1))
    if len(values) != len(set(values)):
        raise SystemExit(f"managed Codex {name} contains duplicates")
    return set(values)

enabled = string_array("MANAGED_CODEX_ENABLED_FEATURES")
disabled = string_array("MANAGED_CODEX_DISABLED_FEATURES")
if enabled & disabled:
    raise SystemExit("managed Codex feature classifications overlap")

feature_rows = subprocess.run(
    [os.environ["CODEX_BIN"], "features", "list"],
    check=True,
    capture_output=True,
    text=True,
).stdout.splitlines()
all_features = set()
nonremoved = set()
for row in feature_rows:
    fields = row.split()
    if len(fields) < 3:
        raise SystemExit(f"malformed Codex feature row: {row!r}")
    name = fields[0]
    all_features.add(name)
    if fields[1] != "removed":
        nonremoved.add(name)

classified = enabled | disabled | {"network_proxy"}
unknown = classified - all_features
missing = nonremoved - classified
if unknown:
    raise SystemExit(f"managed Codex classifies unknown features: {sorted(unknown)}")
if missing:
    raise SystemExit(f"managed Codex leaves nonremoved features unclassified: {sorted(missing)}")

with open(os.environ["CODEX_SCHEMA"], encoding="utf-8") as stream:
    definitions = json.load(stream)["definitions"]

notification = definitions["ServerNotification"]
emitted_at = notification.get("properties", {}).get("emittedAtMs")
if emitted_at is None or emitted_at.get("type") != "integer" \
        or emitted_at.get("format") != "int64" \
        or "emittedAtMs" in notification.get("required", []):
    raise SystemExit("Codex ServerNotification emittedAtMs contract drifted")

expected_params = {
    "initialize": {"$ref": "#/definitions/InitializeParams"},
    "thread/start": {"$ref": "#/definitions/v2/ThreadStartParams"},
    "hooks/list": {"$ref": "#/definitions/v2/HooksListParams"},
    "turn/start": {"$ref": "#/definitions/v2/TurnStartParams"},
    "turn/interrupt": {"$ref": "#/definitions/v2/TurnInterruptParams"},
    "mcpServerStatus/list": {"$ref": "#/definitions/v2/ListMcpServerStatusParams"},
    "command/exec": {"$ref": "#/definitions/v2/CommandExecParams"},
    "config/read": {"$ref": "#/definitions/v2/ConfigReadParams"},
    "configRequirements/read": {"type": "null"},
    "account/read": {"$ref": "#/definitions/v2/GetAccountParams"},
}
observed_params = {}
request_variants = {}
for variant in definitions["ClientRequest"]["oneOf"]:
    methods = variant.get("properties", {}).get("method", {}).get("enum", [])
    if len(methods) != 1 or methods[0] not in expected_params:
        continue
    method = methods[0]
    if method in observed_params:
        raise SystemExit(f"duplicate Codex request schema for {method}")
    observed_params[method] = variant.get("properties", {}).get("params")
    request_variants[method] = variant
if observed_params != expected_params:
    raise SystemExit(
        f"managed Codex request refs drifted: observed={observed_params!r}"
    )
requirements_read = request_variants["configRequirements/read"]
if "params" in requirements_read.get("required", []):
    raise SystemExit("configRequirements/read unexpectedly requires params")

initialized = []
for variant in definitions["ClientNotification"]["oneOf"]:
    methods = variant.get("properties", {}).get("method", {}).get("enum", [])
    if methods == ["initialized"]:
        initialized.append(variant)
if len(initialized) != 1 \
        or initialized[0].get("required") != ["method"] \
        or set(initialized[0].get("properties", {})) != {"method"}:
    raise SystemExit("Codex initialized notification contract drifted")

initialize_response = definitions["InitializeResponse"]
expected_initialize_fields = {"userAgent", "codexHome", "platformFamily", "platformOs"}
if set(initialize_response.get("required", [])) != expected_initialize_fields \
        or set(initialize_response.get("properties", {})) != expected_initialize_fields \
        or initialize_response["properties"]["codexHome"].get("allOf") \
            != [{"$ref": "#/definitions/v2/AbsolutePathBuf"}]:
    raise SystemExit("Codex InitializeResponse authority fields drifted")
for field in ("userAgent", "platformFamily", "platformOs"):
    if initialize_response["properties"][field].get("type") != "string":
        raise SystemExit(f"Codex InitializeResponse {field} type drifted")
PY
            touch "$out"
          '';
        codexManagedHookFailureSmoke = pkgs.runCommand
          "north-codex-managed-hook-failure-smoke-${codexPkg.version}"
          {
            nativeBuildInputs = [
              pkgs.bash
              pkgs.coreutils
              pkgs.python3
            ];
          }
          ''
            bash ${./bin/tests/codex-managed-hook-failure-smoke.sh} \
              ${codexPkg}/bin/codex \
              ${pkgs.libredirect}/lib/libredirect.so \
              ${pkgs.python3}/bin/python3
            touch "$out"
          '';
        sdkVersion =
          let
            declared = (builtins.fromJSON (builtins.readFile ./sdk/package.json))
              .dependencies."@anthropic-ai/claude-agent-sdk";
            exact = lib.removePrefix "^" declared;
          in
            if builtins.match "[0-9]+\\.[0-9]+\\.[0-9]+" exact != null then
              exact
            else
              throw "North requires an exact or caret-prefixed Claude SDK version, got ${declared}";
        zodVersion =
          let
            declared = (builtins.fromJSON (builtins.readFile ./sdk/package.json))
              .dependencies.zod;
            exact = lib.removePrefix "^" declared;
          in
            if builtins.match "[0-9]+\\.[0-9]+\\.[0-9]+" exact != null then
              exact
            else
              throw "North requires an exact or caret-prefixed Zod version, got ${declared}";
        opentuiVersion =
          let
            declared = (builtins.fromJSON (builtins.readFile ./sdk/package.json))
              .dependencies."@opentui/core";
          in
            if builtins.match "[0-9]+\\.[0-9]+\\.[0-9]+" declared != null then
              declared
            else
              throw "North requires an exact OpenTUI version, got ${declared}";
        # Runtime PATH for the bb-backed CLIs. util-linux supplies `setsid` for
        # managed lanes on every supported host. iproute2 supplies Linux's `ss`
        # for daemon-health probes and procps supplies lifecycle-hook `pgrep`;
        # neither has a Darwin package, where the corresponding host utilities
        # are part of the platform runtime.
        runtimePackages = [
          pkgs.babashka
          pkgs.coreutils
          pkgs.bash
          pkgs.bun
          pkgs.findutils
          pkgs.gawk
          pkgs.git
          pkgs.gnugrep
          pkgs.gnused
          pkgs.python3
          pkgs.util-linux
        ] ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [
          # The read-only shell and managed Codex both exec through it; undeclared,
          # it was supplied only by whatever ambient PATH the launcher happened to carry.
          pkgs.bubblewrap
          pkgs.iproute2
          pkgs.procps
        ] ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
          pkgs.lsof
        ];
        runtimePath = lib.makeBinPath runtimePackages
          + lib.optionalString pkgs.stdenv.hostPlatform.isDarwin
            ":/usr/bin:/bin:/usr/sbin:/sbin";
        sdkPlatform =
          if pkgs.stdenv.hostPlatform.isLinux then
            if pkgs.stdenv.hostPlatform.isx86_64 then
              {
                packageName = "@anthropic-ai/claude-agent-sdk-linux-x64";
                url = "https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk-linux-x64/-/claude-agent-sdk-linux-x64-${sdkVersion}.tgz";
                hash = "sha256-8/GE6r4O8xV5d1ldbdt0pbOrAJh9Wj7BUHkzMkfaH+c=";
              }
            else if pkgs.stdenv.hostPlatform.isAarch64 then
              {
                packageName = "@anthropic-ai/claude-agent-sdk-linux-arm64";
                url = "https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk-linux-arm64/-/claude-agent-sdk-linux-arm64-${sdkVersion}.tgz";
                hash = "sha256-1TvCr4FpvGorgeBvzTDZhN+/10+KTV3mFpfK6USWr/8=";
              }
            else throw "North's Claude SDK package does not support ${system}"
          else if pkgs.stdenv.hostPlatform.isDarwin then
            if pkgs.stdenv.hostPlatform.isAarch64 then
              {
                packageName = "@anthropic-ai/claude-agent-sdk-darwin-arm64";
                url = "https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk-darwin-arm64/-/claude-agent-sdk-darwin-arm64-${sdkVersion}.tgz";
                hash = "sha256-7QSObN3pKI9xyqIUNM/wYBU/z9xhzf76fAgm3/w0RRU=";
              }
            else throw "North's Claude SDK package does not support ${system}"
          else throw "North's Claude SDK package does not support ${system}";
        sdkSource = pkgs.fetchurl {
          url = "https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/-/claude-agent-sdk-${sdkVersion}.tgz";
          hash = "sha256-H86L8GA5ar4iZzzqLmWcdOWWFg0G8OIJM2mZfu0WQAo=";
        };
        sdkPlatformSource = pkgs.fetchurl {
          inherit (sdkPlatform) url hash;
        };
        opentuiPlatform =
          if pkgs.stdenv.hostPlatform.isLinux then
            if pkgs.stdenv.hostPlatform.isx86_64 then
              if pkgs.stdenv.hostPlatform.isMusl then {
                packageName = "@opentui/core-linux-x64-musl";
                tarballName = "core-linux-x64-musl";
                hash = "sha512-SEg+/lG2ToswziX/ICMRy2QTHmZcb2wfQftQsbmejjL8zI3vGIhT+YlgVHz4jYlGm9zj/gLl2hAHj7mGKPZFzA==";
              } else {
                packageName = "@opentui/core-linux-x64";
                tarballName = "core-linux-x64";
                hash = "sha512-GwPW6tXCamEUdg3ykabzYW9ayGCOR18yiHKbuY8GB5EgbcA2rkwczE7KQs08RGuSNSWIGEEwHZE2cqhXjYogCQ==";
              }
            else if pkgs.stdenv.hostPlatform.isAarch64 then
              if pkgs.stdenv.hostPlatform.isMusl then {
                packageName = "@opentui/core-linux-arm64-musl";
                tarballName = "core-linux-arm64-musl";
                hash = "sha512-fIS0eDs9m6SDgVVG0Aaqn6Co39K8J436Vp0xMD3FjPc41mpsoFjyHquhIkpAX9bh8Qr225uGG5zRm7A+88FlKw==";
              } else {
                packageName = "@opentui/core-linux-arm64";
                tarballName = "core-linux-arm64";
                hash = "sha512-BidyUBbI6n9WGPZpmJ1X457FUCMhbj7G2kcPnUln8w2zuaXGb9AN8QfOTtD9JGuujkx0Xsz8yhfC3cCWLQyz5w==";
              }
            else throw "North's OpenTUI package does not support ${system}"
          else if pkgs.stdenv.hostPlatform.isDarwin then
            if pkgs.stdenv.hostPlatform.isAarch64 then {
              packageName = "@opentui/core-darwin-arm64";
              tarballName = "core-darwin-arm64";
              hash = "sha512-FcLH4Rs2/xnBOudMzuHimEK8aNuJ3QpOde+xjz+7hf/0cmurnDJLs+VJ85qYYdBdcnuBonFbjZfB19OLd2RwIA==";
            } else throw "North's OpenTUI package does not support ${system}"
          else throw "North's OpenTUI package does not support ${system}";
        opentuiSource = pkgs.fetchurl {
          url = "https://registry.npmjs.org/@opentui/core/-/core-${opentuiVersion}.tgz";
          hash = "sha512-LCHPiwB8zjvJ1KTTayQoq5nygdRwLI1ApsvWiCF06PtMPr0yP/zU+L3xk9KiFoTbAabtgA7PoBf16M2hPZR+tg==";
        };
        opentuiPlatformSource = pkgs.fetchurl {
          url = "https://registry.npmjs.org/${opentuiPlatform.packageName}/-/${opentuiPlatform.tarballName}-${opentuiVersion}.tgz";
          inherit (opentuiPlatform) hash;
        };
        opentuiRuntimeSources = [
          { packageName = "@opentui/core"; source = opentuiSource; }
          { inherit (opentuiPlatform) packageName; source = opentuiPlatformSource; }
          {
            packageName = "bun-ffi-structs";
            source = pkgs.fetchurl {
              url = "https://registry.npmjs.org/bun-ffi-structs/-/bun-ffi-structs-0.3.1.tgz";
              hash = "sha512-3gM7PpVWLyrwxWjcilSiGuhWanhZivvo6l0u573NziPH6f/gwk6McbaYgn7oJWov6pKGRTDbrg94W5DcJsKTtQ==";
            };
          }
          {
            packageName = "diff";
            source = pkgs.fetchurl {
              url = "https://registry.npmjs.org/diff/-/diff-9.0.0.tgz";
              hash = "sha512-svtcdpS8CgJyqAjEQIXdb3OjhFVVYjzGAPO8WGCmRbrml64SPw/jJD4GoE98aR7r25A0XcgrK3F02yw9R/vhQw==";
            };
          }
          {
            packageName = "marked";
            source = pkgs.fetchurl {
              url = "https://registry.npmjs.org/marked/-/marked-17.0.1.tgz";
              hash = "sha512-boeBdiS0ghpWcSwoNm/jJBwdpFaMnZWRzjA6SkUMYb40SVaN1x7mmfGKp0jvexGcx+7y2La5zRZsYFZI6Qpypg==";
            };
          }
          {
            packageName = "string-width";
            source = pkgs.fetchurl {
              url = "https://registry.npmjs.org/string-width/-/string-width-7.2.0.tgz";
              hash = "sha512-tsaTIkKW9b4N+AEj+SVA+WhJzV7/zMhcSu78mLKWSk7cXMOSHsBKFWUs0fWwq8QyK3MgJBQRX6Gbi4kYbdvGkQ==";
            };
          }
          {
            packageName = "strip-ansi";
            source = pkgs.fetchurl {
              url = "https://registry.npmjs.org/strip-ansi/-/strip-ansi-7.1.2.tgz";
              hash = "sha512-gmBGslpoQJtgnMAvOVqGZpEz9dyoKTCzy2nfz/n8aIFhN/jCE/rCmcxabB6jOOHV+0WNnylOxaxBQPSvcWklhA==";
            };
          }
          {
            packageName = "emoji-regex";
            source = pkgs.fetchurl {
              url = "https://registry.npmjs.org/emoji-regex/-/emoji-regex-10.6.0.tgz";
              hash = "sha512-toUI84YS5YmxW219erniWD0CIVOo46xGKColeNQRgOzDorgBi1v4D71/OFzgD9GO2UGKIv1C3Sp8DAn0+j5w7A==";
            };
          }
          {
            packageName = "get-east-asian-width";
            source = pkgs.fetchurl {
              url = "https://registry.npmjs.org/get-east-asian-width/-/get-east-asian-width-1.6.0.tgz";
              hash = "sha512-QRbvDIbx6YklUe6RxeTeleMR0yv3cYH6PsPZHcnVn7xv7zO1BHN8r0XETu8n6Ye3Q+ahtSarc3WgtNWmehIBfA==";
            };
          }
          {
            packageName = "ansi-regex";
            source = pkgs.fetchurl {
              url = "https://registry.npmjs.org/ansi-regex/-/ansi-regex-6.2.2.tgz";
              hash = "sha512-Bq3SmSpyFHaWjPk8If9yc6svM8c56dB5BAtW4Qbw5jHTwwXXcTLoRMkpDJp6VL0XzlWaCHTXrkFURMYmD0sLqg==";
            };
          }
        ];
        zodSource = pkgs.fetchurl {
          url = "https://registry.npmjs.org/zod/-/zod-${zodVersion}.tgz";
          hash = "sha512-ytENFjIJFl2UwYglde2jchW2Hwm4GJFLDiSXWdTrJQBIN9Fcyp7n4DhxJEiWNAJMV1/BqWfW/kkg71UDcHJyTQ==";
        };
        runtimeSource = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./out
            ./share/help
            (lib.fileset.difference ./cli ./cli/tests)
            ./sdk/src
            ./contracts/agent-run-ledger-v2.json
            ./profiles/tom/hooks/lib/harness-dial.sh
            ./bin/north
            ./bin/north-comms
            ./bin/north-mcp
            ./bin/north-actor-key
            ./bin/north-mark-delegated
            ./bin/north-on-spawn
            ./bin/north-on-stop
            ./bin/north-on-tooluse
            ./bin/north-succession
            ./bin/north-stream-sync
            ./bin/north-stream-sync-all
            ./bin/docctl
            ./bin/concern
            ./bin/ensure-private-docs
          ];
        };
        # Runtime-only Orchestration contract. Generated adapters, authoring scripts,
        # skills, and private docs stay out of North's closure.
        orchestrationContract = pkgs.stdenvNoCC.mkDerivation {
          pname = "orchestration-runtime-contract";
          version = builtins.substring 0 12 (self.rev or self.dirtyRev or "local");
          src = ./orchestration;
          dontConfigure = true;
          dontBuild = true;
          installPhase = ''
            runHook preInstall
            mkdir -p $out/staffing $out/providers $out/docs/deltas $out/scripts
            cp staffing/catalog.json $out/staffing/
            cp providers/anthropic.json providers/openai.json $out/providers/
            cp docs/roles.md docs/task-grades.md docs/topologies.md docs/postures.md docs/comms.md $out/docs/
            cp -r docs/deltas/. $out/docs/deltas/
            # Canonical assessment validator + its only import. North's runtime
            # (routing-economics.ts) resolves selection-assessment.mjs under
            # NORTH_ORCHESTRATION_HOME/scripts; provider-catalog.mjs reads the provider JSON
            # already installed above. No authoring or private material.
            cp scripts/selection-assessment.mjs scripts/provider-catalog.mjs $out/scripts/
            runHook postInstall
          '';
        };

        # The packaged TypeScript runtime needs the public SDK, North's direct
        # Zod dependency, and host-matched native packages for its UI/runtime.
        # Fetching those tarballs directly keeps each system's closure bounded
        # instead of prefetching every 200+ MB optional OS/architecture package
        # in npm's universal lockfile.
        sdkRuntimeDependencies = pkgs.stdenvNoCC.mkDerivation {
          pname = "north-sdk-runtime-dependencies";
          version = sdkVersion;
          dontUnpack = true;
          nativeBuildInputs = [ pkgs.gnutar pkgs.gzip ];
          installPhase = ''
            runHook preInstall
            mkdir -p \
              $out/node_modules/@anthropic-ai/claude-agent-sdk \
              $out/node_modules/${sdkPlatform.packageName} \
              $out/node_modules/zod
            tar -xzf ${sdkSource} --strip-components=1 \
              -C $out/node_modules/@anthropic-ai/claude-agent-sdk
            tar -xzf ${sdkPlatformSource} --strip-components=1 \
              -C $out/node_modules/${sdkPlatform.packageName}
            tar -xzf ${zodSource} --strip-components=1 \
              -C $out/node_modules/zod
            ${lib.concatMapStringsSep "\n" (pkg: ''
              mkdir -p "$out/node_modules/${pkg.packageName}"
              tar -xzf ${pkg.source} --strip-components=1 \
                -C "$out/node_modules/${pkg.packageName}"
            '') opentuiRuntimeSources}
            chmod +x $out/node_modules/${sdkPlatform.packageName}/claude
            runHook postInstall
          '';
        };

        # The checkout launcher and immutable package have one runtime contract.
        # Keep the contract data-only here so neither can drift from the other.
        storeRpcEnvironment = "/home/tom/.local/state/north/beagle-store.env";
        northRuntimeVariables = {
          NORTH_ORCHESTRATION_HOME = orchestrationContract;
          NORTH_BB = "${pkgs.babashka}/bin/bb";
          NORTH_BUN = "${pkgs.bun}/bin/bun";
          NORTH_GIT_BIN = "${pkgs.git}/bin/git";
          NORTH_MKFIFO_BIN = "${pkgs.coreutils}/bin/mkfifo";
          NORTH_PEER_BB = "${pkgs.babashka}/bin/bb";
          NORTH_MCP_BB = "${pkgs.babashka}/bin/bb";
          NORTH_MCP_BUN = "${pkgs.bun}/bin/bun";
          NORTH_MANAGED_CODEX_BIN = "${codexPkg}/bin/codex";
          NORTH_PACKAGE_MODE = "nix-store";
          NORTH_PACKAGE_REV = builtins.substring 0 12 (self.rev or self.dirtyRev or "dirty");
        };
        northWrapperArgs = variables:
          [ "--prefix" "PATH" ":" runtimePath ]
          ++ lib.concatMap
            (name: [ "--set" name variables.${name} ])
            (builtins.attrNames variables);
        northRuntimeExports = lib.concatStringsSep "\n"
          (map (name: "export ${name}=${lib.escapeShellArg northRuntimeVariables.${name}}")
            (builtins.attrNames northRuntimeVariables));
        northEnv = pkgs.symlinkJoin {
          name = "north-env";
          paths = runtimePackages;
          postBuild = ''
            mkdir -p $out/sdk
            ln -s ${sdkRuntimeDependencies}/node_modules $out/sdk/node_modules
            cat > $out/bin/north-env <<'EOF'
#!${pkgs.bash}/bin/bash
set -euo pipefail

${northRuntimeExports}

source ${storeRpcEnvironment}

export NORTH_HOME="''${NORTH_HOME:-$PWD}"
export NORTH_BIN="''${NORTH_BIN:-$NORTH_HOME/bin/north}"
exec "$NORTH_BIN" "$@"
EOF
            chmod +x $out/bin/north-env
          '';
          meta = {
            description = "North checkout runtime environment without application code";
            mainProgram = "north-env";
          };
        };

        # north CLI + MCP. Same relocatable layout. Installed entrypoints source
        # the host-published FRAMRPC identity; North does not package or select a
        # second engine. NORTH_BIN points MCP at the wrapped CLI in this out.
        northPkg = pkgs.stdenvNoCC.mkDerivation {
          pname = "north";
          version = "0.1.0";
          # Keep the package derivation tied only to files copied into the
          # runtime. Archived web sources, tests, and docs cannot invalidate or
          # leak into the closure.
          src = runtimeSource;
          # Babashka must be present while patchShebangs runs. Otherwise the
          # copied `#!/usr/bin/env bb` survives into `.north-mcp-wrapped`, where
          # the Nix build sandbox has no `/usr/bin/env` to execute.
          nativeBuildInputs = [
            pkgs.makeWrapper
            pkgs.babashka
            # The install-phase smoke checks exercise the packaged CLI directly,
            # which reaches its TypeScript surface through bun.
            pkgs.bun
            pkgs.python3
            pkgs.ripgrep
          ];
          dontConfigure = true;
          dontBuild = true;
          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/contracts $out/out $out/sdk \
              $out/profiles/tom/hooks/lib $out/share
            cp -r out/. $out/out/
            # bin/north resolves its card + topic pages under $NORTH/share/help;
            # unshipped, the packaged CLI exits 1 on every `north help`.
            cp -r share/help $out/share/help
            cp contracts/agent-run-ledger-v2.json $out/contracts/
            cp profiles/tom/hooks/lib/harness-dial.sh \
              $out/profiles/tom/hooks/lib/
            # bb-verb CLIs (agents/watch/trace/health/dashboard/config/...)
            # route through $root/cli — without this every non-engine verb dies
            # on the packaged binary with "File does not exist: .../cli/*.clj".
            cp -r cli $out/cli
            test ! -e "$out/cli/tests"
            # Package the complete TypeScript runtime tree. Hand-maintained
            # transitive import lists inevitably rot as provider adapters grow.
            cp -r sdk/src $out/sdk/src
            ln -s ${sdkRuntimeDependencies}/node_modules $out/sdk/node_modules
            cp bin/north bin/north-comms bin/north-mcp bin/north-actor-key \
              bin/north-mark-delegated bin/north-on-spawn bin/north-on-stop \
              bin/north-on-tooluse \
              bin/north-stream-sync bin/north-stream-sync-all \
              bin/north-succession \
              bin/docctl bin/concern bin/ensure-private-docs \
              $out/bin/
            patchShebangs $out/bin

            # The Linear route is spread across these load-bearing runtime
            # modules. Catch untracked/omitted flake sources before producing a
            # package whose `north linear` verb points at a missing entrypoint.
            for f in cli.ts north-state.ts app-server-broker.ts \
              reserve-link.clj reserve-schema-fact.clj \
              find-bootstrap-links.clj; do
              test -f "$out/sdk/src/integrations/linear/$f"
            done
            test -f "$out/sdk/src/strict-json.ts"

            wrapProgram $out/bin/north \
              ${lib.escapeShellArgs (northWrapperArgs northRuntimeVariables)} \
              --run ${lib.escapeShellArg "source ${storeRpcEnvironment}"} \
              --set NORTH_HOME "$out" \
              --set NORTH_BIN "$out/bin/north"

            wrapProgram $out/bin/north-mcp \
              --prefix PATH : ${runtimePath} \
              --run ${lib.escapeShellArg "source ${storeRpcEnvironment}"} \
              --set NORTH_ORCHESTRATION_HOME ${orchestrationContract} \
              --set NORTH_HOME $out \
              --set NORTH_BIN $out/bin/north \
              --set NORTH_BB ${pkgs.babashka}/bin/bb \
              --set NORTH_BUN ${pkgs.bun}/bin/bun \
              --set NORTH_GIT_BIN ${pkgs.git}/bin/git \
              --set NORTH_MKFIFO_BIN ${pkgs.coreutils}/bin/mkfifo \
              --set NORTH_PEER_BB ${pkgs.babashka}/bin/bb \
              --set NORTH_MCP_BB ${pkgs.babashka}/bin/bb \
              --set NORTH_MCP_BUN ${pkgs.bun}/bin/bun \
              --set NORTH_MANAGED_CODEX_BIN ${codexPkg}/bin/codex

            wrapProgram $out/bin/north-comms \
              --prefix PATH : ${runtimePath} \
              --set NORTH_HOME $out \
              --set NORTH_BB ${pkgs.babashka}/bin/bb

            for hook in north-mark-delegated north-on-stop; do
              wrapProgram "$out/bin/$hook" \
                --prefix PATH : ${runtimePath} \
                --set NORTH_HOME $out
            done

            for hook in north-on-spawn north-on-tooluse; do
              wrapProgram "$out/bin/$hook" \
                --prefix PATH : ${runtimePath} \
                --run ${lib.escapeShellArg "source ${storeRpcEnvironment}"} \
                --set NORTH_HOME $out
            done

            wrapProgram $out/bin/north-stream-sync \
              --prefix PATH : ${runtimePath} \
              --set NORTH_PACKAGE_MODE nix-store

            wrapProgram $out/bin/north-stream-sync-all \
              --prefix PATH : ${runtimePath}

            wrapProgram $out/bin/docctl \
              --prefix PATH : ${runtimePath}

            wrapProgram $out/bin/concern \
              --prefix PATH : ${runtimePath} \
              --run ${lib.escapeShellArg "source ${storeRpcEnvironment}"} \
              --set NORTH_HOME $out \
              --set NORTH_BB ${pkgs.babashka}/bin/bb

            wrapProgram $out/bin/ensure-private-docs \
              --prefix PATH : ${runtimePath} \
              --set NORTH_HOME $out

            impurity_pattern='/(home|Users)/|/run/current-system/sw|/code/north(?:/|$|[^[:alnum:]_.-])|~/code/north|[$]HOME/code/north|[.]m2|[.]cpcache|[.]cache/babashka'
            # Audited exceptions to the store-external scan, and only these:
            # (1) sdk/src/trusted-runtime.ts's NixOS entry-hint pointers
            # /run/current-system/sw/bin/{git,bb,codex,mkfifo}. They are root-managed runtime
            # symlinks, NOT baked store paths — trustedStoreExecutable() still
            # forces each to canonicalize (realpathSync) into the immutable
            # /nix/store and be executable, so they never widen trust. They are
            # required because managed spawns don't always inherit the wrapper's
            # NORTH_GIT_BIN / NORTH_BB. The exemption is line-exact: any other
            # path in that same file, any other system-profile target, and every
            # match in every other file stays fatal.
            # (2) bin/{north,concern}'s fixed bb fallback. Packaged launchers
            # receive NORTH_BB from their wrappers; promoted checkout launchers
            # retain this root-managed entry hint for units with a minimal PATH.
            # Only the three exact fallback expressions in each wrapped launcher
            # are exempt.
            # (3) The installed North entrypoints source the one host-published
            # FRAMRPC identity file. It is data authority, not executable code.
            sanctioned='(^|/)sdk/src/trusted-runtime\.ts:[0-9]+:[[:space:]]*"/run/current-system/sw/bin/(git|bb|codex|mkfifo)",$|(^|/)bin/[.]north-wrapped:[0-9]+:[[:space:]]*(elif \[ -x /run/current-system/sw/bin/bb \]; then|BB="/run/current-system/sw/bin/bb"|echo "north: cannot find babashka — tried \\[$]NORTH_BB, PATH, /run/current-system/sw/bin/bb" >&2)$|(^|/)bin/[.]concern-wrapped:[0-9]+:[[:space:]]*(elif \[ -x /run/current-system/sw/bin/bb \]; then|BB="/run/current-system/sw/bin/bb"|echo "concern: cannot find babashka — tried \\[$]NORTH_BB, PATH, /run/current-system/sw/bin/bb" >&2)$|(^|/)bin/(north|north-mcp|concern|north-on-spawn|north-on-tooluse):[0-9]+:source /home/tom/[.]local/state/north/beagle-store[.]env$'
            residual=$(LC_ALL=C rg --hidden -n "$impurity_pattern" "$out" \
              | LC_ALL=C rg -v "$sanctioned" || true)
            if [ -n "$residual" ]; then
              printf '%s\n' "$residual" >&2
              echo "north package contains a checkout/home/cache path" >&2
              exit 1
            fi

            # Exercise every packaged TypeScript CLI entrypoint with hermetic
            # subscription/auth fixtures. These probes never make a model turn.
            smoke=$(mktemp -d)
            export BEAGLE_STORE_HOME="$smoke/beagle-store"
            export BEAGLE_STORE_BIN="$BEAGLE_STORE_HOME/bin"
            export BEAGLE_STORE_OUT="$BEAGLE_STORE_HOME/out"
            ${pkgs.coreutils}/bin/env -i \
              HOME="$smoke/poison-home" \
              NORTH_HOME="$out" \
              SPAWN_MODULE="$out/sdk/src/spawn.ts" \
              WORKTREE_MODULE="$out/sdk/src/worktree.ts" \
              EXPECTED_NORTH_BIN="$out/bin/north" \
              ${pkgs.bun}/bin/bun -e '
                await import(process.env.SPAWN_MODULE);
                const module = await import(process.env.WORKTREE_MODULE);
                const actual = module.worktreeNorthExecutable(process.env);
                if (actual !== process.env.EXPECTED_NORTH_BIN)
                  throw new Error("packaged worktree North CLI mismatch: " + actual);
              '
            cleanup_smoke() {
              rm -rf "$smoke"
            }
            trap cleanup_smoke EXIT
            mkdir -p "$smoke/bin" "$smoke/home"
            cat > "$smoke/bin/claude" <<'EOF'
#!${pkgs.bash}/bin/bash
if [ "$1" = "--version" ]; then echo 'claude smoke'; exit 0; fi
if [ "$1 $2 $3" = "auth status --json" ]; then echo '{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty"}'; exit 0; fi
exit 2
EOF
            cat > "$smoke/bin/codex" <<'EOF'
#!${pkgs.bash}/bin/bash
if [ "$1" = "--version" ]; then echo 'codex smoke'; exit 0; fi
if [ "$1 $2" = "login status" ]; then echo 'Logged in using ChatGPT'; exit 0; fi
exit 2
EOF
            chmod +x "$smoke/bin/claude" "$smoke/bin/codex"
            # The managed OpenAI surface is an exact nixpkgs-master package,
            # never the ambient PATH fixture used by account/auth probes below.
            test -e ${codexVersionSmoke}
            expected_codex_export="export NORTH_MANAGED_CODEX_BIN='${codexPkg}/bin/codex'"
            expected_mkfifo_export="export NORTH_MKFIFO_BIN='${pkgs.coreutils}/bin/mkfifo'"
            for wrapper in "$out/bin/north" "$out/bin/north-mcp"; do
              test "$(grep -Fxc "$expected_codex_export" "$wrapper")" -eq 1
              test "$(grep -Fc 'NORTH_MANAGED_CODEX_BIN=' "$wrapper")" -eq 1
              test "$(grep -Fxc "$expected_mkfifo_export" "$wrapper")" -eq 1
              test "$(grep -Fc 'NORTH_MKFIFO_BIN=' "$wrapper")" -eq 1
              test "$(grep -Fxc 'source ${storeRpcEnvironment}' "$wrapper")" -eq 1
            done
            test "$(grep -Fxc 'source ${storeRpcEnvironment}' "$out/bin/concern")" -eq 1
            mkdir -p "$smoke/home/.local/state/north/threads"
            client_repo="$smoke/home/code/client/smoke/widget"
            mkdir -p "$client_repo"
            ${pkgs.git}/bin/git -C "$client_repo" init -q
            # Every public executable must work with no ambient PATH or checkout.
            ${pkgs.coreutils}/bin/env -i \
              HOME="$smoke/home" PATH= NORTH_HOME="$out" \
              BEAGLE_STORE_HOME="$BEAGLE_STORE_HOME" BEAGLE_STORE_BIN="$BEAGLE_STORE_BIN" \
              BEAGLE_STORE_OUT="$BEAGLE_STORE_OUT" \
              $out/bin/.north-wrapped help > "$smoke/help.out"
            grep -q 'north — coordinate work, agents, and time' "$smoke/help.out"
            ${pkgs.coreutils}/bin/env -i \
              HOME="$smoke/home" PATH= \
              $out/bin/ensure-private-docs "$client_repo"
            if ! grep -qxF 'docs/private/' "$client_repo/.gitignore"; then
              echo "north package smoke: ensure-private-docs did not install its exact ignore rule" >&2
              sed -n '1,120p' "$client_repo/.gitignore" >&2
              exit 1
            fi
            stream_src="$smoke/source with spaces/project"
            mkdir -p "$stream_src" "$smoke/xdg"
            printf '{"type":"package-stream-probe"}\n' \
              > "$stream_src/12345678-1234-1234-1234-123456789abc.jsonl"
            ${pkgs.coreutils}/bin/env -i \
              HOME="$smoke/home" XDG_STATE_HOME="$smoke/xdg" PATH= \
              $out/bin/north-stream-sync \
                --src-dir "$smoke/source with spaces" \
                --provider anthropic --source-namespace package-smoke \
                --layout claude
            stream_raw="$smoke/xdg/north/streams/raw"
            stream_dest="$(${pkgs.findutils}/bin/find "$stream_raw" -maxdepth 1 \
              -type f -name '*.jsonl' -print -quit)"
            test -n "$stream_dest"
            ${pkgs.diffutils}/bin/cmp \
              "$stream_src/12345678-1234-1234-1234-123456789abc.jsonl" \
              "$stream_dest"
            cursor_file="$(${pkgs.findutils}/bin/find "$stream_raw" -maxdepth 1 \
              -type f -name '.cursors.v4.*' -print -quit)"
            test -n "$cursor_file"
            cursor_hash="$(${pkgs.coreutils}/bin/sha256sum "$cursor_file")"
            ${pkgs.coreutils}/bin/env -i \
              HOME="$smoke/home" XDG_STATE_HOME="$smoke/xdg" PATH= \
              $out/bin/north-stream-sync \
                --src-dir "$smoke/source with spaces" \
                --provider anthropic --source-namespace package-smoke \
                --layout claude
            test "$cursor_hash" = \
              "$(${pkgs.coreutils}/bin/sha256sum "$cursor_file")"
            ${pkgs.diffutils}/bin/cmp \
              "$stream_src/12345678-1234-1234-1234-123456789abc.jsonl" \
              "$stream_dest"
            stream_all_state="$smoke/all-state"
            stream_all_src="$stream_all_state/accounts/openai/package-account/sessions/2026/07/29"
            stream_all_raw="$smoke/all-raw"
            mkdir -p "$stream_all_src"
            printf '{"type":"package-codex-stream-probe"}\n' \
              > "$stream_all_src/rollout-2026-07-29T00-00-00-package.jsonl"
            ${pkgs.coreutils}/bin/env -i \
              HOME="$smoke/home" PATH= NORTH_STATE_ROOT="$stream_all_state" \
              NORTH_AMBIENT_CODEX_HOME="$smoke/no-ambient-codex" \
              $out/bin/north-stream-sync-all --raw-dir "$stream_all_raw"
            stream_all_dest="$(${pkgs.findutils}/bin/find "$stream_all_raw" \
              -maxdepth 1 -type f -name '*.jsonl' -print -quit)"
            test -n "$stream_all_dest"
            ${pkgs.diffutils}/bin/cmp \
              "$stream_all_src/rollout-2026-07-29T00-00-00-package.jsonl" \
              "$stream_all_dest"
            test ! -e "$out/streams/raw"
            # Import the public SDK and prove npm selected an executable native
            # Claude binary for this exact Nix system. This resolves no account
            # and makes no model turn.
            (
              cd $out/sdk
              HOME="$smoke/home" ${pkgs.bun}/bin/bun -e \
                'import { query } from "@anthropic-ai/claude-agent-sdk";
                 import { constants, accessSync } from "node:fs";
                 import { createRequire } from "node:module";
                 import { dirname, resolve } from "node:path";
                 const require = createRequire(import.meta.url);
                 const manifest = require.resolve("${sdkPlatform.packageName}/package.json");
                 accessSync(resolve(dirname(manifest), "claude"), constants.X_OK);
                 if (typeof query !== "function") process.exit(1);'
            )
            now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
            reset=$(date -u -d '+1 hour' +%Y-%m-%dT%H:%M:%SZ)
            printf '{"version":1,"observations":[{"targetId":"openai","provider":"openai","observedAt":"%s","windows":[{"usedPercent":10,"resetsAt":"%s"}]}]}\n' "$now" "$reset" > "$smoke/observations.json"
            printf '{"policyVersion":"north-routing-pin-v1","issuedAt":"%s","expiresAt":"%s","reasonCode":"capability-requirement","detail":"package smoke validates the exact OpenAI route","pins":[{"kind":"provider","value":"openai"}]}\n' \
              "$now" "$reset" > "$smoke/openai-pin-evidence.json"
            HOME="$smoke/home" NORTH_CLAUDE_BIN="$smoke/bin/claude" NORTH_CODEX_BIN="$smoke/bin/codex" \
              NORTH_STAFFING_SOURCE=file \
              NORTH_HOME="$out" NORTH_ORCHESTRATION_HOME=${orchestrationContract} \
              NORTH_PROVIDER_OBSERVATIONS="$smoke/observations.json" \
              $out/bin/.north-wrapped providers --json > "$smoke/providers.json"
            ${pkgs.jq}/bin/jq -e \
              '([.providers[].targets[] | select(.id == "anthropic")][0] | .installed and .authenticated) and
               ([.providers[].targets[] | select(.id == "openai")][0] |
                 .installed and .authenticated and .headroom == "plenty")' \
              "$smoke/providers.json" > /dev/null
            HOME="$smoke/home" NO_COLOR=1 NORTH_STAFFING_SOURCE=file \
              NORTH_HOME="$out" NORTH_ORCHESTRATION_HOME=${orchestrationContract} \
              $out/bin/.north-wrapped spawn implementer probe \
              --provider openai --pin-evidence "@$smoke/openai-pin-evidence.json" \
              --ad-hoc --dry-run > "$smoke/spawn.out"
            grep -q 'grade=mid tier=standard' "$smoke/spawn.out"
            grep -q 'AGENT_ROLE=implementer' "$smoke/spawn.out"
            # Assessed dispatch must resolve Orchestration's canonical selection
            # validator from the packaged contract alone. The sandbox has no
            # external sibling Orchestration checkout, and the wrapper forces
            # NORTH_ORCHESTRATION_HOME at the runtime contract, so this exercises
            # the exact shape (stock verifier
            # composition + assessment sidecar, dry-run) that failed before
            # scripts/selection-assessment.mjs + provider-catalog.mjs were
            # packaged. The dry-run resolves the composition, admits the
            # assessment through the canonical validator, and stops — it makes
            # no worker, no provider turn, and no lane.
            printf '%s\n' '{"version":"minimum-sufficient-v1","signals":{"decisionOwnership":"none","seamScope":"none","errorExposure":"contained-reversible","oracleStrength":"judgment-only","foundationalImpact":"none","dependencyShape":"atomic-cohesive","reasoningShape":"multi-hypothesis"},"derived":{"minimumTier":"senior","minimumReasoning":"high","ruleCodes":["oracle-strength:judgment-only","reasoning-shape:multi-hypothesis"]},"selected":{"tier":"senior","reasoning":"high"}}' \
              > "$smoke/verifier-assessment.json"
            NORTH_ORCHESTRATION_HOME=${orchestrationContract} HOME="$smoke/home" NO_COLOR=1 \
              NORTH_STAFFING_SOURCE=file NORTH_HOME="$out" \
              NORTH_ORCHESTRATION_HOME=${orchestrationContract} \
              $out/bin/.north-wrapped spawn verifier probe \
              --assessment "@$smoke/verifier-assessment.json" --ad-hoc --dry-run \
              > "$smoke/assessed-spawn.out"
            grep -q 'grade=senior tier=senior reasoning=high role=verifier' "$smoke/assessed-spawn.out"
            grep -q 'AGENT_ROUTING_ASSESSMENT=RECORDED' "$smoke/assessed-spawn.out"
            grep -q '\[dry-run\] not executed' "$smoke/assessed-spawn.out"
            # Fail-closed is intact: a forged derived block is rejected THROUGH
            # the packaged canonical validator, never silently admitted.
            printf '%s\n' '{"version":"minimum-sufficient-v1","signals":{"decisionOwnership":"none","seamScope":"none","errorExposure":"contained-reversible","oracleStrength":"judgment-only","foundationalImpact":"none","dependencyShape":"atomic-cohesive","reasoningShape":"multi-hypothesis"},"derived":{"minimumTier":"senior","minimumReasoning":"high","ruleCodes":["forged"]},"selected":{"tier":"senior","reasoning":"high"}}' \
              > "$smoke/verifier-assessment-forged.json"
            if NORTH_ORCHESTRATION_HOME=${orchestrationContract} HOME="$smoke/home" NO_COLOR=1 \
                 NORTH_STAFFING_SOURCE=file NORTH_HOME="$out" \
                 NORTH_ORCHESTRATION_HOME=${orchestrationContract} \
                 $out/bin/.north-wrapped spawn verifier probe \
                 --assessment "@$smoke/verifier-assessment-forged.json" --ad-hoc --dry-run \
                 > "$smoke/assessed-forged.out" 2>&1; then
              echo "north package smoke: forged assessment was admitted" >&2
              cat "$smoke/assessed-forged.out" >&2
              exit 1
            fi
            grep -q 'canonical Orchestration validation' "$smoke/assessed-forged.out"
            # Runtime Orchestration reads must be hermetic: exercise exact provider/model
            # resolution against the packaged contract, with no sibling checkout.
            # File staffing source: the graph default needs a live coordinator,
            # which a sandboxed build never has.
            NORTH_ORCHESTRATION_HOME=${orchestrationContract} HOME="$smoke/home" \
              NORTH_STAFFING_SOURCE=file ${pkgs.bun}/bin/bun -e \
              'import { readFileSync } from "node:fs";
               import { resolveModelAlias, resolveModelDelta, resolveTier } from "'$out'/sdk/src/providers/catalog.ts";
               const route = resolveTier("openai", "frontier");
               const terra = resolveModelAlias("openai", "terra");
               const terraDelta = resolveModelDelta("openai", terra);
               const terraContents = terraDelta.kind === "calibrated" && terraDelta.absolutePath
                 ? readFileSync(terraDelta.absolutePath, "utf8") : "";
               const validTerraDelta = terraDelta.provider === "openai"
                 && terraDelta.model === "gpt-5.6-terra"
                 && terraDelta.kind === "calibrated"
                 && terraDelta.path === "docs/deltas/gpt-5.6-terra.md"
                 && terraContents.startsWith("# gpt-5.6-terra delta");
               const opus = resolveModelAlias("anthropic", "opus");
               const delta = resolveModelDelta("anthropic", opus);
               const validDelta = delta.provider === "anthropic" && delta.model === "claude-opus-5"
                 && (delta.kind === "calibrated"
                   ? Boolean(delta.path?.trim() && delta.absolutePath?.trim())
                   : delta.kind === "none" && Boolean(delta.reason?.trim()));
               if (route.model !== "gpt-5.6-sol" || terra !== "gpt-5.6-terra"
                 || !validTerraDelta || opus !== "claude-opus-5" || !validDelta) process.exit(1);'
            grep -q '^## distinguished$' ${orchestrationContract}/docs/task-grades.md
            grep -q '^## worker$' ${orchestrationContract}/docs/topologies.md
            grep -q '^## universal$' ${orchestrationContract}/docs/comms.md
            printf '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}\n' | \
              ${pkgs.coreutils}/bin/env -i HOME="$smoke/home" PATH= NORTH_HOME="$out" \
              BEAGLE_STORE_HOME="$BEAGLE_STORE_HOME" BEAGLE_STORE_BIN="$BEAGLE_STORE_BIN" \
              BEAGLE_STORE_OUT="$BEAGLE_STORE_OUT" \
              $out/bin/.north-mcp-wrapped > "$smoke/north-mcp-tools.json"
            ${pkgs.jq}/bin/jq -e \
              '([.result.tools[] | select(.name | startswith("linear_")) | .name] | sort) == ["linear_get", "linear_import", "linear_plan", "linear_sync"]' \
              "$smoke/north-mcp-tools.json" > /dev/null
            mkdir -p "$smoke/home/.config/north" \
              "$smoke/home/.local/state/north/accounts/anthropic/claude-smoke"
            printf '{"version":1,"mode":"balanced","targets":[{"id":"claude-smoke","provider":"anthropic","authMode":"isolated","profile":"claude-smoke"}],"targetOrder":["claude-smoke"]}\n' \
              > "$smoke/home/.config/north/routing-policy.json"
            printf '{"rate_limits":{"five_hour":{"used_percentage":10,"resets_at":4102444800}}}\n' | \
              HOME="$smoke/home" \
              CLAUDE_CONFIG_DIR="$smoke/home/.local/state/north/accounts/anthropic/claude-smoke" \
              NORTH_PROVIDER_OBSERVATIONS="$smoke/ingested.json" \
              NORTH_HOME="$out" NORTH_ORCHESTRATION_HOME=${orchestrationContract} \
              $out/bin/.north-wrapped provider-observe claude-statusline
            test -s "$smoke/ingested.json"
            runHook postInstall
          '';

          meta = with lib; {
            description = "north — fact-native work coordination CLI + MCP server";
            mainProgram = "north";
            platforms = [
              "x86_64-linux"
              "aarch64-linux"
              "aarch64-darwin"
            ];
          };
        };
      in {
        packages = {
          default = northPkg;
          north = northPkg;
          north-env = northEnv;
          # This is the exact derivation injected into managed OpenAI lanes;
          # Firn can install and attest the same executable without repackaging.
          codex = codexPkg;
        };

        checks = {
          codex-app-server-contract = codexAppServerContractSmoke;
          codex-version = codexVersionSmoke;
        } // lib.optionalAttrs (system == "x86_64-linux") {
          codex-managed-hook-failure = codexManagedHookFailureSmoke;
        };

        apps = {
          default = {
            type = "app";
            program = "${northPkg}/bin/north";
            meta.description = "North provider-neutral coordination CLI";
          };
          north = {
            type = "app";
            program = "${northPkg}/bin/north";
            meta.description = "North provider-neutral coordination CLI";
          };
          north-mcp = {
            type = "app";
            program = "${northPkg}/bin/north-mcp";
            meta.description = "North fact and coordination MCP server";
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # North CLI + MCP. Archived web sources are not part of the shell.
            babashka
          ];
        };
      });
}
