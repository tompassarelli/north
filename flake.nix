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
      packages = [ (pkgs.runCommandLocal "capacity-gate-shims" { } "install -Dm755 ${./bin/cargo} $out/bin/cargo; install -Dm755 ${./bin/cargo} $out/bin/bun") rustToolchain pkgs.shellcheck pkgs.bun (pkgs.python3.withPackages (python: [ python.websockets ])) ];
      # Cargo hashes workspace members by relative path, so without this key another checkout's build into this target reads as fresh.
      shellHook = ''export RUSTFLAGS="-Cmetadata=checkout:$(git rev-parse --show-toplevel 2>/dev/null || pwd)"'';
    };
  }) (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml))) (import nixpkgs {
      system = system;
      overlays = [ rust-overlay.overlays.default ];
    }))) "x86_64-linux"));
}
