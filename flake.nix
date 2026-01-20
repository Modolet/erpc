{
  description = "eRPC erpcgen dev shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          glibcStaticLib = "${pkgs.glibc.static}/lib";
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cmake
              ninja
              gnumake
              gcc13
              bison
              flex
              python3
              glibc.static
            ];

            BISON = "${pkgs.bison}/bin/bison";
            FLEX = "${pkgs.flex}/bin/flex";
            LIBRARY_PATH = glibcStaticLib;
            NIX_LDFLAGS = "-L${glibcStaticLib}";

            shellHook = ''
              export ERPC_ROOT="$PWD"
            '';
          };
        }
      );
    };
}
