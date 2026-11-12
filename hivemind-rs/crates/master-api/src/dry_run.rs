//! Local dry run for hcode tasks.
//!
//! `POST /api/tasks/dry-run` runs the submitted source once on this machine with
//! the limits a replica uses, then prices the task from the usage that run
//! reports. It never contacts Nodepool and never touches a balance.
//!
//! The figures are an estimate. Nodepool settles on what the Workers report, so a
//! Worker built from a different interpreter can report different usage. The
//! response carries the semantics digest of the interpreter that ran, so a caller
//! can compare it with the one the network uses.

use std::sync::{Arc, OnceLock};

use axum::{http::StatusCode, Json};
use general_compute_runtime::managed_budget::required_task_budget;
use managed_function_runtime::dry_run::{dry_run, DryRunLimits, FailureCause};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::handlers::{validate_runtime_contract, CreateTaskBody};
use crate::middleware::AuthUser;

/// Replicas a task runs on when it names no policy. This is the network default;
/// a deployment that configures another value will quote differently.
pub const DEFAULT_REPLICA_COUNT: u16 = 3;

/// Dry runs that may execute at once. Each can use a CPU core for up to the
/// wall-time limit, so the count is kept small.
const MAX_CONCURRENT_RUNS: usize = 2;

/// Output returned in the response. A longer output is cut at a character boundary.
const OUTPUT_PREVIEW_BYTES: usize = 64 * 1024;

/// Request body limit. The source and the input travel as JSON strings, and escaping can turn one
/// byte into six, so the body may be several times larger than the sizes the contract allows. The
/// contract's own limits are enforced on the decoded text; this one only bounds what is read.
pub const MAX_REQUEST_BYTES: usize = (hivemind_proto::MANAGED_TASK_SOURCE_MAX_BYTES
    + hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES)
    * 6
    + 64 * 1024;

#[derive(Debug, Deserialize)]
pub struct DryRunBody {
    pub task_source: String,
    /// The JSON input, as the submit body carries it.
    #[serde(default)]
    pub torrent: Option<String>,
    #[serde(default)]
    pub managed_replica_count: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct DryRunQuote {
    /// Usage one replica reports for this input.
    pub replica_usage_units: i64,
    pub replica_count: u16,
    /// `"default"` when the request named no replica count.
    pub replica_count_source: &'static str,
    /// `replica_usage_units × replica_count`.
    pub reserved_usage_cpt: i64,
    pub platform_fee_cpt: i64,
    /// Smallest `max_cpt` that lets every replica finish. It is also the charge
    /// when every replica runs and is accepted.
    pub required_max_cpt: i64,
}

#[derive(Debug, Serialize)]
pub struct DryRunError {
    pub code: String,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// `program`, `usage_limit`, or `timed_out`. Only `program` means a Worker
    /// would fail the same way.
    pub cause: &'static str,
    pub usage_units: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct DryRunResponse {
    /// The run completed.
    pub success: bool,
    pub message: String,
    pub output: String,
    pub output_bytes: usize,
    pub output_truncated: bool,
    /// A result larger than the consensus limit is rejected when Workers report it.
    pub exceeds_consensus_result_limit: bool,
    pub consensus_result_limit_bytes: usize,
    pub usage_units: Option<u64>,
    pub quote: Option<DryRunQuote>,
    pub error: Option<DryRunError>,
    /// Digest of the interpreter that ran. Compare it with the network's.
    pub semantics_manifest_sha256: &'static str,
}

impl DryRunResponse {
    fn empty(success: bool, message: impl Into<String>) -> Self {
        Self {
            success,
            message: message.into(),
            output: String::new(),
            output_bytes: 0,
            output_truncated: false,
            exceeds_consensus_result_limit: false,
            consensus_result_limit_bytes: hivemind_proto::MANAGED_CONSENSUS_RESULT_MAX_BYTES,
            usage_units: None,
            quote: None,
            error: None,
            semantics_manifest_sha256: managed_function_runtime::V1_SEMANTICS_MANIFEST_SHA256,
        }
    }

