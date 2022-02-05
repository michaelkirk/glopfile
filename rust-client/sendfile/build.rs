fn main() -> Result<(), Box<dyn std::error::Error>> {
    prost_build::Config::new()
        .bytes(&["."])
        .compile_protos(&["src/p2p.proto", "src/websocket.proto"], &["src/"])?;
    Ok(())
}
