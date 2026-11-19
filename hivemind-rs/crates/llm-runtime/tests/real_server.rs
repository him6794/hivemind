//! Runs a real llama.cpp server when the environment points at one.
//!
//! Set `HM_LLM_TEST_BINARY` to the llama-server executable and `HM_LLM_TEST_MODEL`
//! to a GGUF file. Without both variables the test skips, so CI stays offline.
//! The prompt-token count below is specific to SmolLM2-360M-Instruct (Q8_0).

use std::path::PathBuf;
use std::time::Duration;

use hivemind_llm_runtime::{CompletionRequest, IsolationPolicy, LlamaServer, ServerConfig};

fn configured() -> Option<(PathBuf, PathBuf)> {
    let binary = std::env::var_os("HM_LLM_TEST_BINARY")?;
    let model = std::env::var_os("HM_LLM_TEST_MODEL")?;
    Some((PathBuf::from(binary), PathBuf::from(model)))
}

#[tokio::test]
async fn a_real_server_gives_stable_greedy_output_and_exact_counts() {
    let Some((binary, model)) = configured() else {
        eprintln!("skipped: set HM_LLM_TEST_BINARY and HM_LLM_TEST_MODEL to run this test");
        return;
    };
    let mut config = ServerConfig::new(binary, model, 512, 2);
    config.startup_timeout = Duration::from_secs(120);
    config.request_timeout = Duration::from_secs(120);
    let server = LlamaServer::start(&config, IsolationPolicy::UnsafeDevelopmentHost)
        .await
        .expect("server starts after the key check passes");
    let request = CompletionRequest {
        prompt: "The capital of France is".to_owned(),
        max_new_tokens: 16,
        temperature: 0.0,
        seed: 7,
    };
    let first = server.complete(&request).await.expect("first completion");
    let second = server.complete(&request).await.expect("second completion");
    server.shutdown().await.expect("the server stops");

    assert_eq!(
        first.content, second.content,
        "greedy output is reproducible"
    );
    assert_eq!(first.prompt_tokens, second.prompt_tokens);
    assert_eq!(
        first.prompt_tokens, 5,
        "matches llama-tokenize for this model"
    );
    assert_eq!(first.completion_tokens, 16);
    assert_eq!(first.stop_type, "limit");
}
