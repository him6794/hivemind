use hivemind_proto::{
    worker_node_service_server::WorkerNodeService, ExecuteTaskRequest, ExecuteTaskResponse,
    StopTaskExecutionRequest, StopTaskExecutionResponse, TaskOutputRequest, TaskOutputResponse,
    TaskOutputUploadRequest, TaskOutputUploadResponse, TaskResultUploadRequest,
    TaskResultUploadResponse, TaskUsageRequest, TaskUsageResponse,
};
use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::WorkerExecutor;
use hivemind_config::HivemindConfig;
use hivemind_models::{Task, TaskStatus};

pub struct WorkerGrpcState {
    pub config: HivemindConfig,
    pub executor: Arc<WorkerExecutor>,
}

pub struct GrpcWorkerNodeService {
    state: Arc<WorkerGrpcState>,
}

impl GrpcWorkerNodeService {
    pub fn new(state: Arc<WorkerGrpcState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl WorkerNodeService for GrpcWorkerNodeService {
    async fn execute_task(
        &self,
        request: Request<ExecuteTaskRequest>,
    ) -> Result<Response<ExecuteTaskResponse>, Status> {
        let req = request.into_inner();
        let limits = req.resource_limits.unwrap_or_default();
        let task = Task {
            id: uuid::Uuid::new_v4(),
            task_id: req.task_id.clone(),
            owner: String::new(),
            worker_id: None,
            worker_ip: None,
            status: TaskStatus::Running,
            status_message: None,
            output: None,
            result_torrent: None,
            torrent_source: Some(req.torrent),
            expected_btih: None,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: 0.0,
            gpu_memory_usage: 0.0,
            req_cpu_score: limits.cpu_score,
            req_gpu_score: limits.gpu_score,
            req_memory_gb: (limits.memory_mb / 1024) as i32,
            req_gpu_memory_gb: (limits.vram_mb / 1024) as i32,
            req_storage_gb: limits.storage_total_gb,
            host_count: 1,
            max_cpt: 0,
            billing_settled: false,
            billed_amount: 0,
            retry_count: 0,
            max_retries: 3,
            deadline: None,
            deterministic: false,
            side_effects: false,
            priority: 0,
            cpu_time_ms: 0,
            wall_time_ms: 0,
            peak_memory_mb: 0,
            download_bytes: 0,
            cache_hits: 0,
            created_at: chrono::Utc::now(),
            last_update: chrono::Utc::now(),
            completed_at: None,
        };
        tracing::info!("Worker executing task {}", req.task_id);
        match self.state.executor.execute_task(&task).await {
            Ok(result) => {
                if result.success {
                    Ok(Response::new(ExecuteTaskResponse {
                        success: true,
                        status_message: result.output.unwrap_or_default(),
                    }))
                } else {
                    Ok(Response::new(ExecuteTaskResponse {
                        success: false,
                        status_message: result.error.unwrap_or_default(),
                    }))
                }
            }
            Err(e) => Ok(Response::new(ExecuteTaskResponse {
                success: false,
                status_message: e.to_string(),
            })),
        }
    }

    async fn task_output_upload(
        &self,
        request: Request<TaskOutputUploadRequest>,
    ) -> Result<Response<TaskOutputUploadResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(
            "Output upload task {} ({} bytes)",
            req.task_id,
            req.output.len()
        );
        Ok(Response::new(TaskOutputUploadResponse {
            success: true,
            status_message: "OK".into(),
        }))
    }

    async fn task_result_upload(
        &self,
        request: Request<TaskResultUploadRequest>,
    ) -> Result<Response<TaskResultUploadResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(
            "Result upload task {} torrent={}",
            req.task_id,
            req.result_torrent
        );
        Ok(Response::new(TaskResultUploadResponse {
            success: true,
            status_message: "OK".into(),
        }))
    }

    async fn task_output(
        &self,
        request: Request<TaskOutputRequest>,
    ) -> Result<Response<TaskOutputResponse>, Status> {
        let _req = request.into_inner();
        Ok(Response::new(TaskOutputResponse {
            success: true,
            status_message: "OK".into(),
            output: String::new(),
        }))
    }

    async fn stop_task_execution(
        &self,
        request: Request<StopTaskExecutionRequest>,
    ) -> Result<Response<StopTaskExecutionResponse>, Status> {
        let req = request.into_inner();
        tracing::info!("Stop task {}", req.task_id);
        Ok(Response::new(StopTaskExecutionResponse {
            success: true,
            status_message: "Stopped".into(),
        }))
    }

    async fn task_usage(
        &self,
        request: Request<TaskUsageRequest>,
    ) -> Result<Response<TaskUsageResponse>, Status> {
        let req = request.into_inner();
        let usage = req.usage.unwrap_or_default();
        tracing::debug!(
            "Task {} usage: cpu={:.1}% mem={:.1}%",
            req.task_id,
            usage.cpu_percent,
            usage.memory_percent
        );
        Ok(Response::new(TaskUsageResponse {
            success: true,
            status_message: "OK".into(),
        }))
    }
}
