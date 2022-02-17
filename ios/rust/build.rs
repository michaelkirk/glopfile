const OUT_DIR: &str = "../Sendfile/src/FFI/Rust/SendfileRustFFI";

fn main() {
    uniffi_build::generate_scaffolding("src/lib.udl").unwrap();

    uniffi_bindgen::generate_bindings(
        "src/lib.udl",
        Some("uniffi.toml"),
        vec!["swift"],
        Some(OUT_DIR),
        true,
    )
    .unwrap();
}
