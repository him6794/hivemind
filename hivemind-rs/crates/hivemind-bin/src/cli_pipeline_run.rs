use super::super::{artifacts::PreparedArtifacts, TaskCreation};
use super::*;
use general_compute_runtime::sha256_digest;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const STATE_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
struct RunState {
    version: u32,
    prefix: String,
    digest: String,
    settings: Value,
    rows: Vec<Row>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Row {
    stage: usize,
    item: usize,
    attempts: Vec<Attempt>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Attempt {
    task_id: String,
    max_cpt: i64,
    status: String,
    output: String,
    billed_cpt: Option<i64>,
    message: String,
    uploaded: bool,
}

impl Attempt {
    fn complete(&self) -> bool {
        self.status == "COMPLETED"
    }
    fn failed(&self) -> bool {
        matches!(
            self.status.as_str(),
            "FAILED"
                | "TIMED_OUT"
                | "CANCELLED"
                | "SUBMIT_FAILED"
                | "ID_CONFLICT"
                | "INVALID_OUTPUT"
        )
    }
    fn sent(&self) -> bool {
        !matches!(
            self.status.as_str(),
            "NOT_SUBMITTED" | "SUBMIT_FAILED" | "ID_CONFLICT"
        )
    }
    fn liability(&self) -> i128 {
        if self.sent() {
            i128::from(self.billed_cpt.unwrap_or(self.max_cpt))
        } else {
            0
        }
    }
}

fn task_id(prefix: &str, stage: usize, item: usize, retry: usize) -> String {
    let base = format!("{prefix}-{}-{}", stage + 1, item + 1);
    if retry == 0 {
        base
    } else {
        format!("{base}-r{retry}")
    }
}

fn settings(c: &PipelineRunCommand) -> Value {
    json!({"api":c.api_base,"username":c.username,"replicas":c.replicas,
        "cpu_score":c.cpu_score,"memory_gb":c.memory_gb,"gpu_score":c.gpu_score,
        "gpu_memory_gb":c.gpu_memory_gb,"storage_gb":c.storage_gb,"host_count":c.host_count})
}

/// Pin executable contents, not merely filenames. Caps are intentionally absent:
/// a failed attempt may be resumed with a larger budget, retaining old charges.
fn item_contents(source: &str, params: &Value, artifacts: &BTreeMap<String, String>) -> Value {
    json!({"source":source,"params":params,"artifacts":artifacts})
}

fn fingerprint(pipeline: &PipelineFile, base: &Path) -> Result<(String, Vec<String>)> {
    let mut stages = Vec::new();
    let mut item_digests = Vec::new();
    for (s, stage) in pipeline.stages.iter().enumerate() {
        if stage.items.is_empty() {
            bail!("stage {} has no items", s + 1);
        }
        let mut items = Vec::new();
        for (i, item) in stage.items.iter().enumerate() {
            let source = std::fs::read_to_string(resolve(base, &item.source)).map_err(|e| {
                anyhow!("stage {} item {} source {}: {e}", s + 1, i + 1, item.source)
            })?;
            if source.trim().is_empty() {
                bail!("stage {} item {} source is empty", s + 1, i + 1);
            }
            let mut artifacts = BTreeMap::new();
            for (name, path) in &item.artifacts {
                let bytes = std::fs::read(resolve(base, path))
                    .map_err(|e| anyhow!("artifact {path}: {e}"))?;
                artifacts.insert(name.clone(), sha256_digest(&bytes));
            }
            let contents = item_contents(&source, &item_params(base, item)?, &artifacts);
            item_digests.push(sha256_digest(&serde_json::to_vec(&contents)?));
            items.push(contents);
        }
        stages.push(items);
    }
    let digest = sha256_digest(&serde_json::to_vec(
        &json!({"input":pipeline.input,"stages":stages}),
    )?);
    Ok((digest, item_digests))
}

async fn atomic_json(path: &str, value: &impl Serialize) -> Result<()> {
    let temporary = format!("{path}.{}.tmp", uuid::Uuid::new_v4());
    tokio::fs::write(&temporary, serde_json::to_vec_pretty(value)?)
        .await
        .map_err(|e| anyhow!("cannot write {temporary}: {e}"))?;
    tokio::fs::rename(&temporary, path)
        .await
        .map_err(|e| anyhow!("cannot replace {path}: {e}"))
}

fn quote_body(c: &PipelineRunCommand) -> serde_json::Map<String, Value> {
    let mut body =
        json!({"runtime":"hcode","task_source":"return 1;","torrent":"null","max_cpt":1})
            .as_object()
            .unwrap()
            .clone();
    let values = settings(c);
    for key in [
        "cpu_score",
        "memory_gb",
        "gpu_score",
        "gpu_memory_gb",
        "storage_gb",
        "host_count",
    ] {
        if !values[key].is_null() {
            body.insert(key.into(), values[key].clone());
        }
    }
    if let Some(replicas) = c.replicas {
        body.insert("managed_replica_count".into(), replicas.into());
        body.insert(
            "managed_quorum".into(),
            super::super::check::majority(replicas).into(),
        );
    }
    body
}

fn validate_output(text: &str) -> Result<Value> {
    if text.len() > hivemind_proto::MANAGED_CONSENSUS_RESULT_MAX_BYTES {
        bail!("output exceeds consensus result size limit");
    }
    serde_json::from_str(text).map_err(|e| anyhow!("output is not a JSON value: {e}"))
}

fn verify_saved(
    state: &RunState,
    c: &PipelineRunCommand,
    pipeline: &PipelineFile,
    digest: &str,
    prefix: &str,
) -> Result<()> {
    if state.version != STATE_VERSION {
        bail!("unsupported pipeline state version");
    }
    if state.prefix != prefix || state.settings != settings(c) {
        bail!("run prefix, account, API, resource or replica settings changed");
    }
    if state.digest != digest {
        bail!("pipeline input, source, parameters or artifact contents changed; cannot resume");
    }
    let expected: Vec<_> = pipeline
        .stages
        .iter()
        .enumerate()
        .flat_map(|(s, stage)| (0..stage.items.len()).map(move |i| (s, i)))
        .collect();
    if state
        .rows
        .iter()
        .map(|r| (r.stage, r.item))
        .collect::<Vec<_>>()
        != expected
    {
        bail!("saved pipeline rows do not match the pipeline");
    }
    for row in &state.rows {
        for (retry, attempt) in row.attempts.iter().enumerate() {
            if attempt.task_id != task_id(prefix, row.stage, row.item, retry)
                || attempt.max_cpt <= 0
                || attempt.billed_cpt.is_some_and(|v| v < 0)
            {
                bail!(
                    "invalid saved attempt for stage {} item {}",
                    row.stage + 1,
                    row.item + 1
                );
            }
        }
    }
    Ok(())
}

struct Runner<'a> {
    command: &'a PipelineRunCommand,
    client: reqwest::Client,
    token: String,
    state_path: String,
    base: PathBuf,
    state: RunState,
    item_digests: Vec<String>,
    next_submit: Instant,
}

impl Runner<'_> {
    async fn save(&self) -> Result<()> {
        atomic_json(&self.state_path, &self.state).await
    }

