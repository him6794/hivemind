//! Development CLI for the local model path: keys, catalog signing, metainfo,
//! private-swarm seed and fetch, and one completion through a local server.
//!
//! Nothing here is a production control plane. `run` refuses to start a server
//! unless the operator passes `--isolation unsafe-dev-host`, and `required` is
//! the default. That default fails until a Linux rootless OCI sandbox is configured.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use hivemind_llm_runtime::{
    generate_manifest, parse_pinned, sha256_file, CompletionRequest, GpuBackend, GpuOffloadPolicy,
    IsolationPolicy, LlamaServer, ServerConfig,
};
use hivemind_model_catalog::{
    parse_public_key_hex, parse_secret_key_hex, sha256_hex, sign_catalog, BlockQuota, Catalog,
    ModelEntry, TOKENS_PER_BLOCK,
};
use hivemind_model_swarm::{
    download, DownloadOptions, Metainfo, SeedServer, DEFAULT_MAX_CONNECTIONS,
};
use rand::RngCore;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        bail!(USAGE);
    }
    let command = args.remove(0);
    let flags = parse_flags(&args)?;
    match command.as_str() {
        "keygen" => keygen(&flags),
        "runtime-manifest" => runtime_manifest(&flags),
        "make-metainfo" => make_metainfo(&flags),
        "sign-catalog" => sign_catalog_command(&flags),
        "verify-catalog" => verify_catalog_command(&flags),
        "seed" => seed(&flags).await,
        "fetch" => fetch(&flags).await,
        "run" => run(&flags).await,
        other => bail!("unknown command `{other}`\n{USAGE}"),
    }
}

const USAGE: &str = "usage:
  llm-local keygen --secret <file> --public <file>
  llm-local runtime-manifest --dir <dir> --out <file>
  llm-local make-metainfo --file <gguf> --piece-length <bytes> --out <file>
  llm-local sign-catalog --catalog <file> --secret <file> --out <file>
  llm-local verify-catalog --catalog <file> --signature <file> --public <file>
  llm-local seed --catalog <file> --signature <file> --public <file> --model-id <id> --file <gguf> --metainfo <file> --listen <addr> [--seconds <n>]
  llm-local fetch --catalog <file> --signature <file> --public <file> --model-id <id> --metainfo <file> --peer <addr[,addr]> --out-dir <dir>
  llm-local run --catalog <file> --signature <file> --public <file> --model-id <id> --models-dir <dir> --runtime-dir <dir> --runtime-manifest <file> --prompt <text> --max-tokens <n> [--temperature <f>] [--seed <n>] [--threads <n>] [--context <n>] [--gpu-backend auto|cpu|cuda|metal|vulkan|rocm] [--gpu-layers auto|all|<n>] [--isolation required|unsafe-dev-host]";

type Flags = HashMap<String, String>;

fn parse_flags(args: &[String]) -> Result<Flags> {
    let mut flags = Flags::new();
    let mut index = 0;
    while index < args.len() {
        let key = args[index]
            .strip_prefix("--")
            .ok_or_else(|| anyhow!("expected a --flag, found `{}`", args[index]))?;
        let value = args
            .get(index + 1)
            .ok_or_else(|| anyhow!("--{key} needs a value"))?;
        if flags.insert(key.to_owned(), value.clone()).is_some() {
            bail!("--{key} is given twice");
        }
        index += 2;
    }
    Ok(flags)
}

fn required<'a>(flags: &'a Flags, name: &str) -> Result<&'a str> {
    flags
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("missing --{name}"))
}

fn optional<'a>(flags: &'a Flags, name: &str, default: &'a str) -> &'a str {
    flags.get(name).map_or(default, String::as_str)
}

fn random_peer_id() -> [u8; 20] {
    let mut peer_id = [0u8; 20];
    peer_id[..8].copy_from_slice(b"-HM0001-");
    rand::thread_rng().fill_bytes(&mut peer_id[8..]);
    peer_id
}

