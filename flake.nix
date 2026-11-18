{
  description = "Native Rust multi-account switcher and real-time quota dashboard for Google Antigravity";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system: f (import nixpkgs { inherit system; }));
    in
    {
      packages = forAllSystems (pkgs: rec {
        agy-auth-rust = pkgs.rustPlatform.buildRustPackage {
          pname = "agy-auth-rust";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.dbus ];

          meta = with pkgs.lib; {
            description = "Native Rust multi-account switcher and quota dashboard for Google Antigravity";
            homepage = "https://github.com/deactivated0/agy-auth-rust";
            license = licenses.mit;
            mainProgram = "agy-auth-rust";
          };
        };
        default = agy-auth-rust;
      });

      apps = forAllSystems (pkgs: rec {
        agy-auth-rust = {
          type = "app";
          program = "${self.packages.${pkgs.system}.agy-auth-rust}/bin/agy-auth-rust";
        };
        agy-switch = {
          type = "app";
          program = "${self.packages.${pkgs.system}.agy-auth-rust}/bin/agy-switch";
        };
        default = agy-auth-rust;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [
            pkgs.cargo
            pkgs.rustc
            pkgs.clippy
            pkgs.rustfmt
          ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.dbus ];
        };
      });
    };
}
