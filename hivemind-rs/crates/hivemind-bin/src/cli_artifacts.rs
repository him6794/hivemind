use anyhow::{anyhow, bail, Result};
use general_compute_runtime::{
    hcode_artifacts::{
        TensorInputEnvelope, CHUNK_BYTES, MAX_ARTIFACTS, MAX_FILE_BYTES, MAX_TOTAL_BYTES,
    },
    sha256_digest, validate_artifact_id, ArtifactChunk, ArtifactManifest, ArtifactRole,
};
use tokio::io::AsyncReadExt;

pub(super) fn parse_spec(spec: &str) -> Result<(String, String)> {
    let (name, path) = spec
        .split_once('=')
        .ok_or_else(|| anyhow!("--artifact requires name=path.safetensors"))?;
    validate_artifact_id(name).map_err(anyhow::Error::msg)?;
    if path.trim().is_empty() {
        bail!("--artifact path must not be empty");
    }
    Ok((name.to_owned(), path.to_owned()))
}

pub(super) struct PreparedArtifacts {
    pub input_json: String,
    files: Vec<(ArtifactManifest, Vec<u8>)>,
}

impl PreparedArtifacts {
    pub async fn prepare(input: &str, specs: &[(String, String)]) -> Result<Option<Self>> {
        if specs.is_empty() {
            return Ok(None);
        }
        if specs.len() > MAX_ARTIFACTS {
            bail!("too many tensor artifacts");
        }
        let input: serde_json::Value = serde_json::from_str(if input.trim().is_empty() {
            "null"
        } else {
            input
        })?;
        let mut files = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        let mut total = 0;
        for (name, path) in specs {
            validate_artifact_id(name).map_err(anyhow::Error::msg)?;
            if !names.insert(name) {
                bail!("duplicate artifact name: {name}");
            }
            let file = tokio::fs::File::open(path)
                .await
                .map_err(|error| anyhow!("cannot open artifact {path}: {error}"))?;
            let metadata = file.metadata().await?;
            if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
                bail!("artifact must be a regular file of at most 64 MiB: {path}");
            }
            let mut bytes = Vec::new();
            file.take(MAX_FILE_BYTES + 1)
                .read_to_end(&mut bytes)
                .await?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                bail!("artifact exceeds 64 MiB: {path}");
            }
            total += bytes.len() as u64;
            if total > MAX_TOTAL_BYTES {
                bail!("tensor artifacts exceed 256 MiB total");
            }
            // Retain exactly the bytes hashed here. A changed file cannot be
            // re-read between manifest submission and chunk upload.
            let chunks = bytes
                .chunks(CHUNK_BYTES)
                .enumerate()
                .map(|(index, chunk)| ArtifactChunk {
                    offset: (index * CHUNK_BYTES) as u64,
                    size_bytes: chunk.len() as u64,
                    sha256: sha256_digest(chunk),
                })
                .collect();
            let manifest = ArtifactManifest {
                artifact_id: name.clone(),
                role: ArtifactRole::Input,
                size_bytes: bytes.len() as u64,
                mime_type: "application/x-safetensors".into(),
                sha256: sha256_digest(&bytes),
                chunks,
                inline_bytes: None,
            };
            files.push((manifest, bytes));
        }
        let envelope = TensorInputEnvelope {
            protocol: 1,
            input,
            artifacts: files.iter().map(|(manifest, _)| manifest.clone()).collect(),
        };
        let input_json = envelope.to_input_json().map_err(anyhow::Error::msg)?;
        Ok(Some(Self { input_json, files }))
    }

    pub async fn upload(
        &self,
        client: &reqwest::Client,
        api: &str,
        token: &str,
        task_id: &str,
    ) -> Result<()> {
        let url = format!("{api}/api/tasks/{task_id}/general-compute/artifacts/chunk");
        for (manifest, bytes) in &self.files {
            for (chunk, payload) in manifest.chunks.iter().zip(bytes.chunks(CHUNK_BYTES)) {
                let response = client
                    .post(&url)
                    .bearer_auth(token)
                    .timeout(std::time::Duration::from_secs(60))
                    .json(&serde_json::json!({
                        "artifact_id": manifest.artifact_id, "offset": chunk.offset,
                        "size_bytes": chunk.size_bytes, "sha256": chunk.sha256, "bytes": payload,
                    }))
                    .send()
                    .await?;
                let status = response.status();
                let body: serde_json::Value = response.json().await?;
                if !status.is_success() || body["success"] != true || body["accepted_chunks"] != 1 {
                    bail!("task {task_id} artifact {} upload failed at {}: {} (task remains pending until inputs are complete or expire)",
                        manifest.artifact_id, chunk.offset, body["message"]);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_flag_preserves_paths_but_rejects_unsafe_names() {
        assert_eq!(
            parse_spec(r"model=C:\models\weights.safetensors").unwrap(),
            ("model".into(), r"C:\models\weights.safetensors".into())
        );
        assert_eq!(parse_spec("model=a=b").unwrap().1, "a=b");
        for value in ["../model=x", "model=", "file.safetensors", "=file"] {
            assert!(parse_spec(value).is_err());
        }
    }

    #[tokio::test]
    async fn prepare_pins_multiple_chunks_and_preserves_user_input() {
        let path =
            std::env::temp_dir().join(format!("hcode-cli-artifact-{}.bin", uuid::Uuid::new_v4()));
        let bytes = vec![7; CHUNK_BYTES + 19];
        tokio::fs::write(&path, &bytes).await.unwrap();
        let specs = vec![("model".into(), path.to_string_lossy().into_owned())];
        let prepared = PreparedArtifacts::prepare("[1,2]", &specs)
            .await
            .unwrap()
            .unwrap();
        tokio::fs::remove_file(&path).await.unwrap();
        assert_eq!(prepared.files[0].1, bytes);
        assert_eq!(prepared.files[0].0.chunks.len(), 2);
        let envelope = general_compute_runtime::hcode_artifacts::parse_input(&prepared.input_json)
            .unwrap()
            .unwrap();
        assert_eq!(envelope.input, serde_json::json!([1, 2]));
        assert_eq!(envelope.artifacts[0].sha256, sha256_digest(&bytes));
        assert!(PreparedArtifacts::prepare("not-json", &specs)
            .await
            .is_err());
    }
}