fn keygen(flags: &Flags) -> Result<()> {
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let secret = hex::encode(seed);
    let public = hex::encode(parse_secret_key_hex(&secret)?.verifying_key().to_bytes());
    std::fs::write(required(flags, "secret")?, format!("{secret}\n"))?;
    std::fs::write(required(flags, "public")?, format!("{public}\n"))?;
    println!("{}", serde_json::json!({ "public_key": public }));
    Ok(())
}

fn runtime_manifest(flags: &Flags) -> Result<()> {
    let manifest = generate_manifest(Path::new(required(flags, "dir")?))?;
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    std::fs::write(required(flags, "out")?, &bytes)?;
    println!(
        "{}",
        serde_json::json!({
            "manifest_sha256": sha256_hex(&bytes),
            "files": manifest.files.len(),
        })
    );
    Ok(())
}

fn make_metainfo(flags: &Flags) -> Result<()> {
    let piece_length: u32 = required(flags, "piece-length")?
        .parse()
        .context("--piece-length must be a number of bytes")?;
    let metainfo = Metainfo::from_file(Path::new(required(flags, "file")?), piece_length)?;
    let bytes = metainfo.to_bytes();
    std::fs::write(required(flags, "out")?, &bytes)?;
    println!(
        "{}",
        serde_json::json!({
            "name": metainfo.name(),
            "length": metainfo.length(),
            "piece_length": metainfo.piece_length(),
            "pieces": metainfo.piece_count(),
            "infohash": metainfo.infohash_hex(),
            "metainfo_sha256": sha256_hex(&bytes),
        })
    );
    Ok(())
}

fn sign_catalog_command(flags: &Flags) -> Result<()> {
    let bytes = std::fs::read(required(flags, "catalog")?)?;
    let catalog: Catalog = serde_json::from_slice(&bytes)?;
    catalog.validate()?;
    let secret = std::fs::read_to_string(required(flags, "secret")?)?;
    let signature = sign_catalog(&bytes, &parse_secret_key_hex(secret.trim())?);
    std::fs::write(required(flags, "out")?, format!("{signature}\n"))?;
    println!("{}", serde_json::json!({ "signed_bytes": bytes.len() }));
    Ok(())
}

fn verify_catalog_command(flags: &Flags) -> Result<()> {
    let catalog = load_catalog(flags)?;
    let models: Vec<&str> = catalog.models.iter().map(|m| m.model_id.as_str()).collect();
    println!(
        "{}",
        serde_json::json!({
            "verified": true,
            "generation": catalog.generation,
            "models": models,
        })
    );
    Ok(())
}

/// Loads the catalog only after its signature verifies under the pinned key.
fn load_catalog(flags: &Flags) -> Result<Catalog> {
    let bytes = std::fs::read(required(flags, "catalog")?)?;
    let signature = std::fs::read_to_string(required(flags, "signature")?)?;
    let public = std::fs::read_to_string(required(flags, "public")?)?;
    let key = parse_public_key_hex(public.trim())?;
    Ok(Catalog::verify_and_parse(&bytes, signature.trim(), &key)?)
}

/// Checks that the metainfo is the one the catalog names, before any peer is trusted.
fn check_metainfo(bytes: &[u8], model: &ModelEntry) -> Result<Metainfo> {
    if sha256_hex(bytes) != model.swarm.metainfo_sha256 {
        bail!("metainfo bytes do not match the catalog digest");
    }
    let metainfo = Metainfo::parse(bytes)?;
    if metainfo.infohash_hex() != model.swarm.infohash {
        bail!("metainfo infohash does not match the catalog");
    }
    if metainfo.piece_length() != model.swarm.piece_length {
        bail!("metainfo piece length does not match the catalog");
    }
    if metainfo.length() != model.gguf.size_bytes || metainfo.name() != model.gguf.file_name {
        bail!("metainfo file name or length does not match the catalog");
    }
    Ok(metainfo)
}

