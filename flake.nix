{
  description = "North development environment";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs = ({ nixpkgs, rust-overlay, ... }: ((system: ((pkgs: ((rustToolchain: {
    packages.${system}.shellcheck = pkgs.shellcheck;
    devShells.${system}.default = pkgs.mkShell {
      packages = [ (pkgs.runCommandLocal "capacity-gate-shims" { } "install -Dm755 ${./bin/cargo} $out/bin/cargo; install -Dm755 ${./bin/cargo} $out/bin/bun") rustToolchain pkgs.shellcheck pkgs.cargo-mutants pkgs.bun (pkgs.python3.withPackages (python: [ python.websockets ])) ];
      # Cargo hashes workspace members by relative path, so a target outside this checkout is keyed to it lest another checkout's build read as fresh; the checkout's own target stays unkeyed so registry artifacts match its siblings', and `north target-seed` drops the seeded packages' fingerprints instead.
      shellHook = ''
        checkout=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
        if [ -n "$CARGO_TARGET_DIR" ] && [ "$(realpath -m "$CARGO_TARGET_DIR")" != "$(realpath -m "$checkout/target")" ]; then export RUSTFLAGS="-Cmetadata=checkout:$checkout"; fi
        north target-seed || true
      '';
    };
  }) (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml))) (import nixpkgs {
      system = system;
      overlays = [ rust-overlay.overlays.default ];
    }))) "x86_64-linux"));
}
