use hivemind_proto::{
    vpn_service_server::VpnService, GetTaskPeersRequest, GetTaskPeersResponse,
    IssueUserVpnConfigRequest, IssueUserVpnConfigResponse, JoinVpnRequest, JoinVpnResponse,
    LeaveVpnRequest, LeaveVpnResponse, PeerInfo as ProtoPeerInfo, UpdateVpnStatusRequest,
    UpdateVpnStatusResponse,
};
use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::VpnService as VpnSvc;

pub struct GrpcVpnService {
    vpn: Arc<VpnSvc>,
}

impl GrpcVpnService {
    pub fn new(vpn: Arc<VpnSvc>) -> Self {
        Self { vpn }
    }
}

#[tonic::async_trait]
impl VpnService for GrpcVpnService {
    async fn join_vpn(
        &self,
        request: Request<JoinVpnRequest>,
    ) -> Result<Response<JoinVpnResponse>, Status> {
        let req = request.into_inner();
        match self
            .vpn
            .join_vpn(&req.worker_id, &req.hostname, &req.auth_token)
            .await
        {
            Ok(peer) => Ok(Response::new(JoinVpnResponse {
                success: true,
                status_message: "Joined VPN".into(),
                virtual_ip: peer.virtual_ip,
                auth_key: peer.auth_key,
                derp_map: String::new(),
            })),
            Err(e) => Ok(Response::new(JoinVpnResponse {
                success: false,
                status_message: e.to_string(),
                virtual_ip: String::new(),
                auth_key: String::new(),
                derp_map: String::new(),
            })),
        }
    }

    async fn get_task_peers(
        &self,
        request: Request<GetTaskPeersRequest>,
    ) -> Result<Response<GetTaskPeersResponse>, Status> {
        let req = request.into_inner();
        match self
            .vpn
            .get_task_peers(&req.task_id, &req.worker_id, &req.auth_token)
            .await
        {
            Ok(peers) => {
                let infos: Vec<ProtoPeerInfo> = peers
                    .into_iter()
                    .map(|p| ProtoPeerInfo {
                        worker_id: p.worker_id,
                        virtual_ip: p.virtual_ip,
                        hostname: p.hostname,
                        online: p.online,
                        last_seen: p.last_seen,
                    })
                    .collect();
                Ok(Response::new(GetTaskPeersResponse {
                    success: true,
                    status_message: "OK".into(),
                    peers: infos,
                }))
            }
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn leave_vpn(
        &self,
        request: Request<LeaveVpnRequest>,
    ) -> Result<Response<LeaveVpnResponse>, Status> {
        let req = request.into_inner();
        match self.vpn.leave_vpn(&req.worker_id, &req.auth_token).await {
            Ok(_) => Ok(Response::new(LeaveVpnResponse {
                success: true,
                status_message: "Left VPN".into(),
            })),
            Err(e) => Ok(Response::new(LeaveVpnResponse {
                success: false,
                status_message: e.to_string(),
            })),
        }
    }

    async fn update_vpn_status(
        &self,
        request: Request<UpdateVpnStatusRequest>,
    ) -> Result<Response<UpdateVpnStatusResponse>, Status> {
        let req = request.into_inner();
        match self
            .vpn
            .update_vpn_status(&req.worker_id, &req.virtual_ip, req.online, &req.auth_token)
            .await
        {
            Ok(_) => Ok(Response::new(UpdateVpnStatusResponse {
                success: true,
                status_message: "Status updated".into(),
            })),
            Err(e) => Ok(Response::new(UpdateVpnStatusResponse {
                success: false,
                status_message: e.to_string(),
            })),
        }
    }
    async fn issue_user_vpn_config(
        &self,
        request: Request<IssueUserVpnConfigRequest>,
    ) -> Result<Response<IssueUserVpnConfigResponse>, Status> {
        let req = request.into_inner();
        match self
            .vpn
            .issue_user_vpn_config_from_token(&req.token, &req.client_name)
            .await
        {
            Ok(cfg) => Ok(Response::new(IssueUserVpnConfigResponse {
                success: true,
                status_message: "VPN config issued".into(),
                login_server: cfg.login_server,
                auth_key: cfg.auth_key,
                virtual_ip: cfg.virtual_ip,
                client_id: cfg.client_id,
                config_text: cfg.config_text,
                expires_at: cfg.expires_at,
            })),
            Err(e) => Ok(Response::new(IssueUserVpnConfigResponse {
                success: false,
                status_message: e.to_string(),
                login_server: String::new(),
                auth_key: String::new(),
                virtual_ip: String::new(),
                client_id: String::new(),
                config_text: String::new(),
                expires_at: String::new(),
            })),
        }
    }
}
