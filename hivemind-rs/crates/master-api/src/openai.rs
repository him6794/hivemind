//! OpenAI-compatible HTTP API (`/v1/models`, `/v1/chat/completions`, `/v1/completions`),
//! multi-device management (`/api/devices`), and public network status (`/api/network/status`).

use crate::handlers::AppState;
use crate::middleware::RawToken;
use axum::{
    extract::{Extension, Path, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use hivemind_proto::{
    LlmChatMessage, LlmInferChunk, LlmInferRequest, LlmRoutingOptions,
    UpdateDeviceMetadataRequest,
};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

// =============================================================================
// OpenAI Request / Response Types & Error Mapping
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct OpenAiChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<OpenAiChatMessage>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stop: Option<StopParam>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Deserialize)]
pub struct LegacyCompletionsRequest {
    pub model: String,
    pub prompt: String,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stop: Option<StopParam>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum StopParam {
    Single(String),
    Multiple(Vec<String>),
}

impl StopParam {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::Single(s) => vec![s],
            Self::Multiple(v) => v,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OpenAiErrorBody {
    pub error: OpenAiErrorDetail,
}

#[derive(Debug, Serialize)]
pub struct OpenAiErrorDetail {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    pub code: String,
}

pub fn openai_error_response(
    status: StatusCode,
    error_type: &str,
    code: &str,
    message: impl Into<String>,
) -> Response {
    (
        status,
        Json(OpenAiErrorBody {
            error: OpenAiErrorDetail {
                message: message.into(),
                error_type: error_type.to_owned(),
                code: code.to_owned(),
            },
        }),
    )
        .into_response()
}

pub fn map_grpc_status_to_openai_error(status: &tonic::Status) -> Response {
    let (http_status, error_type, code) = match status.code() {
        tonic::Code::InvalidArgument => (
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "invalid_request",
        ),
        tonic::Code::Unauthenticated => (
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "invalid_api_key",
        ),
        tonic::Code::PermissionDenied => (
            StatusCode::FORBIDDEN,
            "permission_error",
            "permission_denied",
        ),
        tonic::Code::NotFound => (StatusCode::NOT_FOUND, "not_found_error", "model_not_found"),
        tonic::Code::ResourceExhausted => (
            StatusCode::TOO_MANY_REQUESTS,
            "insufficient_quota",
            "insufficient_quota",
        ),
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => (
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "upstream_unavailable",
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "internal_error",
        ),
    };
    openai_error_response(http_status, error_type, code, status.message())
}

// =============================================================================
// X-Hivemind-* Routing & Privacy Disclosure Headers
// =============================================================================

pub fn parse_routing_headers(headers: &HeaderMap) -> LlmRoutingOptions {
    let get = |name: &str| -> String {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .unwrap_or_default()
            .to_owned()
    };

    let target_worker_ids = get("x-hivemind-target-workers")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();

    let own_devices_only = matches!(
        get("x-hivemind-own-devices-only")
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes"
    );

    LlmRoutingOptions {
        route_mode: get("x-hivemind-route-mode"),
        target_worker_id: get("x-hivemind-target-worker"),
        target_worker_ids,
        device_tag: get("x-hivemind-device-tag"),
        hardware_type: get("x-hivemind-hardware"),
        own_devices_only,
        trust_tier: get("x-hivemind-trust-tier"),
        overflow_policy: get("x-hivemind-overflow-policy"),
    }
}

fn attach_disclosure_headers(
    headers: &mut HeaderMap,
    data_privacy: &str,
    trust_tier: &str,
    execution_mode: &str,
    worker_id: &str,
    lan_direct: bool,
) {
    let insert = |map: &mut HeaderMap, name: &'static str, value: &str| {
        if !value.is_empty() {
            if let Ok(val) = HeaderValue::from_str(value) {
                map.insert(HeaderName::from_static(name), val);
            }
        }
    };
    insert(
        headers,
        "x-hivemind-data-privacy",
        if data_privacy.is_empty() {
            "public-unencrypted-to-worker"
        } else {
            data_privacy
        },
    );
    insert(headers, "x-hivemind-trust-tier", trust_tier);
    insert(headers, "x-hivemind-execution-mode", execution_mode);
    insert(headers, "x-hivemind-worker-id", worker_id);
    insert(
        headers,
        "x-hivemind-lan-direct",
        if lan_direct { "true" } else { "false" },
    );
}

fn unix_now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn format_chat_sse_chunk(chunk: &LlmInferChunk, created: i64) -> serde_json::Value {
    let finish_reason = if chunk.done {
        Some(if chunk.finish_reason.is_empty() {
            "stop".to_owned()
        } else {
            chunk.finish_reason.clone()
        })
    } else {
        None
    };

    let delta = if chunk.delta_content.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::json!({ "content": chunk.delta_content })
    };

