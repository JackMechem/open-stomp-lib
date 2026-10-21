{
  description = "open-stomp: Rust library for the SORAN Dual Stomp guitar pedal";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        lib = pkgs.lib;
        inherit (pkgs.stdenv.hostPlatform) isLinux;
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = lib.optionals isLinux [ pkgs.alsa-lib ];
          nativeBuildInputs =
            (with pkgs; [
              pkg-config
              cargo
              rustc
              rustfmt
              clippy
              rust-analyzer
            ])
            ++ lib.optionals isLinux [ pkgs.alsa-utils ]; # amidi / aseqdump for poking the pedal
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}
