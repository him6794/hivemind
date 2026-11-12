//! Local model import and measured splitting into ordinary hcode pipelines.

use anyhow::{anyhow, bail, Context, Result};
use general_compute_runtime::{managed_budget::required_task_budget, sha256_digest};
use managed_function_runtime::{artifacts::TensorArtifacts, ExecutionLimits, ManagedExecutor};
use serde::{
    de::{MapAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::pipeline::{PipelineFile, Stage, StageItem};

const VALUE_BYTES: usize = 786_432;
const OUTPUT_BYTES: usize = 196_608;
const COLLECTION_ITEMS: usize = 75_000;
const MATERIALIZATION_BYTES: u64 = 12_582_912;
const USAGE: &str = "Usage: hivemind model init <weights.safetensors> [--out <model.json>] [--activation <none|relu|gelu|softmax>] [--final <none|relu|gelu|softmax>]\n       hivemind model split <model.json> --weights <file> --input <json> --out-dir <dir> [--max-tensor-bytes <n>] [--max-materialization-bytes <n>] [--max-usage <n>] [--replicas <2-7>]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCommand {
    Init(InitOptions),
    Split(SplitOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitOptions {
    pub weights: String,
    pub output: String,
    pub activation: String,
    pub final_activation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitOptions {
    pub model: String,
    pub weights: String,
    pub input: String,
    pub out_dir: String,
    pub max_tensor_bytes: usize,
    pub max_materialization_bytes: u64,
    pub max_usage_units: u64,
    pub replicas: u16,
}

impl Default for SplitOptions {
    fn default() -> Self {
        Self {
            model: String::new(),
            weights: String::new(),
            input: String::new(),
            out_dir: String::new(),
            max_tensor_bytes: 786_432,
            max_materialization_bytes: 12_582_912,
            max_usage_units: 20_000_000,
            replicas: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ModelFile {
    layers: Vec<Layer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum Layer {
    Linear {
        weight: String,
        #[serde(default)]
        bias: Option<String>,
    },
    Relu,
    Gelu,
    Softmax,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    dtype: String,
    shape: Vec<usize>,
    data_offsets: [u64; 2],
}

struct Header(BTreeMap<String, Descriptor>);

impl<'de> Deserialize<'de> for Header {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct HeaderVisitor;
        impl<'de> Visitor<'de> for HeaderVisitor {
            type Value = Header;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a safetensors header with unique tensor names")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Header, M::Error> {
                let mut tensors = BTreeMap::new();
                let mut metadata = false;
                while let Some(name) = map.next_key::<String>()? {
                    if name == "__metadata__" {
                        if metadata {
                            return Err(serde::de::Error::custom("duplicate metadata"));
                        }
                        metadata = true;
                        let _ = map.next_value::<BTreeMap<String, String>>()?;
                    } else {
                        if name.is_empty() || name.len() > 255 || tensors.contains_key(&name) {
                            return Err(serde::de::Error::custom(
                                "invalid or duplicate tensor name",
                            ));
                        }
                        tensors.insert(name, map.next_value()?);
                    }
                }
                Ok(Header(tensors))
            }
        }
        deserializer.deserialize_map(HeaderVisitor)
    }
}

struct Weights {
    path: PathBuf,
    data_start: u64,
    tensors: BTreeMap<String, Descriptor>,
}

#[derive(Clone)]
struct TensorBytes {
    dtype: String,
    shape: Vec<usize>,
    data: Vec<u8>,
}

struct Generated {
    pipeline: PipelineFile,
    files: BTreeMap<String, Vec<u8>>,
    output: Value,
}

pub fn parse_model_args(args: &[String]) -> Result<ModelCommand> {
    let path = args
        .get(3)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| anyhow!(USAGE))?
        .clone();
    let mut init = InitOptions {
        weights: path.clone(),
        output: "model.json".into(),
        activation: "relu".into(),
        final_activation: "none".into(),
    };
    let mut split = SplitOptions {
        model: path,
        ..SplitOptions::default()
    };
    let kind = args.get(2).map(String::as_str).unwrap_or("");
    if !matches!(kind, "init" | "split") {
        bail!(USAGE);
    }
    let mut index = 4;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| anyhow!("missing value for {flag}"))?;
        match (kind, flag) {
            ("init", "--out") => init.output = value.clone(),
            ("init", "--activation") => {
                activation(value)?;
                init.activation = value.clone();
            }
            ("init", "--final") => {
                activation(value)?;
                init.final_activation = value.clone();
            }
            ("split", "--weights") => split.weights = value.clone(),
            ("split", "--input") => split.input = value.clone(),
            ("split", "--out-dir") => split.out_dir = value.clone(),
            ("split", "--max-tensor-bytes") => {
                split.max_tensor_bytes = value.parse().context("invalid --max-tensor-bytes")?
            }
            ("split", "--max-materialization-bytes") => {
                split.max_materialization_bytes = value
                    .parse()
                    .context("invalid --max-materialization-bytes")?
            }
            ("split", "--max-usage") => {
                split.max_usage_units = value.parse().context("invalid --max-usage")?
            }
            ("split", "--replicas") => split.replicas = super::check::parse_replicas(flag, value)?,
            _ => bail!("unknown model {kind} flag {flag}\n{USAGE}"),
        }
        index += 2;
    }
    if kind == "init" {
        return Ok(ModelCommand::Init(init));
    }
    if split.weights.is_empty() || split.input.is_empty() || split.out_dir.is_empty() {
        bail!("model split requires --weights, --input and --out-dir\n{USAGE}");
    }
    validate_options(&split)?;
    Ok(ModelCommand::Split(split))
}

pub async fn run_model(command: ModelCommand) -> Result<()> {
    tokio::task::spawn_blocking(move || match command {
        ModelCommand::Init(options)=>{
            let weights=read_weights(Path::new(&options.weights))?;
            let model=infer_model(&weights,&options.activation,&options.final_activation)?;
            write_new(Path::new(&options.output),&serde_json::to_vec_pretty(&model)?)?;
            println!("Wrote {} layers to {}. Activations were supplied explicitly; they are not stored in safetensors.",model.layers.len(),options.output);
            Ok(())
        },
        ModelCommand::Split(options)=>{
            validate_options(&options)?;
            let destination=Path::new(&options.out_dir);
            if destination.exists() && (!destination.is_dir() || std::fs::read_dir(destination)?.next().is_some()) {
                bail!("output directory must be absent or empty: {}",destination.display());
            }
            let model:ModelFile=serde_json::from_slice(&std::fs::read(&options.model).with_context(||format!("cannot read {}",options.model))?).context("invalid model description")?;
            let weights=read_weights(Path::new(&options.weights))?;
            let input:Value=serde_json::from_str(&options.input).context("--input must be a JSON batch of sample rows")?;
            let generated=generate(&model,&weights,input,&options)?;
            std::fs::create_dir_all(destination)?;
            for (name,bytes) in &generated.files { write_new(&destination.join(name),bytes)?; }
            write_new(&destination.join("pipeline.json"),&serde_json::to_vec_pretty(&generated.pipeline)?)?;
            println!("Wrote {} stages to {}",generated.pipeline.stages.len(),destination.join("pipeline.json").display());
            println!("Local sample output: {}",generated.output);
            println!("Caps were measured locally for {} replicas and this input shape. Resource quotes are checked by pipeline run.",options.replicas);
            Ok(())
        },
    }).await.context("model preparation stopped unexpectedly")?
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| {
            format!(
                "cannot create {}; existing files are never overwritten",
                path.display()
            )
        })?;
    file.write_all(bytes)?;
    Ok(())
}

fn validate_options(options: &SplitOptions) -> Result<()> {
    if !(1..=VALUE_BYTES).contains(&options.max_tensor_bytes) {
        bail!("--max-tensor-bytes must be between 1 and {VALUE_BYTES}");
    }
    if !(1..=MATERIALIZATION_BYTES).contains(&options.max_materialization_bytes) {
        bail!("--max-materialization-bytes must be between 1 and {MATERIALIZATION_BYTES}");
    }
    if options.max_usage_units == 0 {
        bail!("--max-usage must be positive");
    }
    if !(2..=7).contains(&options.replicas) {
        bail!("--replicas must be between 2 and 7");
    }
    Ok(())
}

fn read_weights(path: &Path) -> Result<Weights> {
    let mut file =
        File::open(path).with_context(|| format!("cannot open weights {}", path.display()))?;
    let length = file.metadata()?.len();
    let mut prefix = [0; 8];
    file.read_exact(&mut prefix)
        .context("truncated safetensors header")?;
    let header_len = u64::from_le_bytes(prefix);
    if header_len == 0 || header_len > 1024 * 1024 {
        bail!("safetensors header must be between 1 byte and 1 MiB");
    }
    let data_start = 8 + header_len;
    if data_start > length {
        bail!("truncated safetensors header");
    }
    let mut header = vec![0; header_len as usize];
    file.read_exact(&mut header)?;
    let Header(tensors) = serde_json::from_slice(&header).context("invalid safetensors header")?;
    if tensors.is_empty() {
        bail!("safetensors contains no tensors");
    }
    let mut ranges = Vec::new();
    for (name, descriptor) in &tensors {
        let width = storage_width(&descriptor.dtype)?;
        if descriptor.shape.is_empty()
            || descriptor.shape.len() > 8
            || descriptor.shape.contains(&0)
        {
            bail!("tensor {name} has an empty or invalid shape");
        }
        let count = descriptor
            .shape
            .iter()
            .try_fold(1_u64, |n, dim| n.checked_mul(*dim as u64))
            .ok_or_else(|| anyhow!("tensor {name} shape overflows"))?;
        let bytes = count
            .checked_mul(width as u64)
            .ok_or_else(|| anyhow!("tensor {name} byte count overflows"))?;
        let [start, end] = descriptor.data_offsets;
        if end < start || end > length - data_start || end - start != bytes {
            bail!("tensor {name} offsets do not match its shape and dtype");
        }
        ranges.push((start, end));
    }
    ranges.sort_unstable();
    let mut cursor = 0;
    for (start, end) in ranges {
        if cursor != start {
            bail!("safetensors ranges must be contiguous and non-overlapping");
        }
        cursor = end;
    }
    if cursor != length - data_start {
        bail!("safetensors has unreferenced trailing bytes");
    }
    Ok(Weights {
        path: path.to_path_buf(),
        data_start,
        tensors,
    })
}

fn storage_width(dtype: &str) -> Result<usize> {
    match dtype {
        "F32" => Ok(4),
        "F64" => Ok(8),
        _ => bail!("model weights support F32 and F64, found {dtype}"),
    }
}

fn activation(name: &str) -> Result<Option<Layer>> {
    Ok(match name {
        "none" => None,
        "relu" => Some(Layer::Relu),
        "gelu" => Some(Layer::Gelu),
        "softmax" => Some(Layer::Softmax),
        _ => bail!("unknown activation {name}; use none, relu, gelu or softmax"),
    })
}

fn infer_model(weights: &Weights, hidden: &str, final_activation: &str) -> Result<ModelFile> {
    let hidden = activation(hidden)?;
    let final_activation = activation(final_activation)?;
    let mut ordered = BTreeMap::new();
    for name in weights.tensors.keys() {
        let (number, suffix) = name
            .rsplit_once('.')
            .ok_or_else(|| anyhow!("model init expects N.weight or N.bias, found {name}"))?;
        let index = number
            .parse::<u64>()
            .map_err(|_| anyhow!("model init expects numeric layer names, found {name}"))?;
        if number != index.to_string() || !matches!(suffix, "weight" | "bias") {
            bail!("model init expects canonical N.weight or N.bias, found {name}");
        }
        if suffix == "weight" {
            ordered.insert(index, name.clone());
        } else if !weights.tensors.contains_key(&format!("{number}.weight")) {
            bail!("bias {name} has no matching weight");
        }
    }
    if ordered.is_empty() {
        bail!("no N.weight tensors were found");
    }
    let count = ordered.len();
    let mut layers = Vec::new();
    let mut previous = None;
    for (position, (number, name)) in ordered.into_iter().enumerate() {
        let bias = format!("{number}.bias");
        let layer = Layer::Linear {
            weight: name,
            bias: weights.tensors.contains_key(&bias).then_some(bias),
        };
        let (rows, columns) =
            linear_shape(weights, &layer).with_context(|| format!("layer {}", position + 1))?;
        if let Some(width) = previous.filter(|width| *width != columns) {
            bail!(
                "layer {} expects {columns} inputs but the previous layer has {width} outputs",
                position + 1
            );
        }
        previous = Some(rows);
        layers.push(layer);
        if let Some(op) = if position + 1 == count {
            &final_activation
        } else {
            &hidden
        } {
            layers.push(op.clone());
        }
    }
    Ok(ModelFile { layers })
}

fn linear_shape(weights: &Weights, layer: &Layer) -> Result<(usize, usize)> {
    let Layer::Linear { weight, bias } = layer else {
        bail!("expected a linear layer")
    };
    let descriptor = weights
        .tensors
        .get(weight)
        .ok_or_else(|| anyhow!("missing weight tensor {weight}"))?;
    let [rows, columns] = descriptor.shape.as_slice() else {
        bail!("weight {weight} must have shape [out, in]")
    };
    if let Some(name) = bias {
        let descriptor = weights
            .tensors
            .get(name)
            .ok_or_else(|| anyhow!("missing bias tensor {name}"))?;
        if descriptor.shape != [*rows] {
            bail!("bias {name} must have shape [{rows}]");
        }
    }
    Ok((*rows, *columns))
}

fn read_rows(weights: &Weights, name: &str, start: usize, count: usize) -> Result<TensorBytes> {
    let descriptor = weights
        .tensors
        .get(name)
        .ok_or_else(|| anyhow!("missing tensor {name}"))?;
    let columns = descriptor.shape.get(1).copied().unwrap_or(1);
    let width = storage_width(&descriptor.dtype)?;
    if start
        .checked_add(count)
        .is_none_or(|end| end > descriptor.shape[0])
    {
        bail!("row range is outside tensor {name}");
    }
    let bytes = count
        .checked_mul(columns)
        .and_then(|n| n.checked_mul(width))
        .ok_or_else(|| anyhow!("tensor row size overflow"))?;
    if bytes > managed_function_runtime::artifacts::MAX_TENSOR_ARTIFACT_BYTES {
        bail!("weight rows exceed the per-item artifact limit; split fewer rows");
    }
    let mut file = File::open(&weights.path)?;
    file.seek(SeekFrom::Start(
        weights.data_start + descriptor.data_offsets[0] + (start * columns * width) as u64,
    ))?;
    let mut data = vec![0; bytes];
    file.read_exact(&mut data)?;
    let mut shape = descriptor.shape.clone();
    shape[0] = count;
    Ok(TensorBytes {
        dtype: descriptor.dtype.clone(),
        shape,
        data,
    })
}

fn decoded_size(tensor: &TensorBytes) -> Result<usize> {
    let width = storage_width(&tensor.dtype)?;
    let mut count = 0_usize;
    let mut data_bytes = 0;
    for chunk in tensor.data.chunks_exact(width) {
        let number = if width == 4 {
            f32::from_le_bytes(
                chunk
                    .try_into()
                    .map_err(|_| anyhow!("invalid F32 weight byte width"))?,
            ) as f64
        } else {
            f64::from_le_bytes(
                chunk
                    .try_into()
                    .map_err(|_| anyhow!("invalid F64 weight byte width"))?,
            )
        };
        if !number.is_finite() {
            bail!("weights must contain finite values");
        }
        let number = if number == 0.0 { 0.0 } else { number };
        let mut text = number.to_string();
        if !text.contains(['.', 'e', 'E']) {
            text.push_str(".0");
        }
        data_bytes += text.len();
        count += 1;
    }
    Ok(
        serde_json::to_vec(&serde_json::json!({"data":[],"dtype":"f64","shape":tensor.shape}))?
            .len()
            + data_bytes
            + count.saturating_sub(1),
    )
}

/// Each block is bounded by its decoded canonical JSON size, not storage bytes.
fn weight_blocks(
    weights: &Weights,
    name: &str,
    start: usize,
    count: usize,
    cap: usize,
) -> Result<Vec<TensorBytes>> {
    let descriptor = &weights.tensors[name];
    let columns = descriptor.shape[1];
    if columns > COLLECTION_ITEMS {
        bail!("one weight row has {columns} positions; it cannot fit in a task");
    }
    let stored = count
        .checked_mul(columns)
        .and_then(|n| n.checked_mul(storage_width(&descriptor.dtype).ok()?))
        .ok_or_else(|| anyhow!("weight size overflow"))?;
    if stored > managed_function_runtime::artifacts::MAX_TENSOR_ARTIFACT_BYTES {
        bail!("weight rows exceed the per-item artifact limit; split fewer rows");
    }
    let mut blocks: Vec<TensorBytes> = Vec::new();
    let mut current = TensorBytes {
        dtype: descriptor.dtype.clone(),
        shape: vec![0, columns],
        data: Vec::new(),
    };
    let mut current_size = 0;
    for row in start..start + count {
        let part = read_rows(weights, name, row, 1)?;
        let part_size = decoded_size(&part)?;
        if part_size > cap {
            bail!(
                "one row of {name} needs {part_size} decoded bytes, above --max-tensor-bytes {cap}"
            );
        }
        let overhead =
            serde_json::to_vec(&serde_json::json!({"data":[],"dtype":"f64","shape":[1,columns]}))?
                .len();
        let row_data = part_size - overhead;
        let next_rows = current.shape[0] + 1;
        let next_overhead = serde_json::to_vec(
            &serde_json::json!({"data":[],"dtype":"f64","shape":[next_rows,columns]}),
        )?
        .len();
        let next_size = next_overhead + current_size + row_data + usize::from(current.shape[0] > 0);
        if current.shape[0] > 0 && (next_size > cap || next_rows * columns > COLLECTION_ITEMS) {
            blocks.push(current);
            current = TensorBytes {
                dtype: descriptor.dtype.clone(),
                shape: vec![0, columns],
                data: Vec::new(),
            };
            current_size = 0;
        }
        current_size += row_data + usize::from(current.shape[0] > 0);
        current.shape[0] += 1;
        current.data.extend_from_slice(&part.data);
    }
    if current.shape[0] > 0 {
        blocks.push(current);
    }
    Ok(blocks)
}

fn artifact_bytes(tensors: &BTreeMap<String, TensorBytes>) -> Result<Vec<u8>> {
    let mut header = BTreeMap::new();
    let mut data = Vec::new();
    for (name, tensor) in tensors {
        let start = data.len();
        data.extend_from_slice(&tensor.data);
        header.insert(
            name.clone(),
            Descriptor {
                dtype: tensor.dtype.clone(),
                shape: tensor.shape.clone(),
                data_offsets: [start as u64, data.len() as u64],
            },
        );
    }
    let mut header = serde_json::to_vec(&header)?;
    while header.len() % 8 != 0 {
        header.push(b' ');
    }
    let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
    bytes.extend(header);
    bytes.extend(data);
    Ok(bytes)
}

#[derive(Clone)]
struct Operation {
    index: usize,
    range: Option<(usize, usize)>,
}

struct Candidate {
    source: String,
    artifact: Option<Vec<u8>>,
    output: Value,
    usage: u64,
    input_bytes: usize,
    output_bytes: usize,
    first_layer: usize,
    last_layer: usize,
}

#[allow(clippy::too_many_arguments)]
fn candidate(
    model: &ModelFile,
    weights: &Weights,
    operations: &[Operation],
    previous: &[Value],
    first: bool,
    final_output: bool,
    batch: usize,
    options: &SplitOptions,
) -> Result<Candidate> {
    let input = serde_json::to_string(&serde_json::json!({"previous":previous}))?;
    if input.len() > VALUE_BYTES {
        bail!("composed input needs {} bytes, above the {VALUE_BYTES} byte safety bound; use a smaller sample batch",input.len());
    }
    let mut source = if first {
        "let x = transpose(cast(tensor(get(input, \"previous\")[0]), \"f64\"));\n".to_owned()
    } else if previous.len() == 1 {
        "let x = tensor(get(input, \"previous\")[0]);\n".to_owned()
    } else {
        "let pieces = [];\nfor part in get(input, \"previous\") { let pieces = append(pieces, tensor(part)); }\nlet x = concat(pieces);\n".to_owned()
    };
    let mut tensors = BTreeMap::new();
    for operation in operations {
        match &model.layers[operation.index] {
            Layer::Linear { weight, bias } => {
                let (rows, _) = linear_shape(weights, &model.layers[operation.index])?;
                let (start, count) = operation.range.unwrap_or((0, rows));
                let blocks =
                    weight_blocks(weights, weight, start, count, options.max_tensor_bytes)?;
                let mut row_start = start;
                let mut names = Vec::new();
                for (block_index, block) in blocks.into_iter().enumerate() {
                    let name = format!("w{}_{}", operation.index, block_index);
                    let variable = format!("y{}_{}", operation.index, block_index);
                    let block_rows = block.shape[0];
                    source.push_str(&format!(
                        "let {variable} = matmul(load_tensor(\"model\", \"{name}\"), x);\n"
                    ));
                    tensors.insert(name, block);
                    if let Some(bias) = bias {
                        let name = format!("b{}_{}", operation.index, block_index);
                        let bias = read_rows(weights, bias, row_start, block_rows)?;
                        if decoded_size(&bias)? > options.max_tensor_bytes {
                            bail!("bias rows exceed --max-tensor-bytes; split fewer rows");
                        }
                        source.push_str(&format!("let {variable} = {variable} + matmul(reshape(load_tensor(\"model\", \"{name}\"), [{block_rows}, 1]), zeros([1, {batch}], \"f64\") + 1.0);\n"));
                        tensors.insert(name, bias);
                    }
                    names.push(variable);
                    row_start += block_rows;
                }
                source.push_str(&if names.len() == 1 {
                    format!("let x = {};\n", names[0])
                } else {
                    format!("let x = concat([{}]);\n", names.join(", "))
                });
            }
            Layer::Relu => source.push_str("let x = relu(x);\n"),
            Layer::Gelu => source.push_str("let x = gelu(x);\n"),
            Layer::Softmax => source.push_str("let x = transpose(softmax(transpose(x)));\n"),
        }
    }
    source.push_str(if final_output {
        "return tolist(transpose(x));\n"
    } else {
        "return tolist(x);\n"
    });
    if source.len() > hivemind_proto::MANAGED_TASK_SOURCE_MAX_BYTES {
        bail!("generated source exceeds the task source bound; pack fewer layers or rows");
    }
    let artifact = if tensors.is_empty() {
        None
    } else {
        Some(artifact_bytes(&tensors)?)
    };
    let mut artifacts = TensorArtifacts::default();
    if let Some(bytes) = &artifact {
        artifacts
            .insert("model".into(), bytes.clone(), &sha256_digest(bytes))
            .map_err(|error| anyhow!("generated artifact is too large or invalid: {error}"))?;
    }
    let execution_limits = ExecutionLimits {
        max_value_bytes: VALUE_BYTES as u64,
        max_collection_items: COLLECTION_ITEMS as u64,
        max_value_materialization_bytes: options.max_materialization_bytes,
        max_output_bytes: OUTPUT_BYTES as u64,
        ..ExecutionLimits::for_managed_function_budget(options.max_usage_units)
    };
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let (sender, receiver) = std::sync::mpsc::channel::<()>();
    let result = std::thread::scope(|scope| {
        let flag = &cancelled;
        scope.spawn(move || {
            if receiver.recv_timeout(std::time::Duration::from_secs(10))
                == Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        let result = ManagedExecutor.execute_json_input_with_artifacts(
            &source,
            execution_limits,
            &input,
            &artifacts,
            Some(&cancelled),
        );
        drop(sender);
        result
    })
    .map_err(|error| anyhow!("local item cannot fit its limits: {error}"))?;
    let rendered =
        managed_function_runtime::render_output_bounded(&result.value, OUTPUT_BYTES as u64)
            .map_err(|error| anyhow!("local item output is too large: {error}"))?;
    let output = serde_json::from_str(&rendered).context("generated item did not return JSON")?;
    Ok(Candidate {
        source,
        artifact,
        output,
        usage: result.receipt.usage_units,
        input_bytes: input.len(),
        output_bytes: rendered.len(),
        first_layer: operations
            .first()
            .map(|op| op.index + 1)
            .unwrap_or(model.layers.len()),
        last_layer: operations
            .last()
            .map(|op| op.index + 1)
            .unwrap_or(model.layers.len()),
    })
}

fn sample_shape(model: &ModelFile, weights: &Weights, input: &Value) -> Result<usize> {
    if model.layers.is_empty() {
        bail!("model must contain at least one layer");
    }
    let samples = input
        .as_array()
        .filter(|samples| !samples.is_empty())
        .ok_or_else(|| anyhow!("--input must be a nonempty batch [[...], ...]"))?;
    let mut width = samples[0]
        .as_array()
        .filter(|row| !row.is_empty())
        .ok_or_else(|| anyhow!("each sample must be a nonempty numeric row"))?
        .len();
    for row in samples {
        let row = row
            .as_array()
            .filter(|row| row.len() == width)
            .ok_or_else(|| anyhow!("input sample rows must have the same width"))?;
        if row
            .iter()
            .any(|value| value.as_f64().is_none_or(|number| !number.is_finite()))
        {
            bail!("input rows must contain finite numbers");
        }
    }
    for (index, layer) in model.layers.iter().enumerate() {
        if matches!(layer, Layer::Linear { .. }) {
            let (rows, columns) =
                linear_shape(weights, layer).with_context(|| format!("layer {}", index + 1))?;
            if columns != width {
                bail!(
                    "layer {} expects {columns} input features, received {width}",
                    index + 1
                );
            }
            width = rows;
        }
    }
    Ok(samples.len())
}

fn append_stage(
    generated: &mut Generated,
    items: Vec<Candidate>,
    replicas: u16,
    measurements: &mut Vec<Value>,
) -> Result<Vec<Value>> {
    let stage_index = generated.pipeline.stages.len() + 1;
    let mut stage = Stage { items: Vec::new() };
    let mut outputs = Vec::new();
    for (index, candidate) in items.into_iter().enumerate() {
        let name = format!("stage-{stage_index}-item-{}", index + 1);
        let source = format!("{name}.hcode");
        generated
            .files
            .insert(source.clone(), candidate.source.into_bytes());
        let mut artifacts = BTreeMap::new();
        if let Some(bytes) = candidate.artifact {
            let path = format!("{name}.safetensors");
            generated.files.insert(path.clone(), bytes);
            artifacts.insert("model".into(), path);
        }
        let cap = required_task_budget(candidate.usage, replicas)?.task_budget;
        measurements.push(serde_json::json!({"stage":stage_index,"item":index+1,"first_layer":candidate.first_layer,"last_layer":candidate.last_layer,"usage_per_replica":candidate.usage,"replicas":replicas,"max_cpt":cap,"input_bytes":candidate.input_bytes,"output_bytes":candidate.output_bytes}));
        outputs.push(candidate.output);
        stage.items.push(StageItem {
            source,
            params: None,
            params_file: None,
            artifacts,
            max_cpt: Some(cap),
        });
    }
    generated.pipeline.stages.push(stage);
    Ok(outputs)
}

fn generate(
    model: &ModelFile,
    weights: &Weights,
    input: Value,
    options: &SplitOptions,
) -> Result<Generated> {
    validate_options(options)?;
    let batch = sample_shape(model, weights, &input)?;
    let mut generated = Generated {
        pipeline: PipelineFile {
            input: input.clone(),
            stages: Vec::new(),
        },
        files: BTreeMap::new(),
        output: Value::Null,
    };
    let mut previous = vec![input];
    let mut cursor = 0;
    let mut measurements = Vec::new();
    while cursor < model.layers.len() {
        let first = generated.pipeline.stages.is_empty();
        let mut accepted = None;
        let mut failure = String::new();
        let mut operations = Vec::new();
        // Keep the furthest complete prefix that fits. An earlier prefix may
        // fail only while exporting its large hidden tensor; a later reducing
        // layer can avoid that output and its temporary materialization.
        for end in cursor..model.layers.len() {
            operations.push(Operation {
                index: end,
                range: None,
            });
            match candidate(
                model,
                weights,
                &operations,
                &previous,
                first,
                end + 1 == model.layers.len(),
                batch,
                options,
            ) {
                Ok(item) => accepted = Some((end + 1, item)),
                Err(error) => {
                    failure = error.to_string();
                }
            }
        }
        if let Some((end, item)) = accepted {
            previous = append_stage(
                &mut generated,
                vec![item],
                options.replicas,
                &mut measurements,
            )?;
            cursor = end;
            continue;
        }
        if !matches!(model.layers[cursor], Layer::Linear { .. }) {
            bail!(
                "layer {} cannot fit in one task: {failure}; reduce the sample batch",
                cursor + 1
            );
        }
        let (rows, _) = linear_shape(weights, &model.layers[cursor])?;
        let mut end = cursor + 1;
        while end < model.layers.len() && matches!(model.layers[end], Layer::Relu | Layer::Gelu) {
            end += 1;
        }
        let mut start = 0;
        let mut items = Vec::new();
        // A failed complete layer is retried with successively smaller row ranges.
        // Pointwise activations may travel with a shard; softmax waits for all rows.
        while start < rows {
            let mut count = rows - start;
            loop {
                let mut operations = vec![Operation {
                    index: cursor,
                    range: Some((start, count)),
                }];
                operations.extend((cursor + 1..end).map(|index| Operation { index, range: None }));
                match candidate(model,weights,&operations,&previous,first,false,batch,options) {
                    Ok(item)=>{items.push(item);start+=count;break;},
                    Err(error) if count==1=>bail!("layer {} row {} cannot fit in one task: {error}; increase the local budget or reduce the sample batch",cursor+1,start+1),
                    Err(_)=>count=(count/2).max(1),
                }
            }
        }
        previous = append_stage(&mut generated, items, options.replicas, &mut measurements)?;
        cursor = end;
        if cursor == model.layers.len() {
            let item = candidate(model, weights, &[], &previous, false, true, batch, options)
                .map_err(|error| {
                    anyhow!(
                        "layer {cursor} final merge cannot fit: {error}; reduce the sample batch"
                    )
                })?;
            previous = append_stage(
                &mut generated,
                vec![item],
                options.replicas,
                &mut measurements,
            )?;
        }
    }
    generated.output = previous
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("model produced no output"))?;
    generated.files.insert("measurements.json".into(),serde_json::to_vec_pretty(&serde_json::json!({"local_same_machine":true,"value_byte_limit":VALUE_BYTES,"output_byte_limit":OUTPUT_BYTES,"max_tensor_bytes":options.max_tensor_bytes,"max_materialization_bytes":options.max_materialization_bytes,"items":measurements}))?);
    Ok(generated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("hivemind-model-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn fixture(tensors: &[(&str, Vec<usize>, Vec<f64>, bool)]) -> (PathBuf, Weights) {
        let dir = temp_dir();
        let mut header = serde_json::Map::new();
        let mut data = Vec::new();
        for (name, shape, values, f32_storage) in tensors {
            let start = data.len();
            for value in values {
                if *f32_storage {
                    data.extend_from_slice(&(*value as f32).to_le_bytes());
                } else {
                    data.extend_from_slice(&value.to_le_bytes());
                }
            }
            header.insert((*name).into(), json!({"dtype": if *f32_storage { "F32" } else { "F64" }, "shape":shape,"data_offsets":[start,data.len()]}));
        }
        let header = serde_json::to_vec(&header).unwrap();
        let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&data);
        let path = dir.join("weights.safetensors");
        std::fs::write(&path, bytes).unwrap();
        let weights = read_weights(&path).unwrap();
        (dir, weights)
    }

    fn model_fixture() -> (PathBuf, Weights, ModelFile, Value) {
        let (dir, weights) = fixture(&[
            (
                "0.weight",
                vec![16, 8],
                (0..128).map(|n| (n as f64 - 63.0) / 80.0).collect(),
                true,
            ),
            (
                "0.bias",
                vec![16],
                (0..16).map(|n| (n as f64 - 8.0) / 20.0).collect(),
                false,
            ),
            (
                "2.weight",
                vec![3, 16],
                (0..48).map(|n| (n as f64 - 23.0) / 40.0).collect(),
                false,
            ),
            ("2.bias", vec![3], vec![0.1, -0.2, 0.3], false),
        ]);
        let model = ModelFile {
            layers: vec![
                Layer::Linear {
                    weight: "0.weight".into(),
                    bias: Some("0.bias".into()),
                },
                Layer::Relu,
                Layer::Linear {
                    weight: "2.weight".into(),
                    bias: Some("2.bias".into()),
                },
                Layer::Softmax,
            ],
        };
        (
            dir,
            weights,
            model,
            json!([
                [0.2, -0.1, 0.3, 0.7, 0.0, 0.4, -0.6, 0.9],
                [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7]
            ]),
        )
    }

    fn values(value: &Value) -> Vec<f64> {
        value
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|row| row.as_array().unwrap())
            .map(|v| v.as_f64().unwrap())
            .collect()
    }

    #[test]
    fn model_arguments_parse_and_reject_unknown_or_missing_options() {
        let args: Vec<String> = [
            "hivemind",
            "model",
            "init",
            "weights.safetensors",
            "--activation",
            "gelu",
            "--final",
            "softmax",
            "--out",
            "model.json",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let ModelCommand::Init(init) = parse_model_args(&args).unwrap() else {
            panic!("expected init");
        };
        assert_eq!(init.activation, "gelu");
        assert_eq!(init.final_activation, "softmax");
        for args in [
            vec!["hivemind", "model"],
            vec!["hivemind", "model", "init", "w", "--activation", "tanh"],
            vec!["hivemind", "model", "split", "m", "--weights", "w"],
            vec!["hivemind", "model", "init", "w", "--typo", "x"],
        ] {
            assert!(
                parse_model_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).is_err()
            );
        }
    }

    #[test]
    fn import_orders_numeric_names_and_uses_explicit_activations() {
        let (_dir, weights) = fixture(&[
            ("10.weight", vec![1, 2], vec![1.0, 2.0], false),
            ("2.weight", vec![2, 2], vec![1.0, 0.0, 0.0, 1.0], true),
            ("10.bias", vec![1], vec![0.5], false),
        ]);
        assert_eq!(
            infer_model(&weights, "gelu", "softmax").unwrap(),
            ModelFile {
                layers: vec![
                    Layer::Linear {
                        weight: "2.weight".into(),
                        bias: None
                    },
                    Layer::Gelu,
                    Layer::Linear {
                        weight: "10.weight".into(),
                        bias: Some("10.bias".into())
                    },
                    Layer::Softmax,
                ]
            }
        );
    }

    #[test]
    fn split_and_packed_models_are_bitwise_equal_and_match_an_independent_reference() {
        let (_dir, weights, model, input) = model_fixture();
        let packed = generate(&model, &weights, input.clone(), &SplitOptions::default()).unwrap();
        assert_eq!(
            packed.pipeline.stages.len(),
            1,
            "small adjacent layers should be packed"
        );
        let options = SplitOptions {
            max_tensor_bytes: 240,
            max_materialization_bytes: 8000,
            ..SplitOptions::default()
        };
        let split = generate(&model, &weights, input.clone(), &options).unwrap();
        assert!(split.pipeline.stages.len() > 1);
        assert!(
            split.pipeline.stages.iter().any(|s| s.items.len() > 1),
            "the weight rows should be distributed between tasks"
        );
        let actual = values(&split.output);
        assert_eq!(
            actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            values(&packed.output)
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
        let mut reference = Vec::new();
        for row in input.as_array().unwrap() {
            let x: Vec<f64> = row
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let hidden: Vec<f64> = (0..16)
                .map(|i| {
                    let y = (0..8).fold(0.0, |sum, j| {
                        sum + (((i * 8 + j) as f64 - 63.0) / 80.0) as f32 as f64 * x[j]
                    });
                    (y + (i as f64 - 8.0) / 20.0).max(0.0)
                })
                .collect();
            let logits: Vec<f64> = (0..3)
                .map(|i| {
                    (0..16).fold(0.0, |sum, j| {
                        sum + ((i * 16 + j) as f64 - 23.0) / 40.0 * hidden[j]
                    }) + [0.1, -0.2, 0.3][i]
                })
                .collect();
            let max = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let exps: Vec<f64> = logits.iter().map(|v| (v - max).exp()).collect();
            let sum: f64 = exps.iter().sum();
            reference.extend(exps.iter().map(|v| v / sum));
        }
        for (actual, expected) in actual.iter().zip(reference) {
            assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
        }
        for stage in &split.pipeline.stages {
            for item in &stage.items {
                assert!(item.max_cpt.unwrap() > 0);
                assert!(split.files.contains_key(&item.source));
                for path in item.artifacts.values() {
                    assert!(split.files.contains_key(path));
                }
            }
        }
    }

    #[test]
    fn oversized_rows_and_incompatible_shapes_identify_the_layer() {
        let (_dir, weights, model, input) = model_fixture();
        let options = SplitOptions {
            max_tensor_bytes: 50,
            ..SplitOptions::default()
        };
        let error = generate(&model, &weights, input, &options)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("layer 1") && error.contains("row"),
            "{error}"
        );
        let error = generate(
            &model,
            &weights,
            json!([[1.0, 2.0]]),
            &SplitOptions::default(),
        )
        .err()
        .unwrap()
        .to_string();
        assert!(error.contains("layer 1") && error.contains("8"), "{error}");
    }

    #[test]
    fn invalid_weight_layout_and_non_finite_values_are_rejected() {
        let dir = temp_dir();
        let path = dir.join("bad.safetensors");
        std::fs::write(&path, b"broken").unwrap();
        assert!(read_weights(&path).is_err());
        let (_dir, weights) = fixture(&[("0.weight", vec![1, 1], vec![f64::NAN], false)]);
        let model = ModelFile {
            layers: vec![Layer::Linear {
                weight: "0.weight".into(),
                bias: None,
            }],
        };
        let error = generate(&model, &weights, json!([[1.0]]), &SplitOptions::default())
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("finite"), "{error}");
    }

    #[test]
    fn generated_artifacts_contain_only_used_rows_and_worker_runs_respect_measured_caps() {
        let (_dir, weights, model, input) = model_fixture();
        let options = SplitOptions {
            max_tensor_bytes: 240,
            max_materialization_bytes: 8000,
            ..SplitOptions::default()
        };
        let generated = generate(&model, &weights, input.clone(), &options).unwrap();
        let mut previous = vec![input];
        for stage in &generated.pipeline.stages {
            let mut outputs = Vec::new();
            for item in &stage.items {
                let mut artifacts = TensorArtifacts::default();
                for (name, path) in &item.artifacts {
                    let bytes = &generated.files[path];
                    artifacts
                        .insert(name.clone(), bytes.clone(), &sha256_digest(bytes))
                        .unwrap();
                    let header_len = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
                    let header: Value = serde_json::from_slice(&bytes[8..8 + header_len]).unwrap();
                    let source = String::from_utf8(generated.files[&item.source].clone()).unwrap();
                    for tensor_name in header.as_object().unwrap().keys() {
                        assert!(
                            source.contains(&format!("\"{tensor_name}\"")),
                            "unused tensor {tensor_name} in {path}"
                        );
                    }
                }
                let input =
                    serde_json::to_string(&serde_json::json!({"previous": previous})).unwrap();
                let source = std::str::from_utf8(&generated.files[&item.source]).unwrap();
                let report = managed_function_runtime::dry_run::dry_run_with_artifacts(
                    source,
                    &input,
                    &artifacts,
                    managed_function_runtime::dry_run::DryRunLimits::default(),
                )
                .unwrap();
                assert_eq!(
                    required_task_budget(report.receipt.usage_units, options.replicas)
                        .unwrap()
                        .task_budget,
                    item.max_cpt.unwrap()
                );
                assert!(report.output.len() <= OUTPUT_BYTES);
                assert!(input.len() <= VALUE_BYTES);
                outputs.push(serde_json::from_str(&report.output).unwrap());
            }
            previous = outputs;
        }
        assert_eq!(previous, vec![generated.output]);
    }

    #[test]
    fn gelu_and_bias_free_layers_keep_their_meaning_when_split() {
        let (_dir, weights) = fixture(&[(
            "0.weight",
            vec![4, 2],
            vec![1.0, -1.0, 0.5, 2.0, -3.0, 1.0, 4.0, -2.0],
            false,
        )]);
        let model = ModelFile {
            layers: vec![
                Layer::Linear {
                    weight: "0.weight".into(),
                    bias: None,
                },
                Layer::Gelu,
            ],
        };
        let input = serde_json::json!([[0.25, -0.5]]);
        let generated = generate(&model, &weights, input, &SplitOptions::default()).unwrap();
        for (actual, x) in values(&generated.output)
            .iter()
            .zip([0.75_f64, -0.875, -1.25, 2.0])
        {
            let expected = 0.5
                * x
                * (1.0 + (std::f64::consts::FRAC_2_PI.sqrt() * (x + 0.044715 * x * x * x)).tanh());
            assert!((actual - expected).abs() < 1e-12);
        }
    }

    #[test]
    fn json_stage_boundaries_preserve_rendered_binary64_values() {
        for index in 0..1000 {
            let original = (f64::from(index) - 500.0) / 397.0;
            let rendered = format!("{original:?}");
            let parsed: f64 = serde_json::from_str(&rendered).unwrap();
            assert_eq!(
                parsed.to_bits(),
                original.to_bits(),
                "CLI output parsing changed {rendered}"
            );
            let report = ManagedExecutor
                .execute_json_input(
                    "return input;",
                    ExecutionLimits::for_managed_function_budget(100),
                    &rendered,
                )
                .unwrap();
            let managed_function_runtime::Value::Float(parsed) = report.value else {
                panic!("expected float");
            };
            assert_eq!(
                parsed.to_bits(),
                original.to_bits(),
                "Worker input parsing changed {rendered}"
            );
        }
    }

    #[test]
    fn packing_keeps_large_intermediates_local_when_the_next_layer_reduces_them() {
        let (_dir, weights) = fixture(&[
            (
                "0.weight",
                vec![32, 1],
                (1..=32).map(|n| f64::from(n) / 397.0).collect(),
                false,
            ),
            ("1.weight", vec![1, 32], vec![1.0 / 397.0; 32], false),
        ]);
        let model = ModelFile {
            layers: vec![
                Layer::Linear {
                    weight: "0.weight".into(),
                    bias: None,
                },
                Layer::Linear {
                    weight: "1.weight".into(),
                    bias: None,
                },
            ],
        };
        let input = serde_json::json!(vec![vec![0.123456789_f64]; 512]);
        let options = SplitOptions::default();
        let operations = [
            Operation {
                index: 0,
                range: None,
            },
            Operation {
                index: 1,
                range: None,
            },
        ];
        assert!(candidate(
            &model,
            &weights,
            &operations,
            std::slice::from_ref(&input),
            true,
            true,
            512,
            &options
        )
        .is_ok());
        let generated = generate(&model, &weights, input, &options).unwrap();
        assert_eq!(generated.pipeline.stages.len(),1,"an unexported hidden tensor may exceed the output bound while the complete item still fits");
    }

    #[tokio::test]
    async fn two_replica_caps_pass_pipeline_check_at_the_requested_replica_count() {
        let (directory, weights, model, input) = model_fixture();
        let options = SplitOptions {
            replicas: 2,
            ..SplitOptions::default()
        };
        let generated = generate(&model, &weights, input, &options).unwrap();
        let measurements: Value =
            serde_json::from_slice(&generated.files["measurements.json"]).unwrap();
        let usage = measurements["items"][0]["usage_per_replica"]
            .as_u64()
            .unwrap();
        assert_eq!(
            generated.pipeline.stages[0].items[0].max_cpt,
            Some(required_task_budget(usage, 2).unwrap().task_budget)
        );
        assert_eq!(measurements["items"][0]["replicas"], 2);
        for (name, bytes) in &generated.files {
            std::fs::write(directory.join(name), bytes).unwrap();
        }
        let path = directory.join("pipeline.json");
        std::fs::write(&path, serde_json::to_vec(&generated.pipeline).unwrap()).unwrap();
        super::super::pipeline::run_check(&super::super::pipeline::CheckOptions {
            path: path.to_string_lossy().into_owned(),
            max_usage_units: options.max_usage_units,
            timeout_secs: 10,
            replicas: 2,
        })
        .await
        .unwrap();
    }
}
