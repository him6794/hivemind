mod hcode_artifacts {
    use super::*;
    use general_compute_runtime::{
        hcode_artifacts::TensorInputEnvelope, sha256_digest, ArtifactChunk, ArtifactManifest,
        ArtifactRole,
    };
    use hivemind_auth::worker_execution::{WorkerExecutionIdentity, WorkerExecutionSigner};
    use hivemind_proto::{
        general_compute_artifact_service_client::GeneralComputeArtifactServiceClient,
        general_compute_artifact_service_server::GeneralComputeArtifactServiceServer,
        HcodeArtifactChunkDownload,
    };

    #[test]
    fn tensor_artifact_admission_rejects_malformed_envelopes() {
        for input in [
            "not-json",
            r#"{"$hcode_tensor_inputs":1,"input":null,"artifacts":[]}"#,
        ] {
            assert!(validate_task_input_contract("hcode", input, &[], &[]).is_err());
        }
        assert!(validate_task_input_contract("hcode", "[1,2]", &[], &[]).is_ok());
    }

    #[tokio::test]
    async fn tensor_artifact_rpc_requires_owner_upload_and_signed_live_replica_download() {
        let Some((master, seed_task, other_token, owner)) = test_service().await else {
            return;
        };
        let db = &master.state.scheduler.database().pool;
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 1000)",
        )
        .bind(&owner)
        .execute(db)
        .await
        .unwrap();
        let bytes = b"immutable tensor bytes";
        let mut task = make_task(&format!("tensor-{seed_task}"), &owner);
        task.runtime = Some("hcode".into());
        task.task_source = Some("return input;".into());
        task.torrent_source = Some(
            TensorInputEnvelope {
                protocol: 1,
                input: serde_json::json!([1, 2]),
                artifacts: vec![ArtifactManifest {
                    artifact_id: "model".into(),
                    role: ArtifactRole::Input,
                    size_bytes: bytes.len() as u64,
                    mime_type: "application/x-safetensors".into(),
                    sha256: sha256_digest(bytes),
                    inline_bytes: None,
                    chunks: vec![ArtifactChunk {
                        offset: 0,
                        size_bytes: bytes.len() as u64,
                        sha256: sha256_digest(bytes),
                    }],
                }],
            }
            .to_input_json()
            .unwrap(),
        );
        let task = master.state.scheduler.create_task(&task).await.unwrap();
        let service = GrpcGeneralComputeArtifactService::new(master.state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(GeneralComputeArtifactServiceServer::new(service))
                .serve_with_incoming_shutdown(
                    tokio_stream::wrappers::TcpListenerStream::new(listener),
                    async {
                        let _ = stopped.await;
                    },
                )
                .await
                .unwrap();
        });
        let mut client = GeneralComputeArtifactServiceClient::connect(endpoint)
            .await
            .unwrap();
        let owner_token = token_for(&master.state.auth, &owner);
        let upload = |token: String| GeneralComputeArtifactChunkUpload {
            token,
            task_id: task.task_id.clone(),
            artifact_id: "model".into(),
            offset: 0,
            size_bytes: bytes.len() as i64,
            sha256: sha256_digest(bytes),
            bytes: bytes.to_vec(),
        };
        assert_eq!(
            client
                .upload_chunk(upload(other_token))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
        assert!(
            client
                .upload_chunk(upload(owner_token.clone()))
                .await
                .unwrap()
                .into_inner()
                .success
        );
        assert!(
            client
                .upload_chunk(upload(owner_token.clone()))
                .await
                .unwrap()
                .into_inner()
                .success
        );
        let mut conflicting = upload(owner_token.clone());
        conflicting.bytes[0] ^= 1;
        conflicting.sha256 = sha256_digest(&conflicting.bytes);
        assert_eq!(
            client.upload_chunk(conflicting).await.unwrap_err().code(),
            tonic::Code::InvalidArgument
        );

        let attempt: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO managed_consensus_attempts
             (task_id, execution_id, round_id, request_digest, replica_count, quorum, mode, state, deadline)
             VALUES ($1, 'tensor-exec', 'tensor-round', 'tensor-digest', 3, 2, 'enforce', 'running', NOW() + INTERVAL '60 seconds') RETURNING id"
        ).bind(&task.task_id).fetch_one(db).await.unwrap();
        sqlx::query(
            "INSERT INTO managed_consensus_replicas
             (task_id, attempt_id, replica_id, worker_id, worker_ip, provider_user, execution_id, worker_attempt_id, request_digest, state)
             VALUES ($1, $2, 'tensor-replica', 'tensor-worker', '127.0.0.1', 'provider', 'tensor-exec', 'tensor-attempt', 'tensor-digest', 'running')"
        ).bind(&task.task_id).bind(attempt).execute(db).await.unwrap();
        sqlx::query("UPDATE tasks SET status = 'RUNNING', managed_consensus_attempt_id = $2 WHERE task_id = $1")
            .bind(&task.task_id).bind(attempt).execute(db).await.unwrap();
        let signer =
            WorkerExecutionSigner::from_pem(&master.state.worker_execution_private_key_pem)
                .unwrap();
        let claims = Claims {
            sub: owner.clone(),
            user_id: owner.clone(),
            role: Some("worker-execution".into()),
            task_id: Some(task.task_id.clone()),
            worker_id: Some("tensor-worker".into()),
            iat: Utc::now().timestamp() as usize,
            exp: (Utc::now().timestamp() + 300) as usize,
        };
        let identity = WorkerExecutionIdentity {
            execution_id: "tensor-exec".into(),
            attempt_id: "tensor-attempt".into(),
            idempotency_key: format!("managed-consensus-v1:{}", task.id.simple()),
            request_digest: "tensor-digest".into(),
            transfer_generation: 1,
        };
        let sign = |claims: &Claims, identity: &WorkerExecutionIdentity| {
            signer
                .encode_consensus_claims(claims, identity, "tensor-round", "tensor-replica", 1)
                .unwrap()
        };
        let token = sign(&claims, &identity);
        let download = |token: String| HcodeArtifactChunkDownload {
            token,
            task_id: task.task_id.clone(),
            artifact_id: "model".into(),
            offset: 0,
        };
        assert_eq!(
            client
                .download_hcode_chunk(download(token.clone()))
                .await
                .unwrap()
                .into_inner()
                .bytes,
            bytes
        );
        for invalid in ["forged".to_string(), owner_token.clone()] {
            assert_eq!(
                client
                    .download_hcode_chunk(download(invalid))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated
            );
        }
        let mut wrong_task = download(token.clone());
        wrong_task.task_id = seed_task.clone();
        assert_eq!(
            client
                .download_hcode_chunk(wrong_task)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
        for change in 0..4 {
            let mut bad_claims = claims.clone();
            let mut bad_identity = identity.clone();
            match change {
                0 => bad_claims.sub = "other-owner".into(),
                1 => bad_claims.role = Some("user".into()),
                2 => bad_claims.worker_id = Some("other-worker".into()),
                _ => bad_identity.attempt_id = "old-attempt".into(),
            }
            assert_eq!(
                client
                    .download_hcode_chunk(download(sign(&bad_claims, &bad_identity)))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::PermissionDenied
            );
        }
        let mut expired = claims.clone();
        expired.exp = (Utc::now().timestamp() - 120) as usize;
        assert_eq!(
            client
                .download_hcode_chunk(download(sign(&expired, &identity)))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        sqlx::query("UPDATE managed_consensus_attempts SET state = 'stop_pending' WHERE id = $1")
            .bind(attempt)
            .execute(db)
            .await
            .unwrap();
        assert_eq!(
            client
                .download_hcode_chunk(download(token))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
        sqlx::query("UPDATE tasks SET status = 'COMPLETED' WHERE task_id = $1")
            .bind(&task.task_id)
            .execute(db)
            .await
            .unwrap();
        assert_eq!(
            client
                .upload_chunk(upload(owner_token))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::FailedPrecondition
        );
        drop(client);
        stop.send(()).unwrap();
        server.await.unwrap();
        cleanup(&master.state.scheduler, &task.task_id, &owner).await;
    }
}