    pub(crate) fn rejected(message: impl Into<String>) -> Self {
        Self::empty(false, message)
    }
}

/// The checks task submission applies to an hcode source, input, and replica
/// count, so a dry run never accepts what a submit would refuse.
fn validate(body: &DryRunBody) -> Result<(), &'static str> {
    validate_runtime_contract(&CreateTaskBody {
        runtime: Some("hcode".into()),
        task_source: Some(body.task_source.clone()),
        torrent: body.torrent.clone(),
        managed_replica_count: body.managed_replica_count,
        // A dry run carries no budget; the contract only requires a positive one.
        max_cpt: Some(1),
        ..CreateTaskBody::default()
    })
}

fn preview(output: &str) -> (String, bool) {
    if output.len() <= OUTPUT_PREVIEW_BYTES {
        return (output.to_owned(), false);
    }
    let mut end = OUTPUT_PREVIEW_BYTES;
    while !output.is_char_boundary(end) {
        end -= 1;
    }
    (output[..end].to_owned(), true)
}

fn replica_count(body: &DryRunBody) -> (u16, &'static str) {
    match body
        .managed_replica_count
        .and_then(|count| u16::try_from(count).ok())
    {
        Some(count) => (count, "requested"),
        None => (DEFAULT_REPLICA_COUNT, "default"),
    }
}

/// Runs the source and prices the task. The body must already be validated.
pub(crate) fn analyze(body: &DryRunBody, limits: DryRunLimits) -> DryRunResponse {
    let (replicas, replica_source) = replica_count(body);
    let input = body.torrent.as_deref().unwrap_or_default();
    match dry_run(&body.task_source, input, limits) {
        Ok(report) => {
            let usage = report.receipt.usage_units;
            let (output, truncated) = preview(&report.output);
            let mut response = DryRunResponse::empty(true, "The run completed");
            response.output_bytes = report.output.len();
            response.output = output;
            response.output_truncated = truncated;
            response.exceeds_consensus_result_limit =
                report.output.len() > hivemind_proto::MANAGED_CONSENSUS_RESULT_MAX_BYTES;
            response.usage_units = Some(usage);
            match required_task_budget(usage, replicas) {
                Ok(budget) => {
                    response.quote = Some(DryRunQuote {
                        replica_usage_units: budget.replica_usage,
                        replica_count: budget.replica_count,
                        replica_count_source: replica_source,
                        reserved_usage_cpt: budget.reserved_usage,
                        platform_fee_cpt: budget.fee,
                        required_max_cpt: budget.task_budget,
                    });
                }
                Err(error) => {
                    response.success = false;
                    response.message = error.to_string();
                }
            }
            response
        }
        Err(failure) => {
            let cause = match failure.cause {
                FailureCause::Program => "program",
                FailureCause::UsageLimit => "usage_limit",
                FailureCause::TimedOut => "timed_out",
            };
            let message = match failure.cause {
                FailureCause::Program => "The program failed",
                FailureCause::UsageLimit => {
                    "The run reached the dry-run usage limit before it finished"
                }
                FailureCause::TimedOut => {
                    "The run reached the dry-run time limit before it finished"
                }
            };
            let mut response = DryRunResponse::empty(false, message);
            response.usage_units = failure.usage_units;
            response.error = Some(DryRunError {
                code: failure.code,
                message: failure.message,
                line: failure.line,
                column: failure.column,
                cause,
                usage_units: failure.usage_units,
            });
            response
        }
    }
}

fn slots() -> &'static Arc<Semaphore> {
    static SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SLOTS.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_RUNS)))
}

async fn run_limited(
    slots: Arc<Semaphore>,
    body: DryRunBody,
    limits: DryRunLimits,
) -> (StatusCode, DryRunResponse) {
    let Ok(permit) = slots.try_acquire_owned() else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            DryRunResponse::rejected("Too many dry runs are running; try again in a moment"),
        );
    };
    // The permit stays with the blocking task, so a client that disconnects does
    // not free a slot while its run is still using the CPU.
    let joined = tokio::task::spawn_blocking(move || {
        let response = analyze(&body, limits);
        drop(permit);
        response
    })
    .await;
    match joined {
        Ok(response) => (StatusCode::OK, response),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            DryRunResponse::rejected("The dry run stopped unexpectedly"),
        ),
    }
}

