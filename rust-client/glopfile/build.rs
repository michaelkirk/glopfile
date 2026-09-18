fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protos = ["src/p2p.proto", "src/websocket.proto"];
    for proto in protos {
        println!("cargo:rerun-if-changed={proto}");
    }

    let file_descriptors = protox::compile(protos, ["src/"])?;
    prost_build::Config::new()
        .bytes(["."])
        .compile_fds(file_descriptors)?;

    #[cfg(feature = "ffi")]
    {
        uniffi_build::generate_scaffolding("src/lib.udl")?;
    }

    Ok(())
}
