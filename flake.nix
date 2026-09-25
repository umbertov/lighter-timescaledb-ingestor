{
  description = "Development shell and Docker image for lighter-timescaledb-ingestor";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

  outputs =
    {
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      system = "x86_64-linux";

      pkgs = import nixpkgs {
        inherit system;
        overlays = [ (import rust-overlay) ];
      };

      rustToolchain = pkgs.rust-bin.stable."1.98.1".minimal.override {
        extensions = [
          "clippy"
          "rust-analyzer"
          "rust-src"
          "rustfmt"
        ];
      };

      # Custom rustPlatform using your pinned rustToolchain
      rustPlatform = pkgs.makeRustPlatform {
        cargo = rustToolchain;
        rustc = rustToolchain;
      };

      # Statically linked Rust binary derivation
      app = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
        pname = "lighter-timescaledb-ingestor";
        version = "0.1.0";
        src = ./.;

        cargoLock = {
          lockFile = ./Cargo.lock;
        };

        nativeBuildInputs = [
          pkgs.pkg-config
          pkgs.cmake
          pkgs.perl
        ];

        buildInputs = [ pkgs.pkgsStatic.openssl ];

      };

    in
    {
      formatter.${system} = pkgs.nixfmt-tree;

      packages.${system} = {
        # Default package: compiled static executable
        default = app;

        # Docker image build
        docker = pkgs.dockerTools.buildLayeredImage {
          name = "lighter-timescaledb-ingestor";
          tag = "latest";

          config = {
            Cmd = [ "${app}/bin/lighter-timescaledb-rs" ];
            Env = [
              "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            ];
          };

          contents = [
            pkgs.cacert # Root CA certificates for outgoing DB/TLS connections
          ];
        };
      };

      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.diesel-cli
          pkgs.cmake
          pkgs.git
          pkgs.openssh
          pkgs.perl
          pkgs.pkg-config
          pkgs.postgresql_18
          pkgs.stdenv.cc
          rustToolchain
        ];

        CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER = "${pkgs.stdenv.cc}/bin/cc";
        RUSTFLAGS = "-C relocation-model=static";
      };
    };
}