/// POST /api/tasks/dry-run
pub async fn dry_run_task(
    _user: AuthUser,
    Json(body): Json<DryRunBody>,
) -> (StatusCode, Json<DryRunResponse>) {
    if let Err(message) = validate(&body) {
        return (
            StatusCode::BAD_REQUEST,
            Json(DryRunResponse::rejected(message)),
        );
    }
    let (status, response) = run_limited(Arc::clone(slots()), body, DryRunLimits::default()).await;
    (status, Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::RawToken;
    use axum::body::Body;
    use axum::http::{header, Request};
    use axum::{routing::post, Router};
    use hivemind_models::Claims;
    use std::time::Duration;
    use tower::ServiceExt;

    const SUM_LIST: &str = "let total = 0;\nfor value in get(input, \"values\") {\n  let total = total + value;\n}\ntotal;";
    const SUM_INPUT: &str = r#"{"values": [10, 20, 30]}"#;

    fn body(source: &str, input: &str, replicas: Option<u32>) -> DryRunBody {
        DryRunBody {
            task_source: source.into(),
            torrent: Some(input.into()),
            managed_replica_count: replicas,
        }
    }

    fn limits() -> DryRunLimits {
        DryRunLimits {
            max_usage_units: 1_000_000,
            wall_time: Duration::from_secs(30),
        }
    }

    #[test]
    fn a_completed_run_is_priced_for_three_replicas_by_default() {
        let response = analyze(&body(SUM_LIST, SUM_INPUT, None), limits());
        assert!(response.success, "{}", response.message);
        assert_eq!(response.output, "60");
        assert_eq!(response.usage_units, Some(25));
        let quote = response.quote.unwrap();
        assert_eq!(quote.replica_usage_units, 25);
        assert_eq!(quote.replica_count, 3);
        assert_eq!(quote.replica_count_source, "default");
        assert_eq!(quote.reserved_usage_cpt, 75);
        assert_eq!(quote.platform_fee_cpt, 7);
        assert_eq!(quote.required_max_cpt, 82);
    }

    #[test]
    fn a_requested_replica_count_changes_the_quote() {
        let response = analyze(&body(SUM_LIST, SUM_INPUT, Some(2)), limits());
        let quote = response.quote.unwrap();
        assert_eq!(quote.replica_count, 2);
        assert_eq!(quote.replica_count_source, "requested");
        assert_eq!(quote.reserved_usage_cpt, 50);
        assert_eq!(quote.platform_fee_cpt, 5);
        assert_eq!(quote.required_max_cpt, 55);
    }

    #[test]
    fn printed_text_is_the_output_a_worker_would_report() {
        let response = analyze(
            &body("print(\"step 1\");\nprint(\"step 2\");\n3;", "{}", None),
            limits(),
        );
        assert_eq!(response.output, "step 1\nstep 2\n");
        assert_eq!(response.usage_units, Some(16));
    }

    #[test]
    fn a_syntax_error_names_its_position_and_has_no_quote() {
        let response = analyze(&body("let x = 1;\nlet y = ;", "{}", None), limits());
        assert!(!response.success);
        assert!(response.quote.is_none());
        let error = response.error.unwrap();
        assert_eq!(error.code, "parse_error");
        assert_eq!(error.cause, "program");
        assert_eq!(error.line, Some(2));
    }

    #[test]
    fn reaching_the_usage_limit_is_not_reported_as_a_program_error() {
        let response = analyze(
            &body(SUM_LIST, SUM_INPUT, None),
            DryRunLimits {
                max_usage_units: 10,
                wall_time: Duration::from_secs(30),
            },
        );
        assert!(!response.success);
        assert!(response.quote.is_none());
        assert_eq!(response.error.unwrap().cause, "usage_limit");
    }

    #[test]
    fn a_long_output_is_cut_at_a_character_boundary() {
        let text = "你".repeat(OUTPUT_PREVIEW_BYTES);
        let (cut, truncated) = preview(&text);
        assert!(truncated);
        assert!(cut.len() <= OUTPUT_PREVIEW_BYTES);
        assert!(text.starts_with(&cut));
        assert_eq!(preview("short"), ("short".to_owned(), false));
    }

    #[test]
    fn the_response_names_the_interpreter_that_ran() {
        let response = analyze(&body(SUM_LIST, SUM_INPUT, None), limits());
        assert_eq!(
            response.semantics_manifest_sha256,
            managed_function_runtime::V1_SEMANTICS_MANIFEST_SHA256
        );
        assert_eq!(
            response.consensus_result_limit_bytes,
            hivemind_proto::MANAGED_CONSENSUS_RESULT_MAX_BYTES
        );
    }

    #[test]
    fn validation_matches_task_submission() {
        assert!(validate(&body(SUM_LIST, SUM_INPUT, None)).is_ok());
        assert_eq!(
            validate(&body("  ", SUM_INPUT, None)),
            Err("hcode requires non-empty task_source")
        );
        assert_eq!(
            validate(&body(SUM_LIST, "  ", None)),
            Err("hcode requires non-empty JSON input")
        );
        assert_eq!(
            validate(&DryRunBody {
                task_source: SUM_LIST.into(),
                torrent: None,
                managed_replica_count: None,
            }),
            Err("hcode requires non-empty JSON input")
        );
        assert_eq!(
            validate(&body(SUM_LIST, SUM_INPUT, Some(1))),
            Err("managed consensus replica count must be between 2 and 7")
        );
        assert_eq!(
            validate(&body(SUM_LIST, SUM_INPUT, Some(8))),
            Err("managed consensus replica count must be between 2 and 7")
        );
        let oversized = "s".repeat(hivemind_proto::MANAGED_TASK_SOURCE_MAX_BYTES + 1);
        assert_eq!(
            validate(&body(&oversized, SUM_INPUT, None)),
            Err("hcode task_source exceeds the byte limit")
        );
    }

    #[tokio::test]
    async fn no_free_slot_answers_429_without_running() {
        let (status, response) = run_limited(
            Arc::new(Semaphore::new(0)),
            body(SUM_LIST, SUM_INPUT, None),
            limits(),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert!(!response.success);
        assert!(response.usage_units.is_none());
    }

    #[tokio::test]
    async fn a_slot_is_free_again_after_the_run() {
        let slots = Arc::new(Semaphore::new(1));
        for _ in 0..3 {
            let (status, response) = run_limited(
                Arc::clone(&slots),
                body(SUM_LIST, SUM_INPUT, None),
                limits(),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(response.success);
        }
        assert_eq!(slots.available_permits(), 1);
    }

    fn app() -> Router {
        Router::new().route("/api/tasks/dry-run", post(dry_run_task))
    }

    fn claims() -> Claims {
        Claims {
            sub: "dry-run-test-user".into(),
            user_id: "dry-run-test-user".into(),
            role: None,
            task_id: None,
            worker_id: None,
            exp: usize::MAX,
            iat: 0,
        }
    }

    async fn post_json(app: Router, value: serde_json::Value) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tasks/dry-run")
                    .header(header::CONTENT_TYPE, "application/json")
                    .extension(claims())
                    .extension(RawToken("dry-run-test-token".into()))
                    .body(Body::from(value.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn the_endpoint_runs_and_quotes() {
        let (status, value) = post_json(
            app(),
            serde_json::json!({ "task_source": SUM_LIST, "torrent": SUM_INPUT }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["success"], true);
        assert_eq!(value["output"], "60");
        assert_eq!(value["usage_units"], 25);
        assert_eq!(value["quote"]["required_max_cpt"], 82);
        assert_eq!(value["error"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn the_endpoint_rejects_what_a_submit_would_reject() {
        let (status, value) = post_json(
            app(),
            serde_json::json!({ "task_source": SUM_LIST, "torrent": "" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["success"], false);
        assert_eq!(value["message"], "hcode requires non-empty JSON input");
    }

    #[tokio::test]
    async fn the_endpoint_reports_a_failing_program_with_200() {
        let (status, value) = post_json(
            app(),
            serde_json::json!({ "task_source": "let y = ;", "torrent": "{}" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["success"], false);
        assert_eq!(value["error"]["code"], "parse_error");
        assert_eq!(value["error"]["line"], 1);
    }

    fn limited_app() -> Router {
        Router::new().route(
            "/api/tasks/dry-run",
            post(dry_run_task).layer(axum::extract::DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
    }

    #[tokio::test]
    async fn an_input_at_the_contract_limit_fits_even_when_escaping_grows_it() {
        // Newlines written as `\n` are two bytes each in the input, and the request body escapes
        // the backslash again, so this input (just under the 1 MiB limit) makes a body of 1.5 MiB.
        let input = format!("\"{}\"", "\\n".repeat(511 * 1024));
        assert!(input.len() <= hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES);
        let body =
            serde_json::json!({ "task_source": "len(input);", "torrent": input }).to_string();
        assert!(body.len() > hivemind_proto::MANAGED_JSON_INPUT_MAX_BYTES + 64 * 1024);

        let (status, value) = post_json(
            limited_app(),
            serde_json::json!({ "task_source": "len(input);", "torrent": input }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["success"], true);
        assert_eq!(value["output"], (511 * 1024).to_string());
    }

    #[tokio::test]
    async fn a_body_past_the_read_limit_is_refused_with_413() {
        let response = limited_app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tasks/dry-run")
                    .header(header::CONTENT_TYPE, "application/json")
                    .extension(claims())
                    .extension(RawToken("dry-run-test-token".into()))
                    .body(Body::from(vec![b'x'; MAX_REQUEST_BYTES + 1]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn a_forged_api_token_cannot_run_without_nodepool() {
        if crate::integration_tests::isolated_local_mode(
            "dry_run::tests::a_forged_api_token_cannot_run_without_nodepool",
        ) {
            return;
        }

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        drop(listener);
        let mut config = hivemind_config::HivemindConfig::default();
        config.server.nodepool_grpc_endpoint = Some(endpoint.clone());
        let router = crate::routes::create_router(crate::handlers::AppState {
            grpc_client: crate::grpc_client::GrpcClient::new(endpoint),
            config,
            task_submit_limiter: Arc::new(tokio::sync::Mutex::new(
                crate::handlers::TaskSubmitRateLimiter::new(),
            )),
        });
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tasks/dry-run")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, "Bearer hmk_forged_not_a_real_token")
                    .body(Body::from(
                        serde_json::json!({ "task_source": "42;", "torrent": "{}" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["success"], false);
        assert_eq!(value["output"], "");
        assert!(value["usage_units"].is_null());
        assert!(value["quote"].is_null());
        assert!(!value.to_string().contains("hmk_forged_not_a_real_token"));
    }

    #[tokio::test]
    async fn a_stalled_nodepool_fails_closed_before_dry_run_admission() {
        if crate::integration_tests::isolated_local_mode(
            "dry_run::tests::a_stalled_nodepool_fails_closed_before_dry_run_admission",
        ) {
            return;
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let (_connection, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let held_slots = Arc::clone(slots())
            .acquire_many_owned(MAX_CONCURRENT_RUNS as u32)
            .await
            .unwrap();
        let mut config = hivemind_config::HivemindConfig::default();
        config.server.nodepool_grpc_endpoint = Some(endpoint.clone());
        let router = crate::routes::create_router(crate::handlers::AppState {
            grpc_client: crate::grpc_client::GrpcClient::new(endpoint),
            config,
            task_submit_limiter: Arc::new(tokio::sync::Mutex::new(
                crate::handlers::TaskSubmitRateLimiter::new(),
            )),
        });
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            router.oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tasks/dry-run")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(
                        header::AUTHORIZATION,
                        "Bearer hmk_unverified_pending_authority",
                    )
                    .body(Body::from(
                        serde_json::json!({ "task_source": "42;", "torrent": "{}" }).to_string(),
                    ))
                    .unwrap(),
            ),
        )
        .await
        .expect("Nodepool authorization must have a bounded timeout")
        .unwrap();
        server.abort();
        drop(held_slots);
        // Authorization failure wins over a full local execution queue.
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["success"], false);
        assert_eq!(value["output"], "");
        assert!(value["usage_units"].is_null());
        assert!(value["quote"].is_null());
        assert!(!value
            .to_string()
            .contains("hmk_unverified_pending_authority"));
    }

    #[tokio::test]
    async fn the_route_requires_a_signed_in_user() {
        use crate::grpc_client::GrpcClient;
        use crate::handlers::{AppState, TaskSubmitRateLimiter};

        let router = crate::routes::create_router(AppState {
            grpc_client: GrpcClient::new("127.0.0.1:50051"),
            config: hivemind_config::HivemindConfig::default(),
            task_submit_limiter: Arc::new(tokio::sync::Mutex::new(TaskSubmitRateLimiter::new())),
        });
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tasks/dry-run")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({ "task_source": SUM_LIST, "torrent": SUM_INPUT })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
