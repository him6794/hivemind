mod tensor_artifact_execution {
    use super::*;
    use crate::grpc_server::{TransferLeaseAuthority, TransferLeaseAuthorityError};
    use general_compute_runtime::{
        hcode_artifacts::{TensorInputEnvelope, CHUNK_BYTES},
        sha256_digest, ArtifactChunk,
    };

    struct Source {
        bytes: Vec<u8>,
        corrupt: bool,
        pending: bool,
    }

    #[tonic::async_trait]
    impl TransferLeaseAuthority for Source {
        async fn validate(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: i64,
            _: &str,
            _: &str,
        ) -> Result<(), TransferLeaseAuthorityError> {
            Ok(())
        }
        async fn download_hcode_chunk(
            &self,
            token: &str,
            task: &str,
            artifact: &str,
            offset: u64,
        ) -> Result<Vec<u8>, TransferLeaseAuthorityError> {
            assert_eq!(token, "attempt-token");
            assert_eq!(task, "sandbox-gate-test");
            assert_eq!(artifact, "model");
            if self.pending {
                return std::future::pending().await;
            }
            let start = offset as usize;
            let end = (start + CHUNK_BYTES).min(self.bytes.len());
            let mut bytes = self.bytes[start..end].to_vec();
            if self.corrupt {
                bytes[0] ^= 1;
            }
            Ok(bytes)
        }
    }

    fn fixture(corrupt: bool, pending: bool) -> (Task, crate::HcodeArtifactAccess) {
        let header = r#"{"weights":{"dtype":"F32","shape":[300000],"data_offsets":[0,1200000]}}"#;
        let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
        bytes.extend_from_slice(header.as_bytes());
        bytes.resize(bytes.len() + 1_200_000 - 16, 0);
        for value in [1.0_f32, 2.0, 3.0, 4.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let artifact = ArtifactManifest {
            artifact_id: "model".into(),
            role: ArtifactRole::Input,
            size_bytes: bytes.len() as u64,
            mime_type: "application/x-safetensors".into(),
            sha256: sha256_digest(&bytes),
            inline_bytes: None,
            chunks: bytes
                .chunks(CHUNK_BYTES)
                .enumerate()
                .map(|(i, part)| ArtifactChunk {
                    offset: (i * CHUNK_BYTES) as u64,
                    size_bytes: part.len() as u64,
                    sha256: sha256_digest(part),
                })
                .collect(),
        };
        let envelope = TensorInputEnvelope {
            protocol: 1,
            input: serde_json::json!([1.0, 2.0]),
            artifacts: vec![artifact],
        };
        let mut task = test_task_with_source(envelope.to_input_json().unwrap());
        task.runtime = Some("hcode".into());
        task.max_cpt = 1000;
        task.task_source = Some(r#"return tolist(matmul(load_tensor_slice("model", "weights", 299996, [2,2]), tensor(input)));"#.into());
        (
            task,
            crate::HcodeArtifactAccess {
                token: "attempt-token".into(),
                authority: Arc::new(Source {
                    bytes,
                    corrupt,
                    pending,
                }),
            },
        )
    }

    #[tokio::test]
    async fn worker_tensor_input_over_one_mib_runs_inference_and_preserves_receipt() {
        let (task, access) = fixture(false, false);
        let (_sender, receiver) = watch::channel(false);
        let result = run_hcode_artifact_task(task, receiver, access)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output.as_deref(), Some("[5.0,11.0]"));
        assert!(result.managed_executed_ops > 0);
        assert!(result
            .managed_receipt_json
            .unwrap()
            .contains(MANAGED_DSL_V1_SEMANTICS_MANIFEST_SHA256));
    }

    #[tokio::test]
    async fn worker_tensor_input_rejects_corruption_and_cancels_a_blocked_transfer() {
        let (task, access) = fixture(true, false);
        let (_sender, receiver) = watch::channel(false);
        assert!(run_hcode_artifact_task(task, receiver, access)
            .await
            .unwrap_err()
            .to_string()
            .contains("does not match"));
        let (task, access) = fixture(false, true);
        let (_sender, receiver) = watch::channel(true);
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            run_hcode_artifact_task(task, receiver, access),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.to_string().contains("stopped"));
    }

    struct RpcSource(Arc<dyn TransferLeaseAuthority>);

    #[tonic::async_trait]
    impl hivemind_proto::general_compute_artifact_service_server::GeneralComputeArtifactService
        for RpcSource
    {
        async fn upload_chunk(
            &self,
            _: tonic::Request<hivemind_proto::GeneralComputeArtifactChunkUpload>,
        ) -> Result<
            tonic::Response<hivemind_proto::GeneralComputeArtifactChunkUploadResponse>,
            tonic::Status,
        > {
            Err(tonic::Status::unimplemented("download-only test source"))
        }
        async fn download_hcode_chunk(
            &self,
            request: tonic::Request<hivemind_proto::HcodeArtifactChunkDownload>,
        ) -> Result<
            tonic::Response<hivemind_proto::HcodeArtifactChunkDownloadResponse>,
            tonic::Status,
        > {
            let request = request.into_inner();
            let bytes = self
                .0
                .download_hcode_chunk(
                    &request.token,
                    &request.task_id,
                    &request.artifact_id,
                    request.offset as u64,
                )
                .await
                .map_err(|error| tonic::Status::internal(format!("{error:?}")))?;
            Ok(tonic::Response::new(
                hivemind_proto::HcodeArtifactChunkDownloadResponse { bytes },
            ))
        }
        async fn authorize_hcode_chunk(
            &self,
            _: tonic::Request<hivemind_proto::HcodeArtifactChunkDownload>,
        ) -> Result<tonic::Response<hivemind_proto::HcodeArtifactChunkAuthorization>, tonic::Status>
        {
            Err(tonic::Status::unimplemented("download-only test source"))
        }
    }

    #[tokio::test]
    async fn worker_tensor_artifact_rpc_client_accepts_full_size_chunks() {
        use hivemind_proto::general_compute_artifact_service_server::GeneralComputeArtifactServiceServer;
        let (task, access) = fixture(false, false);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let authority = crate::grpc_server::NodepoolTransferLeaseAuthority::new(format!(
            "http://{}",
            listener.local_addr().unwrap()
        ));
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(GeneralComputeArtifactServiceServer::new(RpcSource(
                    access.authority,
                )))
                .serve_with_incoming_shutdown(
                    tokio_stream::wrappers::TcpListenerStream::new(listener),
                    async {
                        let _ = stopped.await;
                    },
                )
                .await
                .unwrap();
        });
        let (_sender, receiver) = watch::channel(false);
        let result = run_hcode_artifact_task(
            task,
            receiver,
            crate::HcodeArtifactAccess {
                token: "attempt-token".into(),
                authority,
            },
        )
        .await
        .unwrap();
        assert!(result.success);
        assert_eq!(result.output.as_deref(), Some("[5.0,11.0]"));
        stop.send(()).unwrap();
        server.await.unwrap();
    }

    #[test]
    fn tensor_envelope_cannot_execute_without_prepared_bytes() {
        let (task, _) = fixture(false, false);
        assert!(
            execute_managed_function_task(&task, Instant::now(), &AtomicBool::new(false))
                .unwrap_err()
                .to_string()
                .contains("not prepared")
        );
    }
}