    let mut payload = serde_json::json!({
        "id": format!("chatcmpl-{}", chunk.request_id),
        "object": "chat.completion.chunk",
        "created": created,
        "model": chunk.model_id,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish_reason,
        }],
        "hivemind_worker_id": chunk.worker_id,
        "hivemind_trust_tier": chunk.trust_tier,
        "hivemind_data_privacy": chunk.data_privacy,
    });

    if chunk.done && (chunk.prompt_tokens > 0 || chunk.completion_tokens > 0) {
        payload["usage"] = serde_json::json!({
            "prompt_tokens": chunk.prompt_tokens,
            "completion_tokens": chunk.completion_tokens,
            "total_tokens": chunk.prompt_tokens + chunk.completion_tokens,
        });
    }
    payload
}

pub fn format_legacy_sse_chunk(chunk: &LlmInferChunk, created: i64) -> serde_json::Value {
    let finish_reason = if chunk.done {
        Some(if chunk.finish_reason.is_empty() {
            "stop".to_owned()
        } else {
            chunk.finish_reason.clone()
        })
    } else {
        None
    };
    serde_json::json!({
        "id": format!("cmpl-{}", chunk.request_id),
        "object": "text_completion",
        "created": created,
        "model": chunk.model_id,
        "choices": [{
            "index": 0,
            "text": chunk.delta_content,
            "finish_reason": finish_reason,
        }],
        "hivemind_worker_id": chunk.worker_id,
    })
}

// =============================================================================
// HTTP Handlers
// =============================================================================

pub async fn list_models(
    State(state): State<AppState>,
    Extension(RawToken(token)): Extension<RawToken>,
    headers: HeaderMap,
) -> Response {
    let routing = parse_routing_headers(&headers);
    let mut grpc = state.grpc_client.clone();
    match grpc
        .list_online_models(&token, routing.own_devices_only)
        .await
    {
        Ok(resp) => {
            let now = unix_now_secs();
            let data = resp
                .models
                .into_iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.model_id,
                        "object": "model",
                        "created": now,
                        "owned_by": "hivemind",
                        "display_name": m.display_name,
                        "quantization": m.quantization,
                        "context_length": m.context_length,
                        "input_cpt_per_million": m.input_cpt_per_million,
                        "output_cpt_per_million": m.output_cpt_per_million,
                        "online_workers": m.online_single_workers,
                        "distributed_layer_replicas": m.distributed_layer_replicas,
                        "distributed_complete": m.distributed_complete,
                        "available_hardware": m.available_hardware,
                        "available_formats": m.available_formats,
                    })
                })
                .collect::<Vec<_>>();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "object": "list",
                    "data": data,
                })),
            )
                .into_response()
        }
        Err(status) => map_grpc_status_to_openai_error(&status),
    }
}

