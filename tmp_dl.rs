async fn download_task_artifact(command: &TaskLookupCommand) -> Result<()> {
    use std::io::Write;
    let client = reqwest::Client::new();
    let token = login(
        &client,
        &command.api_base,
        &command.username,
        &command.password,
    )
    .await?;
    let url = format!("{}/api/tasks/{}/artifact/download", command.api_base, command.task_id);
    let response = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| anyhow!("artifact download request failed: {}", e))?;
    let status = response.status();
    let headers = response.headers().clone();
    if !status.is_success() {
        let body: serde_json::Value = response.json().await.unwrap_or_default();
        return Err(anyhow!(
            "artifact download failed: {}",
            body.get("message").and_then(|v| v.as_str()).unwrap_or_else(|| status.as_str())
        ));
    }
    let filename = headers
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(|d| d.split("filename=").nth(1))
        .map(|s| s.trim().trim_matches('"'))
        .unwrap_or("artifact.bin")
        .to_string();
    let bytes = response.bytes().await.map_err(|e| anyhow!("download read failed: {}", e))?;
    let mut file = std::fs::File::create(&filename).map_err(|e| anyhow!("cannot create {}: {}", filename, e))?;
    file.write_all(&bytes).map_err(|e| anyhow!("cannot write {}: {}", filename, e))?;
    println!("Downloaded {} ({} bytes)", filename, bytes.len());
    Ok(())
}
