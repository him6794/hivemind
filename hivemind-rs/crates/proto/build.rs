fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../../../proto/hivemind.proto");
    println!("cargo:rerun-if-changed=../../../proto/vpn.proto");
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &["../../../proto/hivemind.proto", "../../../proto/vpn.proto"],
            &["../../../proto"],
        )?;
    Ok(())
}
