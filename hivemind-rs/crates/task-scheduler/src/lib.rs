pub mod task_repository;
pub mod scheduler;
pub mod dispatcher;

use anyhow::Result;
use hivemind_auth::AuthManager;
use hivemind_database::DatabaseManager;
use hivemind_models::{Task, TaskStatus};
use std::sync::Arc;

pub struct TaskScheduler {
    repo: Arc<task_repository::TaskRepository>,
    auth: AuthManager,
    db: DatabaseManager,
}

impl Clone for TaskScheduler {
    fn clone(&self) -> Self {
        Self {
            repo: self.repo.clone(),
            auth: self.auth.clone(),
            db: self.db.clone(),
        }
    }
}

impl TaskScheduler {
    pub fn new(db: DatabaseManager, auth: AuthManager) -> Self {
        Self {
            repo: Arc::new(task_repository::TaskRepository::new(db.pool.clone())),
            auth,
            db,
        }
    }

    pub async fn create_task(&self, task: &Task) -> Result<Task> {
        self.repo.create(task).await
    }

    pub async fn get_task(&self, task_id: &str) -> Result<Option<Task>> {
        self.repo.find_by_task_id(task_id).await
    }

    pub async fn list_user_tasks(&self, owner: &str) -> Result<Vec<Task>> {
        self.repo.find_by_owner(owner).await
    }

    pub async fn update_task_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        message: Option<&str>,
    ) -> Result<Task> {
        self.repo.update_status(task_id, status, message).await
    }

    pub async fn assign_task_to_worker(
        &self,
        task_id: &str,
        worker_id: &str,
        worker_ip: &str,
    ) -> Result<Task> {
        self.repo.assign_to_worker(task_id, worker_id, worker_ip).await
    }

    pub async fn complete_task(
        &self,
        task_id: &str,
        result_torrent: Option<&str>,
        output: Option<&str>,
    ) -> Result<Task> {
        self.repo.complete(task_id, result_torrent, output).await
    }

    pub async fn fail_task(&self, task_id: &str, reason: &str) -> Result<Task> {
        self.repo.fail(task_id, reason).await
    }

    pub async fn cancel_task(&self, task_id: &str) -> Result<Task> {
        self.repo.cancel(task_id).await
    }

    pub async fn get_pending_tasks(&self) -> Result<Vec<Task>> {
        self.repo.find_pending().await
    }

    pub async fn find_suitable_worker(
        &self,
        task: &Task,
        workers: &[hivemind_models::WorkerNode],
    ) -> Option<hivemind_models::WorkerNode> {
        scheduler::find_best_worker(task, workers).await
    }

    pub fn database(&self) -> &DatabaseManager {
        &self.db
    }

    pub fn auth(&self) -> &AuthManager {
        &self.auth
    }
}