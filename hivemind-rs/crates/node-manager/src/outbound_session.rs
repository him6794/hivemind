use crate::grpc::NodepoolState;
use anyhow::Context;
use hivemind_client_core::{
    CancellationAckDisposition, SessionError, SessionHello, SessionIdentity, SessionResult,
};
use hivemind_proto::{
    worker_session_client_frame, worker_session_server_frame, ExecuteTaskRequest,
    StopTaskExecutionRequest, WorkerSessionClientFrame, WorkerSessionHello,
    WorkerSessionServerFrame, WorkerSessionTask, WorkerSessionWelcome,
};
use prost::Message;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tonic::{Request, Response, Status, Streaming};

const SESSION_OUTBOUND_QUEUE_CAPACITY: usize = 128;
const SESSION_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

pub struct GrpcWorkerSessionService {
    state: Arc<NodepoolState>,
}

impl GrpcWorkerSessionService {
    #[must_use]
    pub fn new(state: Arc<NodepoolState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl hivemind_proto::WorkerSessionService for GrpcWorkerSessionService {
    type OpenSessionStream =
        Pin<Box<dyn Stream<Item = Result<WorkerSessionServerFrame, Status>> + Send>>;

    async fn open_session(
        &self,
        request: Request<Streaming<WorkerSessionClientFrame>>,
    ) -> Result<Response<Self::OpenSessionStream>, Status> {
        let mut inbound = request.into_inner();
        let hello_frame = inbound
            .message()
            .await
            .map_err(|error| {
                Status::unauthenticated(format!("Worker session hello failed: {error}"))
            })?
            .ok_or_else(|| Status::unauthenticated("Worker session hello is required"))?;
        hivemind_proto::validate_worker_session_client_frame(&hello_frame)
            .map_err(Status::invalid_argument)?;
        let Some(worker_session_client_frame::Frame::Hello(hello)) = hello_frame.frame else {
            return Err(Status::invalid_argument(
                "Worker session must start with hello",
            ));
        };

        let (identity, parsed_report) = self.authenticate_hello(&hello).await?;
        let now = session_now();
        let outcome = {
            let mut registry = self
                .state
                .session_registry
                .lock()
                .map_err(|_| Status::internal("Worker session registry is unavailable"))?;
            registry
                .connect(
                    SessionHello {
                        protocol_version: hello.protocol_version,
                        token: hello.token,
                        worker_id: identity.worker_id.clone(),
                        owner: identity.owner.clone(),
                        client_instance_id: identity.client_instance_id.clone(),
                        capability_report_json: parsed_report
                            .as_ref()
                            .map(|report| report.capabilities_json())
                            .transpose()
                            .map_err(|error| Status::invalid_argument(error.to_string()))?
                            .unwrap_or_default(),
                        ready: parsed_report.as_ref().is_some_and(|report| report.ready),
                        readiness_reason: parsed_report
                            .as_ref()
                            .map(|report| report.readiness_reason.clone())
                            .unwrap_or_default(),
                        resume_token: (!hello.resume_token.is_empty())
                            .then_some(hello.resume_token),
                        last_received_sequence: hello.last_received_sequence,
                    },
                    now,
                )
                .map_err(session_error_status)?
        };

        if let Some(report) = parsed_report {
            let observation = match crate::service::dynamic_capability_observation(&report)
                .map_err(|error| Status::invalid_argument(error.to_string()))
            {
                Ok(observation) => observation,
                Err(error) => {
                    disconnect_session(&self.state.session_registry, &outcome.welcome.session_id);
                    return Err(error);
                }
            };
            if let Err(error) = self
                .state
                .node_manager
                .update_dynamic_capabilities(
                    &identity.worker_id,
                    &observation.0,
                    &observation.1,
                    observation.2,
                    observation.3.as_deref(),
                )
                .await
                .map_err(|error| Status::failed_precondition(error.to_string()))
            {
                disconnect_session(&self.state.session_registry, &outcome.welcome.session_id);
                return Err(error);
            }
        }

        let (sender, receiver) = mpsc::channel(SESSION_OUTBOUND_QUEUE_CAPACITY);
        let welcome = WorkerSessionServerFrame {
            frame: Some(worker_session_server_frame::Frame::Welcome(
                WorkerSessionWelcome {
                    success: true,
                    status_message: "OK".into(),
                    protocol_version: outcome.welcome.protocol_version,
                    session_id: outcome.welcome.session_id.clone(),
                    worker_id: outcome.welcome.worker_id.clone(),
                    owner: outcome.welcome.owner.clone(),
                    client_instance_id: outcome.welcome.client_instance_id.clone(),
                    resume_token: outcome.welcome.resume_token,
                    next_delivery_sequence: outcome.welcome.next_delivery_sequence,
                    resume_after_sequence: outcome.welcome.resume_after_sequence,
                },
            )),
        };
        if let Err(error) = send_server_frame(&sender, welcome).await {
            disconnect_session(&self.state.session_registry, &outcome.welcome.session_id);
            return Err(error);
        }

        let session_id = outcome.welcome.session_id;
        let registry = self.state.session_registry.clone();
        let identity_for_loop = identity.clone();
        let state = self.state.clone();
        tokio::spawn(async move {
            let mut sent_sequences: HashSet<u64> = HashSet::new();
            let mut sent_cancellation_sequences: HashSet<u64> = HashSet::new();
            for delivery in outcome.redeliveries {
                let result = if delivery.cancellation_requested {
                    send_cancel_delivery(&sender, &mut sent_cancellation_sequences, delivery).await
                } else {
                    send_task_delivery(&sender, &mut sent_sequences, delivery).await
                };
                if let Err(error) = result {
                    tracing::debug!(error = %error, "Worker session closed during redelivery");
                    disconnect_session(&registry, &session_id);
                    return;
                }
            }

            let mut poll = tokio::time::interval(SESSION_POLL_INTERVAL);
            loop {
                tokio::select! {
                    message = inbound.message() => {
                        match message {
                            Ok(Some(frame)) => {
                                match handle_client_frame(
                                    &state,
                                    &registry,
                                    &session_id,
                                    &identity_for_loop,
                                    frame,
                                    &sender,
                                )
                                .await
                                {
                                    Ok(true) => {}
                                    Ok(false) => break,
                                    Err(error) => {
                                        let terminal = matches!(
                                            error.code(),
                                            tonic::Code::Unauthenticated
                                                | tonic::Code::PermissionDenied
                                        );
                                        let _ = send_error_frame(
                                            &sender,
                                            if terminal {
                                                error.to_string()
                                            } else {
                                                "Worker session frame was rejected".into()
                                            },
                                            terminal,
                                        )
                                        .await;
                                        disconnect_session(&registry, &session_id);
                                        break;
                                    }
                                }
                            }
                            Ok(None) | Err(_) => {
                                disconnect_session(&registry, &session_id);
                                break;
                            }
                        }
                    }
                    _ = poll.tick() => {
                        let session_active = registry
                            .lock()
                            .ok()
                            .is_some_and(|registry| registry.is_session_active(&session_id));
                        if !session_active {
                            let _ = send_server_frame(
                                &sender,
                                WorkerSessionServerFrame {
                                    frame: Some(worker_session_server_frame::Frame::Close(
                                        hivemind_proto::WorkerSessionClose {
                                            reason: "Worker session was replaced or expired".into(),
                                        },
                                    )),
                                },
                            )
                            .await;
                            break;
                        }
                        prune_sent_sequences(&registry, &session_id, &mut sent_sequences);
                        prune_sent_sequences(
                            &registry,
                            &session_id,
                            &mut sent_cancellation_sequences,
                        );
                        let cancellations = registry
                            .lock()
                            .ok()
                            .map(|registry| registry.cancellation_deliveries(&session_id))
                            .unwrap_or_default();
                        for delivery in cancellations {
                            if sent_cancellation_sequences.contains(&delivery.delivery_sequence) {
                                continue;
                            }
                            if let Err(error) = send_cancel_delivery(
                                &sender,
                                &mut sent_cancellation_sequences,
                                delivery,
                            )
                            .await
                            {
                                tracing::debug!(error = %error, "Worker session closed while delivering cancellation");
                                disconnect_session(&registry, &session_id);
                                return;
                            }
                        }
                        let deliveries = registry
                            .lock()
                            .ok()
                            .map(|registry| registry.pending_deliveries(&identity_for_loop))
                            .unwrap_or_default();
                        for delivery in deliveries {
                            if sent_sequences.contains(&delivery.delivery_sequence) {
                                continue;
                            }
                            if let Err(error) = send_task_delivery(&sender, &mut sent_sequences, delivery).await {
                                tracing::debug!(error = %error, "Worker session closed while delivering task");
                                disconnect_session(&registry, &session_id);
                                return;
                            }
                        }
                        if let Ok(mut registry) = registry.lock() {
                            registry.expire(session_now());
                        }
                    }
                }
            }
        });

        Ok(Response::new(
            Box::pin(ReceiverStream::new(receiver)) as Self::OpenSessionStream
        ))
    }
}

impl GrpcWorkerSessionService {
    async fn authenticate_hello(
        &self,
        hello: &WorkerSessionHello,
    ) -> Result<
        (
            SessionIdentity,
            Option<hivemind_models::WorkerCapabilityReport>,
        ),
        Status,
    > {
        let claims = self
            .state
            .auth
            .validate_token(&hello.token)
            .map_err(|_| Status::unauthenticated("Invalid Worker session token"))?;
        if claims.sub != hello.owner {
            return Err(Status::permission_denied(
                "Worker session owner does not match token",
            ));
        }
        let worker = self
            .state
            .node_manager
            .get_worker(&hello.worker_id)
            .await
            .map_err(|error| Status::internal(error.to_string()))?
            .ok_or_else(|| Status::not_found("Worker is not registered"))?;
        if worker.username != claims.sub {
            return Err(Status::permission_denied(
                "Worker session is not owned by this user",
            ));
        }
        if self.state.node_manager.is_public_dynamic_admission() {
            let bound_instance = self
                .state
                .node_manager
                .client_instance_id_for_worker(&hello.worker_id, &claims.sub)
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
            if let Some(bound_instance) = bound_instance {
                if bound_instance != hello.client_instance_id {
                    return Err(Status::permission_denied(
                        "Worker session client identity does not match enrollment",
                    ));
                }
            }
            // No enrolled client identity: this Worker registered directly as
            // its owner (private/local mode without a Website API). The owner
            // binding above already authenticated it; bind the reported
            // instance id for the rest of this session's reconnect checks.
        }
        let report = if self.state.node_manager.is_public_dynamic_admission() {
            Some(
                crate::grpc::parse_worker_capability_report(hello.capability_report.clone(), true)
                    .map_err(Status::invalid_argument)?
                    .context("capability report missing")
                    .map_err(|error| Status::invalid_argument(error.to_string()))?,
            )
        } else {
            None
        };
        Ok((
            SessionIdentity {
                worker_id: worker.worker_id,
                owner: claims.sub,
                client_instance_id: hello.client_instance_id.clone(),
            },
            report,
        ))
    }
}

async fn handle_client_frame(
    state: &Arc<NodepoolState>,
    registry: &hivemind_client_core::SharedSessionRegistry,
    session_id: &str,
    identity: &SessionIdentity,
    frame: WorkerSessionClientFrame,
    sender: &mpsc::Sender<Result<WorkerSessionServerFrame, Status>>,
) -> Result<bool, Status> {
    hivemind_proto::validate_worker_session_client_frame(&frame)
        .map_err(Status::invalid_argument)?;
    match frame.frame {
        Some(worker_session_client_frame::Frame::Ack(ack)) => {
            let delivery = registry
                .lock()
                .map_err(|_| Status::internal("Worker session registry is unavailable"))?
                .delivery(session_id, ack.delivery_sequence)
                .ok_or_else(|| {
                    Status::failed_precondition("Worker session delivery is unavailable")
                })?;
            if delivery.task.task_id != ack.task_id
                || delivery.task.attempt_id != ack.attempt_id
                || delivery.task.idempotency_key != ack.idempotency_key
            {
                return Err(Status::permission_denied(
                    "Worker session acknowledgement does not match its delivery",
                ));
            }
            let dispatcher = state.dispatcher.as_ref().ok_or_else(|| {
                Status::failed_precondition(
                    "Worker session acknowledgement requires the authoritative dispatcher",
                )
            })?;
            if let Err(error) = dispatcher
                .handle_worker_session_ack(
                    identity.worker_id.as_str(),
                    identity.owner.as_str(),
                    &ack.task_id,
                    &ack.attempt_id,
                    &ack.idempotency_key,
                    &delivery.task.request_digest,
                )
                .await
            {
                tracing::warn!(
                    worker_id = %identity.worker_id,
                    task_id = %ack.task_id,
                    error = %error,
                    "Rejected Worker session acknowledgement"
                );
                send_error_frame(
                    sender,
                    "Worker session acknowledgement was rejected; reconnecting for redelivery"
                        .into(),
                    false,
                )
                .await?;
                disconnect_session(registry, session_id);
                return Ok(false);
            }
            registry
                .lock()
                .map_err(|_| Status::internal("Worker session registry is unavailable"))?
                .acknowledge(
                    session_id,
                    ack.delivery_sequence,
                    &ack.task_id,
                    &ack.attempt_id,
                    &ack.idempotency_key,
                )
                .map_err(session_error_status)?;
        }
        Some(worker_session_client_frame::Frame::CancelAck(ack)) => {
            let delivery = registry
                .lock()
                .map_err(|_| Status::internal("Worker session registry is unavailable"))?
                .delivery(session_id, ack.delivery_sequence)
                .ok_or_else(|| {
                    Status::failed_precondition("Worker session cancellation is unavailable")
                })?;
            if delivery.task.task_id != ack.task_id
                || delivery.task.attempt_id != ack.attempt_id
                || delivery.task.idempotency_key != ack.idempotency_key
            {
                return Err(Status::permission_denied(
                    "Worker session cancellation acknowledgement does not match its delivery",
                ));
            }
            let disposition = registry
                .lock()
                .map_err(|_| Status::internal("Worker session registry is unavailable"))?
                .acknowledge_cancellation(
                    session_id,
                    ack.delivery_sequence,
                    &ack.task_id,
                    &ack.attempt_id,
                    &ack.idempotency_key,
                )
                .map_err(session_error_status)?;
            if matches!(disposition, CancellationAckDisposition::Duplicate) {
                tracing::debug!(
                    worker_id = %identity.worker_id,
                    task_id = %ack.task_id,
                    "Ignoring duplicate Worker session cancellation acknowledgement"
                );
            }
        }
        Some(worker_session_client_frame::Frame::Result(result)) => {
            let response = result.response.ok_or_else(|| {
                Status::invalid_argument("Worker session result is missing response")
            })?;
            let delivery = registry
                .lock()
                .map_err(|_| Status::internal("Worker session registry is unavailable"))?
                .delivery(session_id, result.delivery_sequence)
                .ok_or_else(|| {
                    Status::failed_precondition("Worker session delivery is unavailable")
                })?;
            if delivery.task.task_id != result.task_id {
                return Err(Status::permission_denied(
                    "Worker session result does not match its delivery",
                ));
            }
            if delivery.cancellation_requested {
                return Err(Status::failed_precondition(
                    "Worker session result arrived after cancellation",
                ));
            }
            let dispatcher = state.dispatcher.as_ref().ok_or_else(|| {
                Status::failed_precondition(
                    "Worker session result requires the authoritative dispatcher",
                )
            })?;
            let request =
                ExecuteTaskRequest::decode(delivery.task.payload.as_slice()).map_err(|_| {
                    Status::failed_precondition("Worker session task payload is invalid")
                })?;
            if let Err(error) = dispatcher
                .handle_worker_session_result(
                    identity.worker_id.as_str(),
                    &result.task_id,
                    request,
                    response.clone(),
                )
                .await
            {
                tracing::warn!(
                    worker_id = %identity.worker_id,
                    task_id = %result.task_id,
                    error = %error,
                    "Rejected Worker session result before delivery completion"
                );
                send_error_frame(
                    sender,
                    "Worker session result was rejected; reconnecting for redelivery".into(),
                    false,
                )
                .await?;
                disconnect_session(registry, session_id);
                return Ok(false);
            }
            let response_digest = response_digest(&response);
            let disposition = registry
                .lock()
                .map_err(|_| Status::internal("Worker session registry is unavailable"))?
                .record_result(
                    session_id,
                    SessionResult {
                        delivery_sequence: result.delivery_sequence,
                        task_id: result.task_id.clone(),
                        success: response.success,
                        response_digest,
                    },
                )
                .map_err(session_error_status)?;
            if matches!(
                disposition,
                hivemind_client_core::ResultDisposition::Duplicate
            ) {
                tracing::debug!(
                    worker_id = %identity.worker_id,
                    task_id = %result.task_id,
                    "Ignoring duplicate Worker session result"
                );
            }
        }
        Some(worker_session_client_frame::Frame::Heartbeat(heartbeat)) => {
            let active_task_ids = {
                let mut registry = registry
                    .lock()
                    .map_err(|_| Status::internal("Worker session registry is unavailable"))?;
                registry
                    .heartbeat(session_id, heartbeat.last_received_sequence, session_now())
                    .map_err(session_error_status)?;
                registry
                    .acknowledged_deliveries(session_id)
                    .into_iter()
                    .map(|delivery| delivery.task)
                    .collect::<Vec<_>>()
            };
            if let Err(error) = state
                .node_manager
                .refresh_session_tasks(&identity.worker_id, &active_task_ids)
                .await
            {
                tracing::warn!(
                    worker_id = %identity.worker_id,
                    error = %error,
                    "Worker session heartbeat could not refresh task liveness"
                );
            }
            send_server_frame(
                sender,
                WorkerSessionServerFrame {
                    frame: Some(worker_session_server_frame::Frame::Heartbeat(
                        hivemind_proto::WorkerSessionServerHeartbeat {
                            last_received_sequence: heartbeat.last_received_sequence,
                        },
                    )),
                },
            )
            .await?;
        }
        Some(worker_session_client_frame::Frame::Close(_)) => {
            disconnect_session(registry, session_id);
            return Ok(false);
        }
        Some(worker_session_client_frame::Frame::Hello(_)) | None => {
            return Err(Status::invalid_argument(
                "Worker session hello is only valid as the first frame",
            ));
        }
    }
    Ok(true)
}

async fn send_task_delivery(
    sender: &mpsc::Sender<Result<WorkerSessionServerFrame, Status>>,
    sent_sequences: &mut HashSet<u64>,
    delivery: hivemind_client_core::SessionDelivery,
) -> Result<(), Status> {
    let request = ExecuteTaskRequest::decode(delivery.task.payload.as_slice())
        .map_err(|_| Status::failed_precondition("Worker session task payload is invalid"))?;
    let frame = WorkerSessionServerFrame {
        frame: Some(worker_session_server_frame::Frame::Task(
            WorkerSessionTask {
                delivery_sequence: delivery.delivery_sequence,
                request: Some(request),
            },
        )),
    };
    send_server_frame(sender, frame).await?;
    sent_sequences.insert(delivery.delivery_sequence);
    Ok(())
}

async fn send_cancel_delivery(
    sender: &mpsc::Sender<Result<WorkerSessionServerFrame, Status>>,
    sent_sequences: &mut HashSet<u64>,
    delivery: hivemind_client_core::SessionDelivery,
) -> Result<(), Status> {
    let request = ExecuteTaskRequest::decode(delivery.task.payload.as_slice()).map_err(|_| {
        Status::failed_precondition("Worker session cancellation payload is invalid")
    })?;
    let frame = WorkerSessionServerFrame {
        frame: Some(worker_session_server_frame::Frame::Cancel(
            hivemind_proto::WorkerSessionCancel {
                delivery_sequence: delivery.delivery_sequence,
                request: Some(StopTaskExecutionRequest {
                    task_id: request.task_id,
                    token: request.token,
                    attempt_id: delivery.task.attempt_id,
                    idempotency_key: delivery.task.idempotency_key,
                }),
            },
        )),
    };
    send_server_frame(sender, frame).await?;
    sent_sequences.insert(delivery.delivery_sequence);
    Ok(())
}

async fn send_server_frame(
    sender: &mpsc::Sender<Result<WorkerSessionServerFrame, Status>>,
    frame: WorkerSessionServerFrame,
) -> Result<(), Status> {
    hivemind_proto::validate_worker_session_server_frame(&frame)
        .map_err(Status::failed_precondition)?;
    sender
        .send(Ok(frame))
        .await
        .map_err(|_| Status::unavailable("Worker session stream is closed"))
}

async fn send_error_frame(
    sender: &mpsc::Sender<Result<WorkerSessionServerFrame, Status>>,
    status_message: String,
    terminal: bool,
) -> Result<(), Status> {
    send_server_frame(
        sender,
        WorkerSessionServerFrame {
            frame: Some(worker_session_server_frame::Frame::Error(
                hivemind_proto::WorkerSessionError {
                    status_message: status_message
                        .chars()
                        .take(hivemind_proto::WORKER_STATUS_MESSAGE_MAX_BYTES)
                        .collect(),
                    terminal,
                },
            )),
        },
    )
    .await
}

fn prune_sent_sequences(
    registry: &hivemind_client_core::SharedSessionRegistry,
    session_id: &str,
    sent_sequences: &mut HashSet<u64>,
) {
    let Ok(registry) = registry.lock() else {
        sent_sequences.clear();
        return;
    };
    sent_sequences.retain(|sequence| registry.delivery(session_id, *sequence).is_some());
}

fn disconnect_session(registry: &hivemind_client_core::SharedSessionRegistry, session_id: &str) {
    if let Ok(mut registry) = registry.lock() {
        let _ = registry.disconnect(session_id, session_now());
    }
}

fn response_digest(response: &hivemind_proto::ExecuteTaskResponse) -> String {
    format!("sha256:{:x}", Sha256::digest(response.encode_to_vec()))
}

fn session_now() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

fn session_error_status(error: SessionError) -> Status {
    match error {
        SessionError::UnsupportedProtocol => Status::unimplemented(error.to_string()),
        SessionError::InvalidIdentity
        | SessionError::FieldTooLarge
        | SessionError::MissingToken => Status::invalid_argument(error.to_string()),
        SessionError::InvalidResumeToken | SessionError::DeliveryIdentityMismatch => {
            Status::permission_denied(error.to_string())
        }
        SessionError::DeliveryCancelled => Status::failed_precondition(error.to_string()),
        SessionError::CancellationNotRequested => Status::failed_precondition(error.to_string()),
        SessionError::QueueFull => Status::resource_exhausted(error.to_string()),
        SessionError::InactiveSession | SessionError::Expired => {
            Status::failed_precondition(error.to_string())
        }
        SessionError::SessionAlreadyActive
        | SessionError::UnknownDelivery
        | SessionError::ConflictingResult => Status::failed_precondition(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{response_digest, send_cancel_delivery};
    use hivemind_client_core::{SessionDelivery, SessionTask};
    use hivemind_proto::{worker_session_server_frame, ExecuteTaskRequest, ExecuteTaskResponse};
    use prost::Message;
    use std::collections::HashSet;
    use tokio::sync::mpsc;

    #[test]
    fn response_digest_is_bounded_and_prefixed() {
        let digest = response_digest(&ExecuteTaskResponse::default());
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest.len(), "sha256:".len() + 64);
    }

    #[tokio::test]
    async fn cancellation_delivery_preserves_the_attempt_bound_worker_token() {
        let request = ExecuteTaskRequest {
            task_id: "task-1".into(),
            token: "attempt-token".into(),
            ..ExecuteTaskRequest::default()
        };
        let delivery = SessionDelivery {
            delivery_sequence: 7,
            task: SessionTask {
                task_id: "task-1".into(),
                execution_id: "execution-1".into(),
                attempt_id: "attempt-1".into(),
                idempotency_key: "idempotency-1".into(),
                request_digest: "digest-1".into(),
                retry_count: 0,
                payload: request.encode_to_vec(),
            },
            cancellation_requested: true,
        };
        let (sender, mut receiver) = mpsc::channel(1);
        let mut sent = HashSet::new();

        send_cancel_delivery(&sender, &mut sent, delivery)
            .await
            .expect("cancellation frame is valid");
        let frame = receiver
            .recv()
            .await
            .expect("cancellation frame is delivered")
            .expect("cancellation frame has no stream error");
        let Some(worker_session_server_frame::Frame::Cancel(cancel)) = frame.frame else {
            panic!("expected a cancellation frame");
        };
        assert_eq!(cancel.delivery_sequence, 7);
        let request = cancel.request.expect("cancellation request is present");
        assert_eq!(request.task_id, "task-1");
        assert_eq!(request.token, "attempt-token");
        assert_eq!(request.attempt_id, "attempt-1");
        assert_eq!(request.idempotency_key, "idempotency-1");
        assert!(sent.contains(&7));
    }
}