pub async fn chat_completions(
    State(state): State<AppState>,
    Extension(RawToken(token)): Extension<RawToken>,
    headers: HeaderMap,
    Json(body): Json<ChatCompletionsRequest>,
) -> Response {
    if body.model.trim().is_empty() {
        return openai_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "missing_model",
            "Field `model` must not be empty.",
        );
    }
    if body.messages.is_empty() {
        return openai_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "missing_messages",
            "Field `messages` must contain at least one message.",
        );
    }

    let request_id = Uuid::new_v4().to_string();
    let routing = parse_routing_headers(&headers);
    let rpc_req = LlmInferRequest {
        token,
        request_id: request_id.clone(),
        model_id: body.model.clone(),
        messages: body
            .messages
            .into_iter()
            .map(|m| LlmChatMessage {
                role: m.role,
                content: m.content,
            })
            .collect(),
        prompt: String::new(),
        max_tokens: body.max_tokens.unwrap_or(512),
        temperature: body.temperature.unwrap_or(0.7),
        top_p: body.top_p.unwrap_or(0.95),
        stop: body.stop.map(StopParam::into_vec).unwrap_or_default(),
        stream: body.stream,
        routing: Some(routing.clone()),
    };

    let mut grpc = state.grpc_client.clone();
    let created = unix_now_secs();

    if body.stream {
        match grpc.llm_infer_stream(rpc_req).await {
            Ok(stream) => {
                let privacy = if routing.own_devices_only
                    || routing.trust_tier.eq_ignore_ascii_case("tier0_private")
                {
                    "private-tier0"
                } else {
                    "public-unencrypted-to-worker"
                };
                let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);
                tokio::spawn(async move {
                    let mut inner = stream;
                    while let Some(item) = inner.next().await {
                        match item {
                            Ok(chunk) => {
                                if !chunk.error_message.is_empty() {
                                    let err_json = serde_json::json!({
                                        "error": {
                                            "message": chunk.error_message,
                                            "type": "stream_error",
                                            "code": "upstream_error",
                                        }
                                    });
                                    let _ = tx.send(Ok(Event::default().data(err_json.to_string()))).await;
                                    break;
                                }
                                let json = format_chat_sse_chunk(&chunk, created);
                                if tx.send(Ok(Event::default().data(json.to_string()))).await.is_err() {
                                    break;
                                }
                                if chunk.done {
                                    let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
                                    break;
                                }
                            }
                            Err(err) => {
                                let err_json = serde_json::json!({
                                    "error": {
                                        "message": err.message(),
                                        "type": "stream_error",
                                        "code": "grpc_stream_error",
                                    }
                                });
                                let _ = tx.send(Ok(Event::default().data(err_json.to_string()))).await;
                                break;
                            }
                        }
                    }
                });
                let sse_stream = ReceiverStream::new(rx);
                let mut resp = Sse::new(sse_stream)
                    .keep_alive(KeepAlive::default())
                    .into_response();
                resp.headers_mut().insert(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("no-cache"),
                );
                attach_disclosure_headers(
                    resp.headers_mut(),
                    privacy,
                    if routing.trust_tier.is_empty() {
                        "tier1_public"
                    } else {
                        &routing.trust_tier
                    },
                    "stream",
                    "",
                    false,
                );
                resp
            }
            Err(status) => map_grpc_status_to_openai_error(&status),
        }
    } else {
        match grpc.llm_infer(rpc_req).await {
            Ok(res) => {
                let mut resp = (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "id": format!("chatcmpl-{}", res.request_id),
                        "object": "chat.completion",
                        "created": created,
                        "model": res.model_id,
                        "choices": [{
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": res.content,
                            },
                            "finish_reason": if res.finish_reason.is_empty() { "stop" } else { &res.finish_reason },
                        }],
                        "usage": {
                            "prompt_tokens": res.prompt_tokens,
                            "completion_tokens": res.completion_tokens,
                            "total_tokens": res.prompt_tokens + res.completion_tokens,
                        },
                        "hivemind": {
                            "charged_cpt": res.charged_cpt,
                            "worker_id": res.worker_id,
                            "worker_chain": res.worker_chain,
                            "execution_mode": res.execution_mode,
                            "trust_tier": res.trust_tier,
                            "data_privacy": res.data_privacy,
                            "lan_direct": res.lan_direct,
                        }
                    })),
                )
                    .into_response();
                attach_disclosure_headers(
                    resp.headers_mut(),
                    &res.data_privacy,
                    &res.trust_tier,
                    &res.execution_mode,
                    &res.worker_id,
                    res.lan_direct,
                );
                resp
            }
            Err(status) => map_grpc_status_to_openai_error(&status),
        }
    }
}

pub async fn legacy_completions(
    State(state): State<AppState>,
    Extension(RawToken(token)): Extension<RawToken>,
    headers: HeaderMap,
    Json(body): Json<LegacyCompletionsRequest>,
) -> Response {
    if body.model.trim().is_empty() {
        return openai_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "missing_model",
            "Field `model` must not be empty.",
        );
    }

    let request_id = Uuid::new_v4().to_string();
    let routing = parse_routing_headers(&headers);
    let rpc_req = LlmInferRequest {
        token,
        request_id,
        model_id: body.model,
        messages: Vec::new(),
        prompt: body.prompt,
        max_tokens: body.max_tokens.unwrap_or(512),
        temperature: body.temperature.unwrap_or(0.7),
        top_p: body.top_p.unwrap_or(0.95),
        stop: body.stop.map(StopParam::into_vec).unwrap_or_default(),
        stream: body.stream,
        routing: Some(routing.clone()),
    };

    let mut grpc = state.grpc_client.clone();
    let created = unix_now_secs();

    if body.stream {
        match grpc.llm_infer_stream(rpc_req).await {
            Ok(stream) => {
                let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);
                tokio::spawn(async move {
                    let mut inner = stream;
                    while let Some(item) = inner.next().await {
                        match item {
                            Ok(chunk) => {
                                let json = format_legacy_sse_chunk(&chunk, created);
                                if tx.send(Ok(Event::default().data(json.to_string()))).await.is_err() {
                                    break;
                                }
                                if chunk.done {
                                    let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });
                let sse_stream = ReceiverStream::new(rx);
                Sse::new(sse_stream)
                    .keep_alive(KeepAlive::default())
                    .into_response()
            }
            Err(status) => map_grpc_status_to_openai_error(&status),
        }
    } else {
        match grpc.llm_infer(rpc_req).await {
            Ok(res) => {
                let mut resp = (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "id": format!("cmpl-{}", res.request_id),
                        "object": "text_completion",
                        "created": created,
                        "model": res.model_id,
                        "choices": [{
                            "index": 0,
                            "text": res.content,
                            "finish_reason": if res.finish_reason.is_empty() { "stop" } else { &res.finish_reason },
                        }],
                        "usage": {
                            "prompt_tokens": res.prompt_tokens,
                            "completion_tokens": res.completion_tokens,
                            "total_tokens": res.prompt_tokens + res.completion_tokens,
                        }
                    })),
                )
                    .into_response();
                attach_disclosure_headers(
                    resp.headers_mut(),
                    &res.data_privacy,
                    &res.trust_tier,
                    &res.execution_mode,
                    &res.worker_id,
                    res.lan_direct,
                );
                resp
            }
            Err(status) => map_grpc_status_to_openai_error(&status),
        }
    }
}

