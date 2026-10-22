{
  description = "SpurXSecureSSH Flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      packages.${system}.default = pkgs.stdenv.mkDerivation {
        pname = "spurx-secure-ssh";
        version = "0.1.0";
        src = ./target/release;

        nativeBuildInputs = [ pkgs.makeWrapper ];

        installPhase = ''
          mkdir -p $out/bin
          cp spurx-secure-ssh $out/bin/

          # Bake the runtime libraries directly into the binary wrapper
          wrapProgram $out/bin/spurx-secure-ssh \
            --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath [
              pkgs.wayland
              pkgs.libxkbcommon
              pkgs.vulkan-loader
              pkgs.libGL
              pkgs.fontconfig
              pkgs.freetype
              pkgs.openssl
            ]}"
        '';
      };

      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [ pkg-config python3 ninja gn clang llvm rustc cargo ];
        buildInputs = with pkgs; [ openssl fontconfig freetype vulkan-loader wayland libxkbcommon libinput libGL glib gtk3 ];
      };
    };
}
