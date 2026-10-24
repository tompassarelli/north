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
    devShells.${system}.default = pkgs.mkShell {
      packages = [ rustToolchain pkgs.bun (pkgs.python3.withPackages (python: [ python.websockets ])) ];
      # Cargo hashes workspace members by relative path, so without this key another checkout's build into this target reads as fresh.
      shellHook = ''export RUSTFLAGS="-Cmetadata=checkout:$(git rev-parse --show-toplevel 2>/dev/null || pwd)"'';
    };
  }) (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml))) (import nixpkgs {
      system = system;
      overlays = [ rust-overlay.overlays.default ];
    }))) "x86_64-linux"));
}
