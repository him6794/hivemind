fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../../../proto/hivemind.proto");
    println!("cargo:rerun-if-changed=../../../proto/vpn.proto");
    println!("cargo:rerun-if-changed=../../../proto/managed_prover.proto");
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .type_attribute(".", "#[allow(clippy::large_enum_variant)]")
        .server_mod_attribute(
            ".",
            "#[allow(clippy::result_large_err, clippy::mixed_attributes_style)]",
        )
        .client_mod_attribute(
            ".",
            "#[allow(clippy::result_large_err, clippy::mixed_attributes_style)]",
        )
        .compile_protos(
            &[
                "../../../proto/hivemind.proto",
                "../../../proto/vpn.proto",
                "../../../proto/managed_prover.proto",
            ],
            &["../../../proto"],
        )?;
    Ok(())
}