    async fn refresh(&mut self) -> Result<()> {
        let ids: Vec<_> = self
            .state
            .rows
            .iter()
            .flat_map(|r| &r.attempts)
            .filter(|a| a.sent())
            .map(|a| a.task_id.clone())
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        let snapshots = super::super::fetch_task_snapshots(
            &self.client,
            &self.command.api_base,
            &self.token,
            &ids,
        )
        .await?;
        for attempt in self
            .state
            .rows
            .iter_mut()
            .flat_map(|r| &mut r.attempts)
            .filter(|a| a.sent())
        {
            let Some(task) = snapshots.get(&attempt.task_id) else {
                continue;
            };
            if let Some(status) = task["status"].as_str() {
                attempt.status = status.into();
            }
            if let Some(output) = task["output"].as_str() {
                attempt.output = output.into();
            }
            if task["billing_settled"].as_bool() == Some(true) {
                let billed = task["billed_amount"].as_i64().ok_or_else(|| {
                    anyhow!("task {} has no settled billing amount", attempt.task_id)
                })?;
                if billed < 0 {
                    bail!("task {} has negative billing", attempt.task_id);
                }
                attempt.billed_cpt = Some(billed);
            } else {
                // TaskInfo carries billed_amount=0 even before settlement.
                // Until Nodepool confirms settlement retain the whole cap.
                attempt.billed_cpt = None;
            }
            attempt.message = task["status_message"].as_str().unwrap_or_default().into();
            if attempt.complete() {
                if let Err(error) = validate_output(&attempt.output) {
                    attempt.status = "INVALID_OUTPUT".into();
                    attempt.message = error.to_string();
                }
            }
        }
        self.save().await
    }

    fn check_failure(&self, indices: &[usize]) -> Result<()> {
        for &index in indices {
            let row = &self.state.rows[index];
            if let Some(attempt) = row.attempts.last().filter(|a| a.failed()) {
                bail!(
                    "stage {} item {} ({}): {} — {}",
                    row.stage + 1,
                    row.item + 1,
                    attempt.task_id,
                    attempt.status,
                    attempt.message
                );
            }
        }
        Ok(())
    }

