fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = "proto/glopfile_websocket_protocol.proto";
    println!("cargo:rerun-if-changed={proto}");

    let file_descriptors = protox::compile([proto], ["proto"])?;
    prost_build::Config::new().compile_fds(file_descriptors)?;
    Ok(())
}