async fn seed(flags: &Flags) -> Result<()> {
    let catalog = load_catalog(flags)?;
    let model = catalog.model(required(flags, "model-id")?)?;
    let file = PathBuf::from(required(flags, "file")?);
    let metainfo_bytes = std::fs::read(required(flags, "metainfo")?)?;
    let metainfo = check_metainfo(&metainfo_bytes, model)?;
    // A seed serves content to others, so it checks the file against the catalog first.
    let (size, digest) = sha256_file(&file)?;
    if size != model.gguf.size_bytes || digest != model.gguf.sha256 {
        bail!("file does not match the catalog digest; refusing to seed it");
    }
    let listen: SocketAddr = required(flags, "listen")?
        .parse()
        .context("--listen must be host:port")?;
    let listener = TcpListener::bind(listen).await?;
    let seed = SeedServer::new(
        metainfo.clone(),
        file,
        random_peer_id(),
        DEFAULT_MAX_CONNECTIONS,
    )?;
    println!(
        "{}",
        serde_json::json!({
            "seeding": model.model_id,
            "listen": listener.local_addr()?.to_string(),
            "infohash": metainfo.infohash_hex(),
        })
    );
    let serving = tokio::spawn(seed.clone().serve(listener));
    match flags.get("seconds") {
        Some(seconds) => {
            let seconds: u64 = seconds.parse().context("--seconds must be a number")?;
            tokio::time::sleep(Duration::from_secs(seconds)).await;
        }
        None => {
            tokio::signal::ctrl_c().await?;
        }
    }
    serving.abort();
    println!(
        "{}",
        serde_json::json!({ "uploaded_bytes": seed.uploaded_bytes() })
    );
    Ok(())
}

