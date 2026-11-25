fn main() {
    let manifest_dir =
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must set CARGO_MANIFEST_DIR");
    let out_dir = std::env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR");
    ascii_assets::stage_workspace_assets(manifest_dir.as_ref(), out_dir.as_ref())
        .expect("failed to copy ASCII assets");
}
