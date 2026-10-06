{
  description = "YAAS portable Linux build tooling";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      buildPortable = pkgs.writeShellApplication {
        name = "yaas-portable-build";
        runtimeInputs = with pkgs; [ bash coreutils docker git python3 ];
        text = ''
          exec bash ${./scripts/build_portable_nix.sh} "$@"
        '';
      };
    in {
      apps.${system} = rec {
        portable-build = {
          type = "app";
          program = "${buildPortable}/bin/yaas-portable-build";
        };
        default = portable-build;
      };
      packages.${system}.default = buildPortable;
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [ docker git just python3 shellcheck ];
      };
      checks.${system}.portable-build-script = pkgs.runCommand "portable-build-script-check" {
        nativeBuildInputs = [ pkgs.shellcheck ];
      } ''
        shellcheck ${./scripts/build_portable_nix.sh}
        touch "$out"
      '';
    };
}
