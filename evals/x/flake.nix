{
  description = "Host tools for Eval X: the container runtime and installer that Terminal-Bench 2's own harness runs on";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  outputs =
    { nixpkgs, ... }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
    in
    {
      devShells.x86_64-linux.default = pkgs.mkShell {
        packages = [
          pkgs.podman
          pkgs.podman-compose
          pkgs.uv
          pkgs.python3
          pkgs.git
          pkgs.jq
        ];
      };
    };
}