// =============================================================================
// Multi-Device Metadata & Public Network Status Endpoints
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct UpdateDeviceMetadataBody {
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub trust_tier: String,
    #[serde(default)]
    pub overflow_policy: String,
    #[serde(default)]
    pub lan_endpoint: String,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub remove_device: bool,
}

pub async fn list_account_devices(
    State(state): State<AppState>,
    Extension(RawToken(token)): Extension<RawToken>,
) -> Response {
    let mut grpc = state.grpc_client.clone();
    match grpc.list_account_devices(&token).await {
        Ok(resp) => {
            let devices = resp
                .devices
                .into_iter()
                .map(|d| {
                    serde_json::json!({
                        "worker_id": d.worker_id,
                        "owner": d.owner,
                        "alias": d.alias,
                        "tags": d.tags,
                        "role": d.role,
                        "status": d.status,
                        "trust_tier": d.trust_tier,
                        "overflow_policy": d.overflow_policy,
                        "lan_endpoint": d.lan_endpoint,
                        "paused": d.paused,
                        "hardware_type": d.hardware_type,
                        "loaded_models": d.loaded_models,
                    })
                })
                .collect::<Vec<_>>();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": resp.success,
                    "devices": devices,
                })),
            )
                .into_response()
        }
        Err(status) => map_grpc_status_to_openai_error(&status),
    }
}

pub async fn update_device_metadata(
    State(state): State<AppState>,
    Extension(RawToken(token)): Extension<RawToken>,
    Path(worker_id): Path<String>,
    Json(body): Json<UpdateDeviceMetadataBody>,
) -> Response {
    let mut grpc = state.grpc_client.clone();
    let req = UpdateDeviceMetadataRequest {
        token,
        worker_id,
        alias: body.alias,
        tags: body.tags,
        role: body.role,
        trust_tier: body.trust_tier,
        overflow_policy: body.overflow_policy,
        lan_endpoint: body.lan_endpoint,
        paused: body.paused,
        remove_device: body.remove_device,
    };
    match grpc.update_device_metadata(req).await {
        Ok(resp) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": resp.success,
                "status_message": resp.status_message,
            })),
        )
            .into_response(),
        Err(status) => map_grpc_status_to_openai_error(&status),
    }
}