async fn fetch(flags: &Flags) -> Result<()> {
    let catalog = load_catalog(flags)?;
    let model = catalog.model(required(flags, "model-id")?)?;
    let metainfo_bytes = std::fs::read(required(flags, "metainfo")?)?;
    let metainfo = check_metainfo(&metainfo_bytes, model)?;
    let peers = required(flags, "peer")?
        .split(',')
        .map(|peer| peer.trim().parse::<SocketAddr>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("--peer must be host:port[,host:port]")?;
    let out_dir = PathBuf::from(required(flags, "out-dir")?);
    std::fs::create_dir_all(&out_dir)?;
    let destination = out_dir.join(&model.gguf.file_name);
    let started = Instant::now();
    let report = download(&metainfo, &peers, &destination, DownloadOptions::default()).await?;
    let seconds = started.elapsed().as_secs_f64();
    let (size, digest) = sha256_file(&destination)?;
    if size != model.gguf.size_bytes || digest != model.gguf.sha256 {
        std::fs::remove_file(&destination)?;
        bail!("downloaded file fails the catalog digest and was removed");
    }
    println!(
        "{}",
        serde_json::json!({
            "fetched": destination.display().to_string(),
            "bytes": report.bytes,
            "pieces": report.pieces,
            "resumed_pieces": report.resumed_pieces,
            "rejected_pieces": report.rejected_pieces,
            "peers_connected": report.peers_connected,
            "seconds": seconds,
            "mib_per_second": (report.bytes as f64 / 1_048_576.0) / seconds,
            "peer_errors": report.peer_errors,
        })
    );
    Ok(())
}

async fn run(flags: &Flags) -> Result<()> {
    let catalog = load_catalog(flags)?;
    let model = catalog.model(required(flags, "model-id")?)?;

    let models_dir = PathBuf::from(required(flags, "models-dir")?);
    let model_path = models_dir.join(&model.gguf.file_name);
    let (size, digest) = sha256_file(&model_path).with_context(|| {
        format!(
            "model file {} is missing; fetch it first",
            model_path.display()
        )
    })?;
    if size != model.gguf.size_bytes || digest != model.gguf.sha256 {
        bail!("model file does not match the catalog digest");
    }

    let manifest_bytes = std::fs::read(required(flags, "runtime-manifest")?)?;
    let manifest = parse_pinned(&manifest_bytes, &catalog.runtime.manifest_sha256)?;
    let runtime_dir = PathBuf::from(required(flags, "runtime-dir")?);
    manifest.verify_dir(&runtime_dir)?;
    let binary_name = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };

    let policy = match optional(flags, "isolation", "required") {
        "required" => IsolationPolicy::Required,
        "unsafe-dev-host" => {
            eprintln!("warning: running the model server without isolation; development only");
            IsolationPolicy::UnsafeDevelopmentHost
        }
        other => bail!("unknown --isolation `{other}`"),
    };

    let gpu_backend = match optional(flags, "gpu-backend", "auto") {
        "auto" => GpuBackend::Auto,
        "cpu" => GpuBackend::Cpu,
        "cuda" => GpuBackend::Cuda,
        "metal" => GpuBackend::Metal,
        "vulkan" => GpuBackend::Vulkan,
        "rocm" => GpuBackend::Rocm,
        other => bail!("unknown --gpu-backend `{other}`"),
    };
    let gpu_layers = match optional(flags, "gpu-layers", "auto") {
        "auto" => GpuOffloadPolicy::Auto,
        "all" => GpuOffloadPolicy::All,
        num => GpuOffloadPolicy::Fixed(
            num.parse()
                .context("--gpu-layers must be auto, all, or an integer")?,
        ),
    };

    let max_tokens: u32 = required(flags, "max-tokens")?
        .parse()
        .context("--max-tokens must be a number")?;
    let request = CompletionRequest {
        prompt: required(flags, "prompt")?.to_owned(),
        max_new_tokens: max_tokens,
        temperature: optional(flags, "temperature", "0")
            .parse()
            .context("--temperature must be a number")?,
        seed: optional(flags, "seed", "1")
            .parse()
            .context("--seed must be a number")?,
    };
    let context_size: u32 = optional(flags, "context", "2048")
        .parse()
        .context("--context must be a number")?;
    // The input reservation is the catalog's context window. A larger server
    // window would let a prompt exceed what was reserved.
    if context_size > model.context_length {
        bail!(
            "--context {context_size} is above the model's context window of {}",
            model.context_length
        );
    }
    let threads: u32 = optional(flags, "threads", "4")
        .parse()
        .context("--threads must be a number")?;
    let mut config = ServerConfig::new(
        runtime_dir.join(binary_name),
        model_path,
        context_size,
        threads,
    );
    config.gpu_backend = gpu_backend;
    config.gpu_layers = gpu_layers;
    config.model_bytes = Some(model.gguf.size_bytes);
    config.max_catalog_context = Some(model.context_length);

    // This development run has no account balance, so the debit always succeeds.
    let mut quota = BlockQuota::default();
    let admission = quota.admit(model, max_tokens, |_| true)?;
    let input_blocks = admission.input_blocks();
    let output_blocks = admission.output_blocks();
    let charge_cpt = admission.charge_cpt();
    let reserved_input = admission.reserved_input_tokens();
    let reserved_output = admission.reserved_output_tokens();
    let started = Instant::now();
    let server = LlamaServer::start(&config, policy.clone()).await?;
    let outcome = server.complete(&request).await;
    server.shutdown().await?;
    let response = outcome?;
    quota.settle(
        admission,
        response.prompt_tokens,
        response.completion_tokens,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "model_id": model.model_id,
            "catalog_generation": catalog.generation,
            "runtime_version": catalog.runtime.version,
            "isolation": policy.label(),
            "gpu_backend": gpu_backend.label(),
            "content": response.content,
            "stop_type": response.stop_type,
            "prompt_tokens": response.prompt_tokens,
            "completion_tokens": response.completion_tokens,
            "billing": {
                "block_tokens": TOKENS_PER_BLOCK,
                "input_blocks_charged": input_blocks,
                "output_blocks_charged": output_blocks,
                "charge_cpt": charge_cpt,
                "reserved_input_tokens": reserved_input,
                "reserved_output_tokens": reserved_output,
                "input_tokens_remaining": quota.input_available(),
                "output_tokens_remaining": quota.output_available(),
                "input_cpt_per_million_tokens": model.prices.input_cpt_per_million_tokens,
                "output_cpt_per_million_tokens": model.prices.output_cpt_per_million_tokens,
            },
            "prompt_tokens_per_second": response.prompt_per_second,
            "completion_tokens_per_second": response.predicted_per_second,
            "wall_seconds_including_load": started.elapsed().as_secs_f64(),
        }))?
    );
    Ok(())
}
