use std::path::PathBuf;

mod build_support;

fn main() {
    let definitions = lsh::compiler::builtin_definitions_path();
    let mut generator = lsh::compiler::Generator::new();
    generator
        .read_directory(definitions)
        .expect("read LSH definitions");
    let generated = generator.generate_rust().expect("compile LSH definitions");
    std::fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("lsh_definitions.rs"),
        generated,
    )
    .expect("write LSH definitions");
    println!("cargo:rerun-if-changed={}", definitions.display());
    println!("cargo:rerun-if-env-changed=TUNDRAUX_BUILD_COMMIT");
    let workspace = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let identity =
        build_support::build_identity(&workspace, std::env::var("TUNDRAUX_BUILD_COMMIT").ok());
    for path in identity.inputs {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!(
        "cargo:rustc-env=TUNDRAUX_BUILD_COMMIT={}",
        identity.commit.unwrap_or_else(|| "unknown".into())
    );
    println!("cargo:rustc-env=TUNDRAUX_BUILD_DIRTY={}", identity.dirty);
}
