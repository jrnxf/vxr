{
  description = "vxr dev shell: Rust with wasm32, wasm-bindgen, wasm-opt, twiggy, nightly for cargo-fuzz";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, rust-overlay, ... }:
    let
      systems = [ "aarch64-darwin" "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system:
        f (import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; }));
    in
    {
      devShells = forAll (pkgs:
        let
          stable = pkgs.rust-bin.stable.latest.default.override {
            targets = [ "wasm32-unknown-unknown" ];
            extensions = [ "rust-src" ];
          };
          nightly = pkgs.rust-bin.selectLatestNightlyWith (t: t.minimal);
          # wasm-bindgen-cli must match the wasm-bindgen crate version exactly (pinned in Cargo.toml).
          web = with pkgs; [ wasm-bindgen-cli_0_2_129 binaryen twiggy brotli nodejs wrangler ];
        in
        {
          default = pkgs.mkShell { packages = [ stable ] ++ web; };
          fuzz = pkgs.mkShell { packages = [ nightly pkgs.cargo-fuzz ]; };
        });
    };
}
