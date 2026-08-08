use std::{collections::HashMap, path::PathBuf};

use risc0_build::{DockerOptionsBuilder, GuestOptionsBuilder};

const BUILDER_TAG: &str =
    "r0.1.88.0@sha256:3e12f71bacd27527a61dea96fa0e53e468c99aa261d3a1019b593f6dbd943eb3";

fn main() {
    if std::env::var("HIVEMIND_ZKVM_USE_DOCKER").as_deref() == Ok("0") {
        risc0_build::embed_methods();
        return;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repository_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("methods crate is nested three levels below the repository root");
    let docker = DockerOptionsBuilder::default()
        .root_dir(repository_root)
        .docker_container_tag(BUILDER_TAG)
        .build()
        .expect("valid RISC Zero Docker options");
    let guest = GuestOptionsBuilder::default()
        .use_docker(docker)
        .build()
        .expect("valid RISC Zero guest options");

    risc0_build::embed_methods_with_options(HashMap::from([(
        "hivemind-managed-proof-guest",
        guest,
    )]));
}
