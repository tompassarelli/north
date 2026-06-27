{
  description = "lodestar — claim-native work coordination";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Elixir / Erlang
            elixir
            beamPackages.erlang
            hex
            rebar3

            # native deps for Phoenix/Hologram
            inotify-tools
            nodejs_22

            # existing lodestar deps
            babashka
            bun
          ];

          shellHook = ''
            export MIX_HOME="$PWD/.mix"
            export HEX_HOME="$PWD/.hex"
            export PATH="$MIX_HOME/bin:$HEX_HOME/bin:$PATH"
          '';
        };
      });
}