    fn check_total(&self, indices: &[usize]) -> Result<()> {
        let committed: i128 = self
            .state
            .rows
            .iter()
            .flat_map(|r| &r.attempts)
            .map(Attempt::liability)
            .sum();
        let planned: i128 = indices
            .iter()
            .filter_map(|&i| self.state.rows[i].attempts.last())
            .filter(|a| a.status == "NOT_SUBMITTED")
            .map(|a| i128::from(a.max_cpt))
            .sum();
        if committed + planned > i128::from(self.command.max_total_cpt) {
            bail!("settled or reserved {committed} CPT plus planned {planned} CPT exceeds --max-total-cpt {}",self.command.max_total_cpt);
        }
        Ok(())
    }

    async fn prepare_stage(
        &mut self,
        stage: &Stage,
        indices: &[usize],
        previous: &[Value],
        price: i64,
    ) -> Result<()> {
        for (&index, item) in indices.iter().zip(&stage.items) {
            if self.state.rows[index]
                .attempts
                .last()
                .is_some_and(|a| !a.failed() && a.status != "NOT_SUBMITTED")
            {
                continue;
            }
            let cap = match item.max_cpt {
                Some(cap) if cap > 0 => cap,
                Some(_) => bail!(
                    "stage {} item {} max_cpt must be positive",
                    self.state.rows[index].stage + 1,
                    self.state.rows[index].item + 1
                ),
                None if self.command.no_local_run => bail!(
                    "stage {} item {} has no max_cpt with --no-local-run",
                    self.state.rows[index].stage + 1,
                    self.state.rows[index].item + 1
                ),
                None => {
                    let (base, item, previous) =
                        (self.base.clone(), item.clone(), previous.to_vec());
                    let report = tokio::task::spawn_blocking(move || {
                        run_item(
                            &base,
                            &item,
                            &previous,
                            limits(DEFAULT_MAX_USAGE_UNITS, 300),
                        )
                    })
                    .await??;
                    required_task_budget(
                        report.usage_units,
                        self.command.replicas.unwrap_or(DEFAULT_REPLICAS),
                    )?
                    .task_budget
                    .max(price)
                }
            };
            if cap < price {
                bail!(
                    "stage {} item {} cap {cap} is below resource quote {price}",
                    self.state.rows[index].stage + 1,
                    self.state.rows[index].item + 1
                );
            }
            let row = &mut self.state.rows[index];
            if let Some(attempt) = row
                .attempts
                .last_mut()
                .filter(|a| a.status == "NOT_SUBMITTED")
            {
                attempt.max_cpt = cap;
                continue;
            }
            let id = task_id(&self.state.prefix, row.stage, row.item, row.attempts.len());
            if !super::super::is_safe_task_id(&id) {
                bail!("invalid or too long task ID {id}");
            }
            row.attempts.push(Attempt {
                task_id: id,
                max_cpt: cap,
                status: "NOT_SUBMITTED".into(),
                output: String::new(),
                billed_cpt: None,
                message: String::new(),
                uploaded: false,
            });
        }
        self.check_total(indices)?;
        // Never adopt another run's task, including a conflicting retry ID.
        let ids: Vec<_> = indices
            .iter()
            .filter_map(|&i| self.state.rows[i].attempts.last())
            .filter(|a| a.status == "NOT_SUBMITTED")
            .map(|a| a.task_id.clone())
            .collect();
        if !ids.is_empty() {
            let existing = super::super::fetch_task_snapshots(
                &self.client,
                &self.command.api_base,
                &self.token,
                &ids,
            )
            .await?;
            if let Some(id) = ids.iter().find(|id| existing.contains_key(*id)) {
                bail!("task ID {id} is already occupied");
            }
        }
        Ok(())
    }