pub async fn get_public_network_status(State(state): State<AppState>) -> Response {
    let mut grpc = state.grpc_client.clone();
    match grpc.get_public_network_status().await {
        Ok(resp) => {
            let models = resp
                .models
                .into_iter()
                .map(|m| {
                    serde_json::json!({
                        "model_id": m.model_id,
                        "display_name": m.display_name,
                        "online_workers": m.online_workers,
                        "min_layer_replicas": m.min_layer_replicas,
                        "layer_coverage_complete": m.layer_coverage_complete,
                        "avg_ttft_ms": m.avg_ttft_ms,
                    })
                })
                .collect::<Vec<_>>();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": resp.success,
                    "total_online_devices": resp.total_online_devices,
                    "private_pool_devices": resp.private_pool_devices,
                    "public_pool_devices": resp.public_pool_devices,
                    "avg_latency_ms": resp.avg_latency_ms,
                    "success_rate": resp.success_rate,
                    "models": models,
                    "updated_at": resp.updated_at,
                })),
            )
                .into_response()
        }
        Err(status) => map_grpc_status_to_openai_error(&status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc_client::GrpcClient;
    use crate::handlers::TaskSubmitRateLimiter;
    use axum::body::Body;
    use axum::http::Request;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[test]
    fn parse_routing_headers_extracts_all_hivemind_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-hivemind-route-mode", HeaderValue::from_static("manual"));
        headers.insert(
            "x-hivemind-target-worker",
            HeaderValue::from_static("w-gpu-0"),
        );
        headers.insert(
            "x-hivemind-target-workers",
            HeaderValue::from_static("w-1, w-2 , w-3"),
        );
        headers.insert("x-hivemind-device-tag", HeaderValue::from_static("studio"));
        headers.insert("x-hivemind-hardware", HeaderValue::from_static("gpu"));
        headers.insert(
            "x-hivemind-own-devices-only",
            HeaderValue::from_static("true"),
        );
        headers.insert(
            "x-hivemind-trust-tier",
            HeaderValue::from_static("tier0_private"),
        );
        headers.insert(
            "x-hivemind-overflow-policy",
            HeaderValue::from_static("strict_private"),
        );

        let parsed = parse_routing_headers(&headers);
        assert_eq!(parsed.route_mode, "manual");
        assert_eq!(parsed.target_worker_id, "w-gpu-0");
        assert_eq!(parsed.target_worker_ids, vec!["w-1", "w-2", "w-3"]);
        assert_eq!(parsed.device_tag, "studio");
        assert_eq!(parsed.hardware_type, "gpu");
        assert!(parsed.own_devices_only);
        assert_eq!(parsed.trust_tier, "tier0_private");
        assert_eq!(parsed.overflow_policy, "strict_private");
    }

    #[test]
    fn sse_chunk_formatter_emits_openai_delta_and_usage_on_done() {
        let chunk = LlmInferChunk {
            request_id: "req-42".to_owned(),
            model_id: "qwen2.5-7b".to_owned(),
            delta_content: "Hello".to_owned(),
            done: false,
            finish_reason: String::new(),
            prompt_tokens: 0,
            completion_tokens: 0,
            charged_cpt: 0,
            worker_id: "w-1".to_owned(),
            worker_chain: vec![],
            execution_mode: "single".to_owned(),
            trust_tier: "tier0_private".to_owned(),
            data_privacy: "private-tier0".to_owned(),
            lan_direct: true,
            error_message: String::new(),
        };
        let v = format_chat_sse_chunk(&chunk, 1_700_000_000);
        assert_eq!(v["object"], "chat.completion.chunk");
        assert_eq!(v["choices"][0]["delta"]["content"], "Hello");
        assert!(v["choices"][0]["finish_reason"].is_null());

        let done_chunk = LlmInferChunk {
            delta_content: String::new(),
            done: true,
            finish_reason: "stop".to_owned(),
            prompt_tokens: 12,
            completion_tokens: 34,
            ..chunk
        };
        let vd = format_chat_sse_chunk(&done_chunk, 1_700_000_000);
        assert_eq!(vd["choices"][0]["finish_reason"], "stop");
        assert_eq!(vd["usage"]["total_tokens"], 46);
    }

    #[tokio::test]
    async fn openai_chat_rejects_empty_model_with_openai_error_schema() {
        let state = AppState {
            grpc_client: GrpcClient::new("127.0.0.1:50051"),
            config: hivemind_config::HivemindConfig::for_test(),
            task_submit_limiter: Arc::new(tokio::sync::Mutex::new(TaskSubmitRateLimiter::new())),
        };
        let app = axum::Router::new()
            .route("/v1/chat/completions", axum::routing::post(chat_completions))
            .with_state(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .extension(RawToken("hmk_test_api_key_123".into()))
            .body(Body::from(
                r#"{"model":"","messages":[{"role":"user","content":"hi"}]}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 64)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert_eq!(json["error"]["code"], "missing_model");
    }

    #[tokio::test]
    async fn openai_routes_require_bearer_auth() {
        let config = hivemind_config::HivemindConfig::for_test();
        let app = crate::routes::create_router(AppState {
            grpc_client: GrpcClient::new("127.0.0.1:50051"),
            config,
            task_submit_limiter: Arc::new(tokio::sync::Mutex::new(TaskSubmitRateLimiter::new())),
        });
        for (method, uri) in [("GET", "/v1/models"), ("POST", "/v1/chat/completions")] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{method} {uri}");
        }
    }
}
