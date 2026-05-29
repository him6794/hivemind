tonic::include_proto!("nodepool");

pub use vpn_service_client::VpnServiceClient;
pub use vpn_service_server::{VpnService, VpnServiceServer};
pub use node_manager_service_client::NodeManagerServiceClient;
pub use node_manager_service_server::{NodeManagerService, NodeManagerServiceServer};
pub use master_node_service_client::MasterNodeServiceClient;
pub use master_node_service_server::{MasterNodeService, MasterNodeServiceServer};
pub use worker_node_service_client::WorkerNodeServiceClient;
pub use worker_node_service_server::{WorkerNodeService, WorkerNodeServiceServer};
pub use user_service_client::UserServiceClient;
pub use user_service_server::{UserService, UserServiceServer};
pub use batch_runtime_service_client::BatchRuntimeServiceClient;
pub use batch_runtime_service_server::{BatchRuntimeService, BatchRuntimeServiceServer};