    async fn submit(&mut self, index: usize, item: &StageItem, previous: &[Value]) -> Result<()> {
        let attempt = self.state.rows[index].attempts.last().unwrap();
        if attempt.complete() {
            return Ok(());
        }
        let fresh = attempt.status == "NOT_SUBMITTED";
        let uncertain = attempt.status == "SENDING";
        if !fresh && !uncertain && attempt.uploaded {
            return Ok(());
        }
        let params = item_params(&self.base, item)?;
        let input = serde_json::to_string(&compose_input(&params, previous))?;
        if input.len() > hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES {
            bail!("composed task input exceeds JSON size limit");
        }
        let specs: Vec<_> = item
            .artifacts
            .iter()
            .map(|(name, path)| {
                (
                    name.clone(),
                    resolve(&self.base, path).to_string_lossy().into_owned(),
                )
            })
            .collect();
        let prepared = PreparedArtifacts::prepare(&input, &specs).await?;
        let source = tokio::fs::read_to_string(resolve(&self.base, &item.source)).await?;
        let artifacts = prepared
            .as_ref()
            .map(PreparedArtifacts::content_hashes)
            .unwrap_or_default();
        // Check the exact retained bytes/values used below, not another file
        // read that could change before submission or artifact upload.
        let contents = item_contents(&source, &params, &artifacts);
        if sha256_digest(&serde_json::to_vec(&contents)?) != self.item_digests[index] {
            bail!("pipeline source, parameters or artifact contents changed during execution");
        }
        let mut body = quote_body(self.command);
        body.insert("task_id".into(), attempt.task_id.clone().into());
        body.insert("max_cpt".into(), attempt.max_cpt.into());
        body.insert("task_source".into(), source.into());
        body.insert(
            "torrent".into(),
            prepared
                .as_ref()
                .map(|p| p.input_json.clone())
                .unwrap_or(input)
                .into(),
        );
        if fresh || uncertain {
            // This checkpoint precedes the side effect. A lost reply must leave
            // SENDING on disk, so resume can reconcile or retry the same ID.
            self.state.rows[index].attempts.last_mut().unwrap().status = "SENDING".into();
            self.save().await?;
            let response = super::super::create_task(
                &self.client,
                &self.command.api_base,
                &self.token,
                &Value::Object(body),
                &mut self.next_submit,
                Duration::from_millis(60_000 / u64::from(self.command.per_minute.max(1))),
            )
            .await;
            let mut refusal = None;
            let attempt = self.state.rows[index].attempts.last_mut().unwrap();
            match response {
                Ok(TaskCreation::Created) => attempt.status = "SUBMITTED".into(),
                Ok(TaskCreation::Exists(_)) if uncertain => attempt.status = "SUBMITTED".into(),
                Ok(TaskCreation::Exists(message)) => {
                    attempt.status = "ID_CONFLICT".into();
                    attempt.message = format!("task ID occupied: {message}");
                }
                Ok(TaskCreation::Refused(message)) => {
                    // Even a 400 can follow task persistence (for example,
                    // consensus-policy persistence failed). Reconcile before
                    // treating the task as absent or releasing its cap.
                    attempt.message = message.clone();
                    refusal = Some(message);
                }
                Err(error) => {
                    attempt.message = error.to_string();
                    self.save().await?;
                    return Err(error);
                }
            }
            self.save().await?;
            if let Some(message) = refusal {
                self.refresh().await?;
                let attempt = self.state.rows[index].attempts.last_mut().unwrap();
                if attempt.status == "SENDING" {
                    attempt.status = "SUBMIT_FAILED".into();
                    attempt.message = message.clone();
                }
                self.save().await?;
                bail!("submission refused: {message}");
            }
            self.check_failure(&[index])?;
        }
        if let Some(prepared) = prepared {
            let id = &self.state.rows[index].attempts.last().unwrap().task_id;
            prepared
                .upload(&self.client, &self.command.api_base, &self.token, id)
                .await?;
        }
        self.state.rows[index].attempts.last_mut().unwrap().uploaded = true;
        self.save().await
    }

    async fn execute(&mut self, pipeline: &PipelineFile) -> Result<Vec<Value>> {
        if self.command.resume {
            self.refresh().await?;
        }
        let price = super::super::post_quote(
            &self.client,
            &self.command.api_base,
            &self.token,
            &quote_body(self.command),
        )
        .await?;
        if price < 0 {
            bail!("negative resource quote");
        }
        let mut previous = vec![pipeline.input.clone()];
        for (s, stage) in pipeline.stages.iter().enumerate() {
            let indices: Vec<_> = self
                .state
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| r.stage == s)
                .map(|(i, _)| i)
                .collect();
            if fingerprint(pipeline, &self.base)?.0 != self.state.digest {
                bail!("pipeline files changed during execution");
            }
            self.prepare_stage(stage, &indices, &previous, price)
                .await?;
            for (&index, item) in indices.iter().zip(&stage.items) {
                // See failures that arrived while earlier items were running
                // before sending the next item in this same stage.
                self.refresh().await?;
                self.check_failure(&indices)?;
                self.check_total(&indices)?;
                self.submit(index, item, &previous).await.map_err(|e| {
                    anyhow!(
                        "stage {} item {} ({}): {e}",
                        s + 1,
                        self.state.rows[index].item + 1,
                        item.source
                    )
                })?;
            }
            let deadline = Instant::now() + Duration::from_secs(self.command.timeout_secs);
            loop {
                self.refresh().await?;
                self.check_failure(&indices)?;
                self.check_total(&indices)?;
                if indices.iter().all(|&i| {
                    self.state.rows[i]
                        .attempts
                        .last()
                        .is_some_and(Attempt::complete)
                }) {
                    break;
                }
                if Instant::now() >= deadline {
                    bail!(
                        "stage {} did not finish within the wait; use --resume",
                        s + 1
                    );
                }
                tokio::time::sleep(
                    Duration::from_secs(self.command.poll_secs)
                        .min(deadline.saturating_duration_since(Instant::now())),
                )
                .await;
            }
            previous = indices
                .iter()
                .map(|&i| validate_output(&self.state.rows[i].attempts.last().unwrap().output))
                .collect::<Result<_>>()?;
        }
        Ok(previous)
    }
}

