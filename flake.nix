{
  description = "North development environment";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs = (
    { nixpkgs, rust-overlay, ... }:
    (
      (
        system:
        (
          (
            pkgs:
            (
              (
                rustToolchain:
                let
                  devPackages = [
                    (pkgs.runCommandLocal "capacity-gate-shims" { }
                      "install -Dm755 ${./bin/cargo} $out/bin/cargo; install -Dm755 ${./bin/cargo} $out/bin/bun"
                    )
                    rustToolchain
                    pkgs.buck2
                    pkgs.shellcheck
                    pkgs.cargo-deny
                    pkgs.osv-scanner
                    pkgs.cargo-mutants
                    pkgs.cargo-nextest
                    pkgs.libfaketime
                    pkgs.rust-analyzer
                    # The release firn's bunRelease installs on every host, so tests here run the bun the north services run.
                    (pkgs.bun.overrideAttrs (_old: {
                      version = "1.4.4";
                      src = pkgs.fetchurl {
                        url = "https://github.com/oven-sh/bun/releases/download/bun-v1.4.4/bun-linux-x64-baseline.zip";
                        hash = "sha256-4ERutVmVdy65iUvrrppr42jZhai5Jx7hJEhvmUbu3XY=";
                      };
                    }))
                    pkgs.openssh
                    pkgs.age
                    pkgs.ast-grep
                    (pkgs.python3.withPackages (python: [ python.websockets ]))
                  ];
                  # Absolute store paths, so each Buck2 action's key names the exact toolchain that runs it.
                  buck2Toolchain = pkgs.writeText "north-buck2-toolchain.buckconfig" ''
                    [nix]
                      rustc = ${rustToolchain}/bin/rustc
                      rustdoc = ${rustToolchain}/bin/rustdoc
                      clippy_driver = ${rustToolchain}/bin/clippy-driver
                      cc = ${pkgs.clang}/bin/clang
                      cxx = ${pkgs.clang}/bin/clang++
                      ar = ${pkgs.stdenv.cc.bintools.bintools}/bin/ar
                      python3 = ${pkgs.python3}/bin/python3
                    [north]
                      test_path = ${
                        pkgs.lib.makeBinPath (devPackages ++ [ pkgs.git ] ++ pkgs.stdenv.initialPath)
                      }:/run/wrappers/bin:/run/current-system/sw/bin
                  '';
                  setup = pkgs.writeShellApplication {
                    name = "north-setup";
                    runtimeInputs = [ pkgs.git ];
                    text = ''
                      checkout="$HOME/code/north/main"
                      say() { printf 'setup: %s\n' "$*"; }
                      fail() { say "$1 failed: $2" >&2; exit 1; }
                      if [ -d "$checkout/.git" ]; then
                        say "1/3 use the north checkout at $checkout"
                      else
                        say "1/3 clone north into $checkout"
                        mkdir -p "$(dirname "$checkout")"
                        git clone https://github.com/tompassarelli/north "$checkout" || fail "the clone" "check the network and that $checkout is free, then run this again"
                      fi
                      cd "$checkout"
                      say "2/3 build north and verify-north; the first build downloads the toolchain and takes minutes"
                      nix develop -c cargo build --locked --bin north --bin verify-north || fail "the build" "the cargo output above names the cause; run this again after fixing it"
                      say "3/3 make the claude-main account from Claude Code's login and have a worker land one change"
                      target/debug/verify-north --north "$checkout/target/debug/north" --claude-account main --from-claude-login || fail "verify-north" "its FAIL line above names the step and the cause; run this again after fixing it"
                      say "done: a worker's change landed; run $checkout/target/debug/north to open the fleet screen"
                    '';
                  };
                in
                {
                  formatter.${system} = pkgs.nixfmt-tree;
                  apps.${system}.setup = {
                    type = "app";
                    program = "${setup}/bin/north-setup";
                  };
                  packages.${system} = {
                    buck2-toolchain = buck2Toolchain;
                    # Off the dev shell so the laptop never compiles it; third-party/reindeer.toml says how to run it.
                    reindeer = pkgs.reindeer.overrideAttrs (old: {
                      patches = (old.patches or [ ]) ++ [ ./third-party/reindeer-workspace-dev-dependencies.patch ];
                    });
                  };
                  devShells.${system}.default = pkgs.mkShell rec {
                    packages = devPackages;
                    # Cargo hashes workspace members by relative path, so a target outside this checkout is keyed to it lest another checkout's build read as fresh; the checkout's own target stays unkeyed so registry artifacts match its siblings', and `north capacity dev-shell run` and depbox seed that target before the shell opens.
                    shellHook = ''
                      checkout=$PWD
                      while [ ! -e "$checkout/.git" ] && [ "$checkout" != / ]; do checkout=$(dirname "$checkout"); done
                      [ -e "$checkout/.git" ] || checkout=$PWD
                      if [ -n "$CARGO_TARGET_DIR" ] && [ "$(realpath -m "$CARGO_TARGET_DIR")" != "$(realpath -m "$checkout/target")" ]; then export RUSTFLAGS="-Cmetadata=checkout:$checkout"; fi
                      export PATH=${builtins.head packages}/bin:$PATH
                      if [ -e "$checkout/.buckconfig" ] && [ "$(readlink "$checkout/.buckconfig.flake")" != ${buck2Toolchain} ]; then ln -sfn ${buck2Toolchain} "$checkout/.buckconfig.flake"; fi
                    '';
                  };
                }
              )
                (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
            )
          )
            (
              import nixpkgs {
                system = system;
                overlays = [ rust-overlay.overlays.default ];
              }
            )
        )
      )
        "x86_64-linux"
    )
  );
}
