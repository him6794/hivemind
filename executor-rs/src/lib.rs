use sha1::{Digest, Sha1};
use std::process::Command;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Magnet,
    ResultUrl,
}

#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    pub steps: u32,
    pub step_log_interval: u32,
    pub strict_source: bool,
    pub output_format: OutputFormat,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            steps: 5_000,
            step_log_interval: 1_000,
            strict_source: true,
            output_format: OutputFormat::ResultUrl,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionReport {
    pub source_btih: String,
    pub result_btih: String,
    pub final_digest: String,
    pub result_torrent: String,
}

#[derive(Debug, Clone)]
pub struct MontyConfig {
    pub python_cmd: String,
}

impl Default for MontyConfig {
    fn default() -> Self {
        Self {
            python_cmd: "python".to_string(),
        }
    }
}

fn pct_encode(input: &str) -> String {
    url::form_urlencoded::byte_serialize(input.as_bytes()).collect::<String>()
}

fn deterministic_btih_from_text(input: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn extract_btih(source: &str) -> Result<String, String> {
    let s = source.trim();
    if s.is_empty() {
        return Err("empty source".to_string());
    }

    let lower = s.to_lowercase();
    if lower.starts_with("magnet:?") {
        let u = Url::parse(s).map_err(|e| format!("invalid magnet url: {e}"))?;
        let xt = u
            .query_pairs()
            .find(|(k, _)| k == "xt")
            .map(|(_, v)| v.to_string())
            .unwrap_or_default()
            .to_lowercase();
        if !xt.starts_with("urn:btih:") {
            return Err("missing xt=urn:btih".to_string());
        }
        let h = xt.trim_start_matches("urn:btih:").to_string();
        if h.len() != 40 || hex::decode(&h).is_err() {
            return Err("invalid btih".to_string());
        }
        return Ok(h);
    }

    if lower.starts_with("http://") || lower.starts_with("https://") {
        let u = Url::parse(s).map_err(|e| format!("invalid http url: {e}"))?;
        let ih = u
            .query_pairs()
            .find(|(k, _)| k == "ih")
            .map(|(_, v)| v.to_string())
            .unwrap_or_default()
            .to_lowercase();
        if ih.len() == 40 && hex::decode(&ih).is_ok() {
            return Ok(ih);
        }
        return Err("missing or invalid ih".to_string());
    }

    Err("unsupported source".to_string())
}

fn resolve_source_btih(source: &str, strict_source: bool) -> Result<String, String> {
    if strict_source {
        return extract_btih(source);
    }
    extract_btih(source).or_else(|_| Ok(deterministic_btih_from_text(source)))
}

fn calc_digest_chain(seed: &str, steps: u32, step_log_interval: u32, mut logger: impl FnMut(&str)) -> String {
    let mut state = {
        let mut h = Sha1::new();
        h.update(seed.as_bytes());
        h.finalize().to_vec()
    };

    for step in 1..=steps {
        let mut h = Sha1::new();
        h.update(&state);
        h.update(step.to_le_bytes());
        state = h.finalize().to_vec();

        if step == 1
            || step == steps
            || (step_log_interval > 0 && step % step_log_interval == 0)
        {
            logger(&format!("executor-rs progress {step}/{steps}"));
        }
    }

    hex::encode(state)
}

fn build_result_torrent(task_id: &str, source_btih: &str, result_btih: &str, digest: &str, output_format: OutputFormat) -> String {
    match output_format {
        OutputFormat::Magnet => format!(
            "magnet:?xt=urn:btih:{}&dn={}&x.hivemind.src={}&x.hivemind.digest={}",
            result_btih,
            pct_encode(&format!("{}-result", task_id)),
            source_btih,
            digest
        ),
        OutputFormat::ResultUrl => format!(
            "result://{}?btih={}&src={}&digest={}",
            pct_encode(task_id),
            result_btih,
            source_btih,
            digest
        ),
    }
}

pub fn run_task(task_id: &str, source: &str, config: &ExecutionConfig, mut logger: impl FnMut(&str)) -> Result<ExecutionReport, String> {
    let task_id = task_id.trim();
    if task_id.is_empty() {
        return Err("empty task_id".to_string());
    }
    let source = source.trim();
    if source.is_empty() {
        return Err("empty source".to_string());
    }

    let source_btih = resolve_source_btih(source, config.strict_source)?;
    logger(&format!("executor-rs accepted task={task_id} btih={source_btih}"));

    let steps = if config.steps == 0 { 1 } else { config.steps };
    let digest_seed = format!("task={task_id};source={source_btih};steps={steps}");
    let final_digest = calc_digest_chain(&digest_seed, steps, config.step_log_interval, &mut logger);

    let result_payload = format!(
        "task_id={task_id}\nsource_btih={source_btih}\nfinal_digest={final_digest}\nsteps={steps}\n"
    );
    let mut rh = Sha1::new();
    rh.update(result_payload.as_bytes());
    let result_btih = hex::encode(rh.finalize());

    let result_torrent = build_result_torrent(task_id, &source_btih, &result_btih, &final_digest, config.output_format);

    Ok(ExecutionReport {
        source_btih,
        result_btih,
        final_digest,
        result_torrent,
    })
}

fn run_monty_digest(seed: &str, rounds: u32, monty: &MontyConfig) -> Result<String, String> {
    let script = r#"
import sys

try:
    import pydantic_monty
except Exception as e:
    print(f"IMPORT_ERROR:{e}", file=sys.stderr)
    sys.exit(11)

seed = sys.argv[1]
rounds = int(sys.argv[2])

code = r'''
def rolling(seed: str, rounds: int):
    state = 2166136261
    i = 0
    while i < rounds:
        for ch in seed:
            state = ((state ^ ord(ch)) * 16777619) & 0xFFFFFFFF
        state = ((state ^ i) * 16777619) & 0xFFFFFFFF
        i += 1
    return state

rolling(seed, rounds)
'''

m = pydantic_monty.Monty(code, inputs=['seed', 'rounds'])
output = m.run(inputs={'seed': seed, 'rounds': rounds})
print(str(output))
"#;

    let out = Command::new(&monty.python_cmd)
        .arg("-c")
        .arg(script)
        .arg(seed)
        .arg(rounds.to_string())
        .output()
        .map_err(|e| format!("failed to start monty python command '{}': {e}", monty.python_cmd))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let details = if !stderr.is_empty() {
            stderr
        } else {
            stdout
        };
        return Err(format!("monty execution failed: {details}"));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let value = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .ok_or_else(|| "monty execution produced empty output".to_string())?;

    Ok(value.to_string())
}

pub fn run_task_with_monty(
    task_id: &str,
    source: &str,
    config: &ExecutionConfig,
    monty: &MontyConfig,
    mut logger: impl FnMut(&str),
) -> Result<ExecutionReport, String> {
    let task_id = task_id.trim();
    if task_id.is_empty() {
        return Err("empty task_id".to_string());
    }
    let source = source.trim();
    if source.is_empty() {
        return Err("empty source".to_string());
    }

    let source_btih = resolve_source_btih(source, config.strict_source)?;
    logger(&format!("executor-rs monty accepted task={task_id} btih={source_btih}"));

    let rounds = if config.steps == 0 { 1 } else { config.steps };
    let seed = format!("task={task_id};source={source_btih};rounds={rounds}");
    let monty_out = run_monty_digest(&seed, rounds, monty)?;
    logger("executor-rs monty execution complete");

    let mut d = Sha1::new();
    d.update(monty_out.as_bytes());
    let final_digest = hex::encode(d.finalize());

    let result_payload = format!(
        "task_id={task_id}\nsource_btih={source_btih}\nfinal_digest={final_digest}\nbackend=monty\n"
    );
    let mut rh = Sha1::new();
    rh.update(result_payload.as_bytes());
    let result_btih = hex::encode(rh.finalize());
    let result_torrent = build_result_torrent(task_id, &source_btih, &result_btih, &final_digest, config.output_format);

    Ok(ExecutionReport {
        source_btih,
        result_btih,
        final_digest,
        result_torrent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_btih_from_magnet() {
        let src = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=x";
        let got = extract_btih(src).expect("extract btih from magnet");
        assert_eq!(got, "0123456789abcdef0123456789abcdef01234567");
    }

    #[test]
    fn extract_btih_from_http_ih() {
        let src = "https://example.com/a.torrent?ih=89abcdef0123456789abcdef0123456789abcdef";
        let got = extract_btih(src).expect("extract btih from url");
        assert_eq!(got, "89abcdef0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn non_strict_source_allows_fallback_hash() {
        let got = resolve_source_btih("file://tmp/task.zip", false).expect("fallback hash");
        assert_eq!(got.len(), 40);
    }

    #[test]
    fn run_task_result_url_output() {
        let cfg = ExecutionConfig {
            steps: 10,
            step_log_interval: 5,
            strict_source: true,
            output_format: OutputFormat::ResultUrl,
        };

        let report = run_task(
            "task-1",
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
            &cfg,
            |_| {},
        )
        .expect("run_task");

        assert_eq!(report.source_btih.len(), 40);
        assert_eq!(report.result_btih.len(), 40);
        assert_eq!(report.final_digest.len(), 40);
        assert!(report.result_torrent.starts_with("result://"));
    }

    #[test]
    fn run_task_magnet_output() {
        let cfg = ExecutionConfig {
            steps: 8,
            step_log_interval: 0,
            strict_source: false,
            output_format: OutputFormat::Magnet,
        };

        let report = run_task("task-2", "file://payload.zip", &cfg, |_| {}).expect("run_task");
        assert!(report.result_torrent.starts_with("magnet:?xt=urn:btih:"));
    }

    #[test]
    fn monty_config_default_python() {
        let m = MontyConfig::default();
        assert_eq!(m.python_cmd, "python");
    }
}