pub async fn run_pipeline(command: &PipelineRunCommand) -> Result<()> {
    let base = pipeline_base_dir(&command.path);
    let pipeline = load_pipeline(Path::new(&command.path))?;
    let (digest, item_digests) = fingerprint(&pipeline, &base)?;
    let prefix = command
        .task_prefix
        .clone()
        .unwrap_or_else(|| format!("pipeline-{}", uuid::Uuid::new_v4().simple()));
    if command.resume && command.task_prefix.is_none() {
        bail!("--resume requires --task-prefix");
    }
    let out = command
        .out_path
        .clone()
        .unwrap_or_else(|| format!("{prefix}-results.json"));
    let state_path = format!("{out}.state.json");
    let state = if command.resume {
        let state: RunState = serde_json::from_slice(
            &tokio::fs::read(&state_path)
                .await
                .map_err(|e| anyhow!("cannot resume {state_path}: {e}"))?,
        )?;
        verify_saved(&state, command, &pipeline, &digest, &prefix)?;
        state
    } else {
        if Path::new(&state_path).exists() || Path::new(&out).exists() {
            bail!("output or state already exists; use --resume or choose a new --out");
        }
        RunState {
            version: STATE_VERSION,
            prefix: prefix.clone(),
            digest,
            settings: settings(command),
            rows: pipeline
                .stages
                .iter()
                .enumerate()
                .flat_map(|(s, stage)| {
                    (0..stage.items.len()).map(move |i| Row {
                        stage: s,
                        item: i,
                        attempts: Vec::new(),
                    })
                })
                .collect(),
        }
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let token = super::super::login(
        &client,
        &command.api_base,
        &command.username,
        &command.password,
    )
    .await?;
    if !command.resume {
        let ids: Vec<_> = state
            .rows
            .iter()
            .map(|r| task_id(&prefix, r.stage, r.item, 0))
            .collect();
        for id in &ids {
            if !super::super::is_safe_task_id(id) {
                bail!("invalid or too long task ID {id}");
            }
        }
        let existing =
            super::super::fetch_task_snapshots(&client, &command.api_base, &token, &ids).await?;
        if let Some(id) = ids.iter().find(|id| existing.contains_key(*id)) {
            bail!("task ID {id} is already occupied");
        }
    }
    println!("Pipeline run: {prefix}; results: {out}");
    let mut runner = Runner {
        command,
        client,
        token,
        state_path,
        base,
        state,
        item_digests,
        next_submit: Instant::now(),
    };
    let result = runner.execute(&pipeline).await;
    // Keep failure and partial results as well as successes. Before the first
    // attempted submission no state/result file is created.
    if runner
        .state
        .rows
        .iter()
        .flat_map(|r| &r.attempts)
        .any(|a| a.status != "NOT_SUBMITTED")
    {
        runner.save().await?;
        let billed: i128 = runner
            .state
            .rows
            .iter()
            .flat_map(|r| &r.attempts)
            .filter_map(|a| a.billed_cpt)
            .map(i128::from)
            .sum();
        let reserved: i128 = runner
            .state
            .rows
            .iter()
            .flat_map(|r| &r.attempts)
            .map(Attempt::liability)
            .sum();
        atomic_json(&out,&json!({"prefix":prefix,"success":result.is_ok(),"final_output":result.as_ref().ok(),"billed_cpt":i64::try_from(billed)?,"settled_or_reserved_cpt":i64::try_from(reserved)?,"error":result.as_ref().err().map(ToString::to_string),"rows":runner.state.rows})).await?;
    }
    let output = result?;
    println!("Final output: {}", serde_json::to_string(&output)?);
    Ok(())
}

#[cfg(test)]
#[path = "cli_pipeline_run_tests.rs"]
mod tests;
