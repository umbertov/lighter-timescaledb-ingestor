{
  description = "Development shell for lighter-timescaledb-ingestor";

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
    in
    {
      formatter.${system} = pkgs.nixfmt-tree;

      devShells.${system}.default = pkgs.mkShell {
        packages = [
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
