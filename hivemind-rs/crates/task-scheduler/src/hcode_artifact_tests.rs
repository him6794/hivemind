mod hcode_artifacts {
    use super::*;
    use general_compute_runtime::{
        hcode_artifacts::TensorInputEnvelope, sha256_digest, ArtifactChunk,
    };
    use hivemind_auth::worker_execution::WorkerExecutionClaims;

    fn input(bytes: &[u8]) -> String {
        TensorInputEnvelope {
            protocol: 1,
            input: serde_json::json!([1, 2]),
            artifacts: vec![ArtifactManifest {
                artifact_id: "model".into(),
                role: ArtifactRole::Input,
                size_bytes: bytes.len() as u64,
                mime_type: "application/x-safetensors".into(),
                sha256: sha256_digest(bytes),
                chunks: bytes
                    .chunks(general_compute_runtime::hcode_artifacts::CHUNK_BYTES)
                    .enumerate()
                    .map(|(index, chunk)| ArtifactChunk {
                        offset: (index * general_compute_runtime::hcode_artifacts::CHUNK_BYTES)
                            as u64,
                        size_bytes: chunk.len() as u64,
                        sha256: sha256_digest(chunk),
                    })
                    .collect(),
                inline_bytes: None,
            }],
        }
        .to_input_json()
        .unwrap()
    }

    #[tokio::test]
    async fn tensor_artifact_pins_account_uuid_without_backfilling_recreated_owners() {
        let (p, fixture) = pool("hcode_artifact_owner_uuid")
            .await
            .expect("isolated PostgreSQL required");
        let repo = TaskRepository::new(p);
        let owner: Uuid = sqlx::query_scalar("INSERT INTO users (username, password_hash) VALUES ('tensor-owner', 'hash') RETURNING id")
            .fetch_one(&repo.pool).await.unwrap();
        let mut task = make_task("owner-pinned-task", "tensor-owner");
        task.runtime = Some("hcode".into());
        task.task_source = Some("return input;".into());
        task.torrent_source = Some(input(b"owner-pinned-tensor"));
        repo.create(&task).await.unwrap();
        let saved: Option<Uuid> = sqlx::query_scalar(
            "SELECT owner_user_id FROM general_compute_artifacts WHERE task_id = $1",
        )
        .bind(&task.task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(saved, Some(owner));
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(owner)
            .execute(&repo.pool)
            .await
            .unwrap();
        let replacement: Uuid = sqlx::query_scalar("INSERT INTO users (username, password_hash) VALUES ('tensor-owner', 'new-hash') RETURNING id")
            .fetch_one(&repo.pool).await.unwrap();
        assert_ne!(replacement, owner);
        hivemind_database::postgres::run_migrations(&repo.pool)
            .await
            .unwrap();
        let saved: Option<Uuid> = sqlx::query_scalar(
            "SELECT owner_user_id FROM general_compute_artifacts WHERE task_id = $1",
        )
        .bind(&task.task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(saved, None);
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn tensor_artifact_storage_and_live_replica_authorization() {
        let Some((p, fixture)) = pool("hcode_artifact_authorization").await else {
            return;
        };
        let repo = TaskRepository::new(p);
        sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ('tensor-owner', 'hash', 1000)")
            .execute(&repo.pool).await.unwrap();
        let bytes = b"immutable tensor fixture";
        let mut task = make_task("tensor-task", "tensor-owner");
        task.runtime = Some("hcode".into());
        task.task_source = Some("return input;".into());
        task.torrent_source = Some(input(bytes));
        task.deterministic = true;
        task.max_cpt = 100;
        task.req_memory_gb = 0;
        let task = repo.create(&task).await.unwrap();
        assert!(!repo.hcode_artifacts_ready(&task).await.unwrap());
        assert!(repo.find_pending().await.unwrap().is_empty());
        let ordinary = make_task("ordinary-task", "tensor-owner");
        repo.create(&ordinary).await.unwrap();
        let ready = repo.find_pending().await.unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].task_id, ordinary.task_id);
        assert!(task.deadline.is_some());
        assert_eq!(task.req_memory_gb, 1);
        let mut workers = Vec::new();
        for index in 1..=2 {
            let provider = format!("provider-{index}");
            let worker_id = format!("worker-{index}");
            sqlx::query(
                "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 0)",
            )
            .bind(&provider)
            .execute(&repo.pool)
            .await
            .unwrap();
            insert_worker(&repo.pool, &worker_id, &provider).await;
            let mut worker = make_worker_node(&worker_id, &format!("10.0.0.{index}"));
            worker.username = provider;
            workers.push(worker);
        }
        assert!(repo
            .create_managed_consensus_attempt(&task, &workers, 3, 2, 120, "enforce")
            .await
            .unwrap()
            .is_none());
        let hold_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM managed_consensus_usage_holds WHERE task_id = $1",
        )
        .bind(&task.task_id)
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(hold_count, 0);
        let mut different_weights = task.clone();
        different_weights.torrent_source = Some(input(b"different tensor fixture"));
        assert_ne!(
            consensus_request_digest(&task, "exec", "round"),
            consensus_request_digest(&different_weights, "exec", "round")
        );
        assert!(repo
            .put_general_compute_artifact_chunk(
                &task.task_id,
                "other",
                0,
                bytes.len() as u64,
                &sha256_digest(bytes),
                bytes
            )
            .await
            .is_err());
        assert!(repo
            .put_general_compute_artifact_chunk(
                &task.task_id,
                "model",
                0,
                bytes.len() as u64,
                &sha256_digest(bytes),
                b"tampered"
            )
            .await
            .is_err());
        repo.put_general_compute_artifact_chunk(
            &task.task_id,
            "model",
            0,
            bytes.len() as u64,
            &sha256_digest(bytes),
            bytes,
        )
        .await
        .unwrap();
        assert!(repo.hcode_artifacts_ready(&task).await.unwrap());
        assert!(repo
            .find_pending()
            .await
            .unwrap()
            .iter()
            .any(|ready| ready.task_id == task.task_id));

        let (attempt, assignments) = repo
            .create_managed_consensus_attempt(&task, &workers, 3, 2, 120, "enforce")
            .await
            .unwrap()
            .unwrap();
        let replica = &assignments[0];
        let good = WorkerExecutionClaims {
            claims: hivemind_models::Claims {
                sub: task.owner.clone(),
                user_id: task.owner.clone(),
                role: Some("worker-execution".into()),
                task_id: Some(task.task_id.clone()),
                worker_id: Some(replica.worker_id.clone()),
                exp: usize::MAX,
                iat: 0,
            },
            execution_id: Some(replica.execution_id.clone()),
            attempt_id: Some(replica.worker_attempt_id.clone()),
            idempotency_key: Some(format!("managed-consensus-v1:{}", task.id.simple())),
            request_digest: Some(replica.request_digest.clone()),
            transfer_generation: Some(1),
            consensus_round_id: Some(replica.round_id.clone()),
            replica_id: Some(replica.replica_id.clone()),
            consensus_protocol_version: Some(1),
        };
        assert_eq!(
            repo.hcode_artifact_chunk_for_replica(&good, "model", 0)
                .await
                .unwrap(),
            Some(bytes.to_vec())
        );
        for change in 0..8 {
            let mut bad = good.clone();
            match change {
                0 => bad.claims.sub = "other-owner".into(),
                1 => bad.claims.worker_id = Some("worker-2".into()),
                2 => bad.attempt_id = Some("old-attempt".into()),
                3 => bad.request_digest = Some("other-inputs".into()),
                4 => bad.idempotency_key = None,
                5 => bad.claims.role = Some("user".into()),
                6 => bad.consensus_round_id = Some("old-round".into()),
                _ => bad.replica_id = Some("replica-2".into()),
            }
            assert!(
                repo.hcode_artifact_chunk_for_replica(&bad, "model", 0)
                    .await
                    .unwrap()
                    .is_none(),
                "case {change}"
            );
        }
        assert!(repo
            .hcode_artifact_chunk_for_replica(&good, "model", 1)
            .await
            .unwrap()
            .is_none());
        sqlx::query("UPDATE managed_consensus_attempts SET state = 'stop_pending' WHERE id = $1")
            .bind(attempt.id)
            .execute(&repo.pool)
            .await
            .unwrap();
        assert!(repo
            .hcode_artifact_chunk_for_replica(&good, "model", 0)
            .await
            .unwrap()
            .is_none());
        fixture.cleanup().await.unwrap();
    }

    /// Shared live-assignment fixture for the metadata-only authorization tests:
    /// an uploaded hcode tensor artifact plus an active enforce-mode consensus
    /// attempt and the matching signed execution claims for one replica.
    async fn tensor_authorization_fixture(
        name: &str,
        bytes: &[u8],
    ) -> Option<(
        TaskRepository,
        hivemind_database::postgres::IsolatedTestPool,
        Task,
        crate::task_repository::ManagedConsensusAttempt,
        WorkerExecutionClaims,
    )> {
        let (p, fixture) = pool(name).await?;
        let repo = TaskRepository::new(p);
        sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ('tensor-owner', 'hash', 1000)")
            .execute(&repo.pool)
            .await
            .unwrap();
        let mut task = make_task("tensor-task", "tensor-owner");
        task.runtime = Some("hcode".into());
        task.task_source = Some("return input;".into());
        task.torrent_source = Some(input(bytes));
        task.deterministic = true;
        task.max_cpt = 100;
        task.req_memory_gb = 0;
        let task = repo.create(&task).await.unwrap();
        let mut workers = Vec::new();
        for index in 1..=2 {
            let provider = format!("provider-{index}");
            let worker_id = format!("worker-{index}");
            sqlx::query(
                "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 0)",
            )
            .bind(&provider)
            .execute(&repo.pool)
            .await
            .unwrap();
            insert_worker(&repo.pool, &worker_id, &provider).await;
            let mut worker = make_worker_node(&worker_id, &format!("10.0.0.{index}"));
            worker.username = provider;
            workers.push(worker);
        }
        for chunk in general_compute_runtime::hcode_artifacts::parse_input(
            task.torrent_source.as_deref().unwrap(),
        )
        .unwrap()
        .unwrap()
        .artifacts[0]
            .chunks
            .iter()
        {
            repo.put_general_compute_artifact_chunk(
                &task.task_id,
                "model",
                chunk.offset,
                chunk.size_bytes,
                &chunk.sha256,
                &bytes[chunk.offset as usize..(chunk.offset + chunk.size_bytes) as usize],
            )
            .await
            .unwrap();
        }
        let (attempt, assignments) = repo
            .create_managed_consensus_attempt(&task, &workers, 3, 2, 120, "enforce")
            .await
            .unwrap()
            .unwrap();
        let replica = &assignments[0];
        let good = WorkerExecutionClaims {
            claims: hivemind_models::Claims {
                sub: task.owner.clone(),
                user_id: task.owner.clone(),
                role: Some("worker-execution".into()),
                task_id: Some(task.task_id.clone()),
                worker_id: Some(replica.worker_id.clone()),
                exp: usize::MAX,
                iat: 0,
            },
            execution_id: Some(replica.execution_id.clone()),
            attempt_id: Some(replica.worker_attempt_id.clone()),
            idempotency_key: Some(format!("managed-consensus-v1:{}", task.id.simple())),
            request_digest: Some(replica.request_digest.clone()),
            transfer_generation: Some(1),
            consensus_round_id: Some(replica.round_id.clone()),
            replica_id: Some(replica.replica_id.clone()),
            consensus_protocol_version: Some(1),
        };
        Some((repo, fixture, task, attempt, good))
    }

    #[tokio::test]
    async fn tensor_artifact_authorization_is_metadata_only_and_live_gated() {
        let bytes = b"immutable tensor fixture";
        let Some((repo, fixture, task, attempt, good)) =
            tensor_authorization_fixture("hcode_artifact_authorization_metadata", bytes).await
        else {
            return;
        };
        let expected = crate::task_repository::HcodeArtifactChunkAuthorization {
            artifact_sha256: sha256_digest(bytes),
            artifact_size_bytes: bytes.len() as i64,
            chunk_sha256: sha256_digest(bytes),
            chunk_size_bytes: bytes.len() as i64,
        };
        // A live, correctly bound replica is authorized and the reply matches the
        // pinned immutable manifest exactly.
        assert_eq!(
            repo.authorize_hcode_artifact_chunk(&good, "model", 0)
                .await
                .unwrap(),
            Some(expected.clone())
        );
        // Table-driven live-gate rejections: each single mutation of the signed
        // identity must deny the metadata authorization.
        for (name, bad) in [
            ("other owner", {
                let mut c = good.clone();
                c.claims.sub = "other-owner".into();
                c
            }),
            ("other worker", {
                let mut c = good.clone();
                c.claims.worker_id = Some("worker-2".into());
                c
            }),
            ("other execution", {
                let mut c = good.clone();
                c.execution_id = Some("old-execution".into());
                c
            }),
            ("other attempt", {
                let mut c = good.clone();
                c.attempt_id = Some("old-attempt".into());
                c
            }),
            ("other request", {
                let mut c = good.clone();
                c.request_digest = Some("other-inputs".into());
                c
            }),
            ("missing idempotency", {
                let mut c = good.clone();
                c.idempotency_key = None;
                c
            }),
            ("not a worker execution", {
                let mut c = good.clone();
                c.claims.role = Some("user".into());
                c
            }),
            ("other round", {
                let mut c = good.clone();
                c.consensus_round_id = Some("old-round".into());
                c
            }),
            ("other replica", {
                let mut c = good.clone();
                c.replica_id = Some("replica-2".into());
                c
            }),
            ("other protocol", {
                let mut c = good.clone();
                c.consensus_protocol_version = Some(2);
                c
            }),
            ("missing protocol", {
                let mut c = good.clone();
                c.consensus_protocol_version = None;
                c
            }),
            ("other generation", {
                let mut c = good.clone();
                c.transfer_generation = Some(2);
                c
            }),
            ("missing generation", {
                let mut c = good.clone();
                c.transfer_generation = None;
                c
            }),
            ("other task", {
                let mut c = good.clone();
                c.claims.task_id = Some("other-task".into());
                c
            }),
        ] {
            assert!(
                repo.authorize_hcode_artifact_chunk(&bad, "model", 0)
                    .await
                    .unwrap()
                    .is_none(),
                "authorization should deny {name}"
            );
            assert!(
                repo.hcode_artifact_chunk_for_replica(&bad, "model", 0)
                    .await
                    .unwrap()
                    .is_none(),
                "download should deny {name}"
            );
        }
        // The offset is bound to the pinned manifest; an offset outside it is
        // denied even for the live replica.
        assert!(repo
            .authorize_hcode_artifact_chunk(&good, "model", 1)
            .await
            .unwrap()
            .is_none());
        // Deleting the chunk content must not change the metadata answer: the
        // authorization reads only immutable manifest identity, while the byte
        // download now has nothing to hand back.
        sqlx::query("DELETE FROM general_compute_artifact_chunks WHERE task_id = $1 AND artifact_id = 'model'")
            .bind(&task.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        assert_eq!(
            repo.authorize_hcode_artifact_chunk(&good, "model", 0)
                .await
                .unwrap(),
            Some(expected.clone())
        );
        assert!(repo
            .hcode_artifact_chunk_for_replica(&good, "model", 0)
            .await
            .unwrap()
            .is_none());
        // Restore real content so later rejections cannot pass merely because
        // the download is missing bytes.
        repo.put_general_compute_artifact_chunk(
            &task.task_id,
            "model",
            0,
            bytes.len() as u64,
            &sha256_digest(bytes),
            bytes,
        )
        .await
        .unwrap();
        assert_eq!(
            repo.hcode_artifact_chunk_for_replica(&good, "model", 0)
                .await
                .unwrap(),
            Some(bytes.to_vec())
        );
        // Artifact expiry is a live gate for both the bytes and the metadata.
        sqlx::query("UPDATE general_compute_artifacts SET expires_at = NOW() - INTERVAL '1 second' WHERE task_id = $1")
            .bind(&task.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        assert!(repo
            .authorize_hcode_artifact_chunk(&good, "model", 0)
            .await
            .unwrap()
            .is_none());
        assert!(repo
            .hcode_artifact_chunk_for_replica(&good, "model", 0)
            .await
            .unwrap()
            .is_none());
        // A stopped attempt and a terminal task both drop the live assignment.
        sqlx::query("UPDATE general_compute_artifacts SET expires_at = NOW() + INTERVAL '1 day' WHERE task_id = $1")
            .bind(&task.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE managed_consensus_attempts SET state = 'stop_pending' WHERE id = $1")
            .bind(attempt.id)
            .execute(&repo.pool)
            .await
            .unwrap();
        assert!(repo
            .authorize_hcode_artifact_chunk(&good, "model", 0)
            .await
            .unwrap()
            .is_none());
        sqlx::query("UPDATE managed_consensus_attempts SET state = 'running' WHERE id = $1")
            .bind(attempt.id)
            .execute(&repo.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE tasks SET status = 'COMPLETED' WHERE task_id = $1")
            .bind(&task.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        assert!(repo
            .authorize_hcode_artifact_chunk(&good, "model", 0)
            .await
            .unwrap()
            .is_none());
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn tensor_artifact_authorizes_partial_last_chunk_and_rejects_stale_assignment() {
        let chunk_bytes = general_compute_runtime::hcode_artifacts::CHUNK_BYTES;
        let mut bytes = vec![7; chunk_bytes];
        bytes.extend_from_slice(&[9; 17]);
        let Some((repo, fixture, task, attempt, good)) =
            tensor_authorization_fixture("hcode_artifact_multichunk_authorization", &bytes).await
        else {
            return;
        };
        for offset in [0, chunk_bytes] {
            let end = (offset + chunk_bytes).min(bytes.len());
            let expected = &bytes[offset..end];
            let metadata = repo
                .authorize_hcode_artifact_chunk(&good, "model", offset as i64)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(metadata.artifact_sha256, sha256_digest(&bytes));
            assert_eq!(metadata.artifact_size_bytes, bytes.len() as i64);
            assert_eq!(metadata.chunk_sha256, sha256_digest(expected));
            assert_eq!(metadata.chunk_size_bytes, expected.len() as i64);
            assert_eq!(
                repo.hcode_artifact_chunk_for_replica(&good, "model", offset as i64)
                    .await
                    .unwrap(),
                Some(expected.to_vec())
            );
        }

        // The old rows remain live, but clearing the current-attempt pointer
        // revokes access to both endpoints immediately.
        sqlx::query("UPDATE tasks SET managed_consensus_attempt_id = NULL WHERE task_id = $1")
            .bind(&task.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        assert!(repo
            .authorize_hcode_artifact_chunk(&good, "model", 0)
            .await
            .unwrap()
            .is_none());
        assert!(repo
            .hcode_artifact_chunk_for_replica(&good, "model", 0)
            .await
            .unwrap()
            .is_none());
        sqlx::query("UPDATE tasks SET managed_consensus_attempt_id = $2 WHERE task_id = $1")
            .bind(&task.task_id)
            .bind(attempt.id)
            .execute(&repo.pool)
            .await
            .unwrap();
        assert!(repo
            .authorize_hcode_artifact_chunk(&good, "model", 0)
            .await
            .unwrap()
            .is_some());

        // Corruption cannot change the authorized manifest coordinates or pass
        // the compatibility downloader's independent digest check.
        sqlx::query("UPDATE general_compute_artifact_chunks SET content = $2 WHERE task_id = $1 AND offset_bytes = $3")
            .bind(&task.task_id).bind(vec![8u8; 17]).bind(chunk_bytes as i64)
            .execute(&repo.pool).await.unwrap();
        let metadata = repo
            .authorize_hcode_artifact_chunk(&good, "model", chunk_bytes as i64)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(metadata.chunk_sha256, sha256_digest(&[9; 17]));
        assert!(repo
            .hcode_artifact_chunk_for_replica(&good, "model", chunk_bytes as i64)
            .await
            .is_err());
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn tensor_artifact_quota_serializes_concurrent_submissions() {
        let Some((p, fixture)) = pool("hcode_artifact_concurrent_quota").await else {
            return;
        };
        let repo = TaskRepository::new(p);
        sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ('quota-owner', 'hash', 1000)")
            .execute(&repo.pool).await.unwrap();
        let bytes = b"tensor bytes";
        let mut seed = make_task("seed", "quota-owner");
        seed.runtime = Some("hcode".into());
        seed.task_source = Some("return 1;".into());
        seed.torrent_source = Some(input(bytes));
        repo.create(&seed).await.unwrap();
        sqlx::query("UPDATE general_compute_artifacts SET size_bytes = $1 WHERE task_id = 'seed'")
            .bind(
                general_compute_runtime::hcode_artifacts::OWNER_STORAGE_QUOTA_BYTES
                    - bytes.len() as i64,
            )
            .execute(&repo.pool)
            .await
            .unwrap();
        let mut left = seed.clone();
        left.task_id = "left".into();
        let mut right = seed;
        right.task_id = "right".into();
        let (left, right) = tokio::join!(repo.create(&left), repo.create(&right));
        assert_ne!(left.is_ok(), right.is_ok());
        let rejected = if left.is_err() { left } else { right };
        assert!(rejected.unwrap_err().to_string().contains("quota"));
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn tensor_artifact_quota_is_reserved_before_upload_and_expires() {
        let Some((p, fixture)) = pool("hcode_artifact_quota").await else {
            return;
        };
        let repo = TaskRepository::new(p);
        sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ('quota-owner', 'hash', 1000)")
            .execute(&repo.pool).await.unwrap();
        let mut task = make_task("quota-task", "quota-owner");
        task.runtime = Some("hcode".into());
        task.task_source = Some("return 1;".into());
        task.torrent_source = Some(input(b"tensor bytes"));
        repo.create(&task).await.unwrap();
        repo.put_general_compute_artifact_chunk(
            &task.task_id,
            "model",
            0,
            12,
            &sha256_digest(b"tensor bytes"),
            b"tensor bytes",
        )
        .await
        .unwrap();
        sqlx::query("UPDATE general_compute_artifacts SET size_bytes = $1 WHERE task_id = $2")
            .bind(general_compute_runtime::hcode_artifacts::OWNER_STORAGE_QUOTA_BYTES)
            .bind(&task.task_id)
            .execute(&repo.pool)
            .await
            .unwrap();
        task.task_id = "quota-task-next".into();
        assert!(repo
            .create(&task)
            .await
            .unwrap_err()
            .to_string()
            .contains("quota"));
        assert!(repo.find_by_task_id(&task.task_id).await.unwrap().is_none());
        sqlx::query(
            "UPDATE general_compute_artifacts SET expires_at = NOW() - INTERVAL '1 second'",
        )
        .execute(&repo.pool)
        .await
        .unwrap();
        repo.create(&task).await.unwrap();
        assert!(repo.find_pending().await.unwrap().is_empty());
        let expired = repo.find_by_task_id("quota-task").await.unwrap().unwrap();
        assert_eq!(expired.status, TaskStatus::Failed);
        assert!(expired.billing_settled);
        assert_eq!(expired.billed_amount, 0);
        let stored_chunks: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM general_compute_artifact_chunks WHERE task_id = 'quota-task'",
        )
        .fetch_one(&repo.pool)
        .await
        .unwrap();
        assert_eq!(stored_chunks, 0);
        fixture.cleanup().await.unwrap();
    }
}
