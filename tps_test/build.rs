fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(false)
        .compile(
            &["../crates/rpc/proto/blockchain.proto"],
            &["../crates/rpc/proto"],
        )?;
    Ok(())
}
