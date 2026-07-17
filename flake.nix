{
  description = "north — fact-native work coordination (CLI + MCP, on babashka)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    # The Fram engine is north's runtime library: bin/north puts
    # $FRAM/out on the bb classpath (fram.kernel/fold/import/export/rt) and
    # shells $FRAM/bin/fram for engine verbs. Fram ships its compiled Clojure
    # in out/ (committed, runs on bare bb — no Beagle at runtime), so we consume
    # it as a plain source tree (flake = false) and wrap it the same way.
    # Pinned via this flake's lock; bump with `nix flake update fram`. (Must be a
    # fetchable URL, never a local path — a path: leaks the author's machine into
    # the published flake and breaks every other consumer + CI.)
    fram = {
      url = "github:tompassarelli/fram";
      flake = false;
    };
    gaffer = {
      url = "github:tompassarelli/gaffer";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, flake-utils, fram, gaffer }:
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

        # Runtime PATH for the bb-backed CLIs. util-linux supplies `setsid` for
        # managed lanes on every supported host. iproute2 supplies Linux's `ss`
        # for daemon-health probes, but has no Darwin package; keep that helper
        # out of Darwin's derivation instead of admitting an unsupported closure.
        runtimePackages = [
          pkgs.babashka
          pkgs.coreutils
          pkgs.bash
          pkgs.bun
          pkgs.util-linux
        ] ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [
          pkgs.iproute2
        ];
        runtimePath = lib.makeBinPath runtimePackages;
        sdkPlatform =
          if pkgs.stdenv.hostPlatform.isLinux then
            if pkgs.stdenv.hostPlatform.isx86_64 then
              {
                packageName = "@anthropic-ai/claude-agent-sdk-linux-x64";
                url = "https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk-linux-x64/-/claude-agent-sdk-linux-x64-${sdkVersion}.tgz";
                hash = "sha512-s1lNi1cL93luoqsItH+fNO4KpIhdkvnVhWGGQUQ/8ftwa2gfmcIQnOg1hG8Ks+KzeD3UUQ8L9YEVHVADnFI/9A==";
              }
            else if pkgs.stdenv.hostPlatform.isAarch64 then
              {
                packageName = "@anthropic-ai/claude-agent-sdk-linux-arm64";
                url = "https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk-linux-arm64/-/claude-agent-sdk-linux-arm64-${sdkVersion}.tgz";
                hash = "sha512-JuIq5Fnz/F1snl0aqi1gcuRZqPWoPNrL9dJ0DuievCxKkO8hnEz/Mmn5Zos7x1X8HE//ZnEvmQXoEQEZXonJew==";
              }
            else throw "North's Claude SDK package does not support ${system}"
          else if pkgs.stdenv.hostPlatform.isDarwin then
            if pkgs.stdenv.hostPlatform.isAarch64 then
              {
                packageName = "@anthropic-ai/claude-agent-sdk-darwin-arm64";
                url = "https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk-darwin-arm64/-/claude-agent-sdk-darwin-arm64-${sdkVersion}.tgz";
                hash = "sha512-WIMM/8HRCLsTDHFTIwQvvE8WCA/oaMJtdQxsP7iNyfzIGwXbuOyU95V8vYIhZfaO2yaSpbBRncunq4CtR5H4ng==";
              }
            else throw "North's Claude SDK package does not support ${system}"
          else throw "North's Claude SDK package does not support ${system}";
        sdkSource = pkgs.fetchurl {
          url = "https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/-/claude-agent-sdk-${sdkVersion}.tgz";
          hash = "sha512-FVmXu9pvOMbuBKWrF8YsYQdQ/upOpv5rS8lFAnFO5jbyXT/2hN7kEPd2vd2GJpaMvNcO/KptyQUK5AxjjTz3+w==";
        };
        sdkPlatformSource = pkgs.fetchurl {
          inherit (sdkPlatform) url hash;
        };

        # Fram engine packaged as a relocatable tree: $out/out (classpath) +
        # $out/bin/{fram,fram-up}. Each bin script does `dirname "$0"/..` to find
        # its repo root, so preserving the bin/ + out/ layout keeps that working;
        # wrapProgram only injects bb (+ daemon tools) onto PATH.
        framPkg = pkgs.stdenvNoCC.mkDerivation {
          pname = "fram-engine";
          version = builtins.substring 0 12 (fram.rev or "local");
          src = fram;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          dontConfigure = true;
          dontBuild = true;
          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/out
            cp -r out/. $out/out/
            for f in fram fram-up fram-daemon; do
              [ -f bin/$f ] && cp bin/$f $out/bin/$f
            done
            for f in $out/bin/*; do
              wrapProgram "$f" --prefix PATH : ${runtimePath}
            done
            runHook postInstall
          '';
        };

        # Runtime-only Gaffer contract. Generated adapters, authoring scripts,
        # skills, and private docs stay out of North's closure.
        gafferContract = pkgs.stdenvNoCC.mkDerivation {
          pname = "gaffer-runtime-contract";
          version = builtins.substring 0 12 (gaffer.rev or "local");
          src = gaffer;
          dontConfigure = true;
          dontBuild = true;
          installPhase = ''
            runHook preInstall
            mkdir -p $out/staffing $out/providers $out/docs/deltas
            cp staffing/catalog.json $out/staffing/
            cp providers/anthropic.json providers/openai.json $out/providers/
            cp docs/roles.md docs/task-grades.md docs/topologies.md docs/postures.md docs/comms.md $out/docs/
            cp docs/deltas/opus.md docs/deltas/sonnet.md $out/docs/deltas/
            runHook postInstall
          '';
        };

        # sdk.mjs is self-contained; at runtime it needs only the public package
        # plus the exact native Claude binary for this host. Fetching those two
        # tarballs directly keeps each system's closure bounded instead of
        # prefetching every 200+ MB optional OS/architecture package in npm's
        # universal lockfile.
        sdkRuntimeDependencies = pkgs.stdenvNoCC.mkDerivation {
          pname = "north-sdk-runtime-dependencies";
          version = sdkVersion;
          dontUnpack = true;
          nativeBuildInputs = [ pkgs.gnutar pkgs.gzip ];
          installPhase = ''
            runHook preInstall
            mkdir -p \
              $out/node_modules/@anthropic-ai/claude-agent-sdk \
              $out/node_modules/${sdkPlatform.packageName}
            tar -xzf ${sdkSource} --strip-components=1 \
              -C $out/node_modules/@anthropic-ai/claude-agent-sdk
            tar -xzf ${sdkPlatformSource} --strip-components=1 \
              -C $out/node_modules/${sdkPlatform.packageName}
            chmod +x $out/node_modules/${sdkPlatform.packageName}/claude
            runHook postInstall
          '';
        };

        # north CLI + MCP. Same relocatable layout. FRAM_HOME is baked to the
        # packaged engine so the CLI is self-contained; an explicit env override
        # still wins (the script reads ${FRAM_HOME:-...}). NORTH_BIN points the
        # MCP server at the wrapped CLI in this same out.
        northPkg = pkgs.stdenvNoCC.mkDerivation {
          pname = "north";
          version = "0.1.0";
          src = self;
          # Babashka must be present while patchShebangs runs. Otherwise the
          # copied `#!/usr/bin/env bb` survives into `.north-mcp-wrapped`, where
          # the Nix build sandbox has no `/usr/bin/env` to execute.
          nativeBuildInputs = [ pkgs.makeWrapper pkgs.babashka ];
          dontConfigure = true;
          dontBuild = true;
          installPhase = ''
            runHook preInstall
            mkdir -p $out/bin $out/out $out/sdk
            cp -r out/. $out/out/
            # bb-verb CLIs (agents/watch/trace/health/dials/dashboard/config/...)
            # route through $root/cli — without this every non-engine verb dies
            # on the packaged binary with "File does not exist: .../cli/*.clj".
            cp -r cli $out/cli
            # Package the complete TypeScript runtime tree. Hand-maintained
            # transitive import lists inevitably rot as provider adapters grow.
            cp -r sdk/src $out/sdk/src
            ln -s ${sdkRuntimeDependencies}/node_modules $out/sdk/node_modules
            cp bin/north bin/north-mcp bin/concern $out/bin/
            patchShebangs $out/bin

            # The Linear route is spread across these load-bearing runtime
            # modules. Catch untracked/omitted flake sources before producing a
            # package whose `north linear` verb points at a missing entrypoint.
            for f in cli.ts north-state.ts app-server-broker.ts; do
              test -f "$out/sdk/src/integrations/linear/$f"
            done

            wrapProgram $out/bin/north \
              --prefix PATH : ${runtimePath} \
              --set-default FRAM_HOME ${framPkg} \
              --set-default GAFFER_HOME ${gafferContract} \
              --set-default NORTH_PACKAGE_MODE nix-store \
              --set-default NORTH_PACKAGE_REV ${builtins.substring 0 12 (self.rev or self.dirtyRev or "dirty")}

            wrapProgram $out/bin/north-mcp \
              --prefix PATH : ${runtimePath} \
              --set-default FRAM_HOME ${framPkg} \
              --set-default GAFFER_HOME ${gafferContract} \
              --set-default NORTH_BIN $out/bin/north

            # Exercise every packaged TypeScript CLI entrypoint with hermetic
            # subscription/auth fixtures. These probes never make a model turn.
            smoke=$(mktemp -d)
            trap 'rm -rf "$smoke"' EXIT
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
            HOME="$smoke/home" NORTH_CLAUDE_BIN="$smoke/bin/claude" NORTH_CODEX_BIN="$smoke/bin/codex" \
              NORTH_PROVIDER_OBSERVATIONS="$smoke/observations.json" $out/bin/north providers --json > "$smoke/providers.json"
            ${pkgs.jq}/bin/jq -e \
              '([.providers[].targets[] | select(.id == "anthropic")][0] | .installed and .authenticated) and
               ([.providers[].targets[] | select(.id == "openai")][0] |
                 .installed and .authenticated and .headroom == "plenty")' \
              "$smoke/providers.json" > /dev/null
            HOME="$smoke/home" NO_COLOR=1 $out/bin/north spawn implementer probe \
              --provider openai --dry-run > "$smoke/spawn.out"
            grep -q 'grade=mid tier=standard' "$smoke/spawn.out"
            grep -q 'AGENT_ROLE=implementer' "$smoke/spawn.out"
            # Runtime Gaffer reads must be hermetic: exercise exact provider/model
            # resolution against the packaged contract, with no sibling checkout.
            GAFFER_HOME=${gafferContract} HOME="$smoke/home" ${pkgs.bun}/bin/bun -e \
              'import { resolveModelAlias, resolveModelDelta, resolveTier } from "'$out'/sdk/src/providers/catalog.ts";
               const route = resolveTier("openai", "frontier");
               const opus = resolveModelAlias("anthropic", "opus");
               const delta = resolveModelDelta("anthropic", opus);
               const validDelta = delta.provider === "anthropic" && delta.model === "claude-opus-4-8"
                 && (delta.kind === "calibrated"
                   ? Boolean(delta.path?.trim() && delta.absolutePath?.trim())
                   : delta.kind === "none" && Boolean(delta.reason?.trim()));
               if (route.model !== "gpt-5.6-sol" || opus !== "claude-opus-4-8" || !validDelta) process.exit(1);'
            grep -q '^## research-grade$' ${gafferContract}/docs/task-grades.md
            grep -q '^## worker$' ${gafferContract}/docs/topologies.md
            grep -q '^## universal$' ${gafferContract}/docs/comms.md
            printf '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}\n' | \
              $out/bin/north-mcp > "$smoke/north-mcp-tools.json"
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
              $out/bin/north provider-observe claude-statusline
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
          fram-engine = framPkg;
        };

        apps = {
          default = {
            type = "app";
            program = "${northPkg}/bin/north";
          };
          north = {
            type = "app";
            program = "${northPkg}/bin/north";
          };
          north-mcp = {
            type = "app";
            program = "${northPkg}/bin/north-mcp";
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # north CLI + the bjs/Bun web cockpit (web-bjs/). The Elixir/Phoenix
            # app was retired 2026-07-10 (see web-v1-archive/); beagle is provided
            # system-wide by the nixos beagle module.
            babashka
            bun
          ];
        };
      });
}
