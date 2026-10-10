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
      packages = [ rustToolchain pkgs.bun ];
    };
  }) (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml))) (import nixpkgs {
      system = system;
      overlays = [ rust-overlay.overlays.default ];
    }))) "x86_64-linux"));
}
