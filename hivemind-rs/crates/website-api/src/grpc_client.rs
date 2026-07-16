use hivemind_proto::{
    user_service_client::UserServiceClient, vpn_service_client::VpnServiceClient,
    GetBalanceRequest, GetBalanceResponse, IssueUserVpnConfigRequest, IssueUserVpnConfigResponse,
    LoginRequest, LoginResponse, RegisterUserRequest, RegisterUserResponse, TransferCptRequest,
    TransferCptResponse,
};
use tokio::time::{sleep, Duration};
use tonic::transport::{Channel, Endpoint};
use tonic::Request;

#[derive(Clone)]
pub struct GrpcClient {
    user: UserServiceClient<Channel>,
    vpn: VpnServiceClient<Channel>,
}

impl GrpcClient {
    pub async fn connect(addr: &str) -> Result<Self, tonic::transport::Error> {
        let endpoint =
            Endpoint::from_shared(format!("http://{}", addr))?.http2_adaptive_window(true);
        let channel = endpoint.connect().await?;
        Ok(Self {
            user: UserServiceClient::new(channel.clone()),
            vpn: VpnServiceClient::new(channel),
        })
    }

    pub async fn connect_with_retry(
        addr: &str,
        attempts: usize,
        delay: Duration,
    ) -> Result<Self, tonic::transport::Error> {
        let mut last_err = None;
        for _ in 0..attempts.max(1) {
            match Self::connect(addr).await {
                Ok(client) => return Ok(client),
                Err(err) => {
                    last_err = Some(err);
                    sleep(delay).await;
                }
            }
        }
        Err(last_err.expect("attempts.max(1) ensures at least one error"))
    }

    pub async fn register_user(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<RegisterUserResponse, tonic::Status> {
        self.user
            .register_user(Request::new(RegisterUserRequest {
                username: username.to_string(),
                password: password.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }

    pub async fn login(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<LoginResponse, tonic::Status> {
        self.user
            .login(Request::new(LoginRequest {
                username: username.to_string(),
                password: password.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }

    pub async fn get_balance(
        &mut self,
        username: &str,
        token: &str,
    ) -> Result<GetBalanceResponse, tonic::Status> {
        self.user
            .get_balance(Request::new(GetBalanceRequest {
                username: username.to_string(),
                token: token.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }

    pub async fn transfer_cpt(
        &mut self,
        token: &str,
        to_username: &str,
        amount_cpt: i64,
        idempotency_key: &str,
    ) -> Result<TransferCptResponse, tonic::Status> {
        self.user
            .transfer_cpt(Request::new(TransferCptRequest {
                token: token.to_string(),
                to_username: to_username.to_string(),
                amount_cpt,
                idempotency_key: idempotency_key.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }

    pub async fn issue_user_vpn_config(
        &mut self,
        token: &str,
        client_name: &str,
    ) -> Result<IssueUserVpnConfigResponse, tonic::Status> {
        self.vpn
            .issue_user_vpn_config(Request::new(IssueUserVpnConfigRequest {
                token: token.to_string(),
                client_name: client_name.to_string(),
            }))
            .await
            .map(|r| r.into_inner())
    }
}
