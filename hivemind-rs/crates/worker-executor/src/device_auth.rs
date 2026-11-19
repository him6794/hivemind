//! Headless OAuth 2.0 Device Authorization Grant (RFC 8628) helper for CLI,
//! Linux servers, and IoT / Raspberry Pi Workers (Milestone 3.2 #5).

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAuthorizationPrompt {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in_secs: u64,
    pub interval_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePollResult {
    AuthorizationPending,
    SlowDown,
    Authorized {
        access_token: String,
        refresh_token: Option<String>,
    },
    AccessDenied,
    ExpiredToken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceAuthError {
    Expired,
    Denied,
}

pub struct DeviceAuthSession {
    prompt: DeviceAuthorizationPrompt,
    started_at: Instant,
    poll_interval: Duration,
}

impl DeviceAuthSession {
    #[must_use]
    pub fn start(prompt: DeviceAuthorizationPrompt) -> Self {
        let interval = Duration::from_secs(prompt.interval_secs.max(1));
        Self {
            prompt,
            started_at: Instant::now(),
            poll_interval: interval,
        }
    }

    #[must_use]
    pub fn prompt(&self) -> &DeviceAuthorizationPrompt {
        &self.prompt
    }

    #[must_use]
    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.started_at.elapsed() >= Duration::from_secs(self.prompt.expires_in_secs)
    }

    /// URL suitable for rendering as a terminal QR code or clickable link.
    #[must_use]
    pub fn qr_target_uri(&self) -> String {
        if let Some(full) = &self.prompt.verification_uri_complete {
            if !full.trim().is_empty() {
                return full.clone();
            }
        }
        let sep = if self.prompt.verification_uri.contains('?') {
            '&'
        } else {
            '?'
        };
        format!(
            "{}{}user_code={}",
            self.prompt.verification_uri, sep, self.prompt.user_code
        )
    }

    /// Human-readable terminal instructions for headless / SSH / Raspberry Pi login.
    #[must_use]
    pub fn format_terminal_instructions(&self) -> String {
        format!(
            "To authorize this Hivemind Worker on a headless device:\n  1. Visit: {}\n  2. Enter code: {}\n  Or scan QR URI: {}\n",
            self.prompt.verification_uri,
            self.prompt.user_code,
            self.qr_target_uri(),
        )
    }

    /// Process a polling response from the authorization server according to RFC 8628 §3.5.
    pub fn apply_poll_result(
        &mut self,
        outcome: DevicePollResult,
    ) -> Result<Option<(String, Option<String>)>, DeviceAuthError> {
        if self.is_expired() {
            return Err(DeviceAuthError::Expired);
        }
        match outcome {
            DevicePollResult::AuthorizationPending => Ok(None),
            DevicePollResult::SlowDown => {
                // RFC 8628 §3.5: MUST increase polling interval by 5 seconds.
                self.poll_interval += Duration::from_secs(5);
                Ok(None)
            }
            DevicePollResult::Authorized {
                access_token,
                refresh_token,
            } => Ok(Some((access_token, refresh_token))),
            DevicePollResult::AccessDenied => Err(DeviceAuthError::Denied),
            DevicePollResult::ExpiredToken => Err(DeviceAuthError::Expired),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_auth_session_formats_qr_uri_and_handles_rfc8628_slow_down() {
        let mut session = DeviceAuthSession::start(DeviceAuthorizationPrompt {
            device_code: "dev-123".to_string(),
            user_code: "WDJB-MJHT".to_string(),
            verification_uri: "https://hivemind.example/activate".to_string(),
            verification_uri_complete: None,
            expires_in_secs: 600,
            interval_secs: 5,
        });

        assert_eq!(
            session.qr_target_uri(),
            "https://hivemind.example/activate?user_code=WDJB-MJHT"
        );
        assert!(session.format_terminal_instructions().contains("WDJB-MJHT"));

        assert_eq!(
            session
                .apply_poll_result(DevicePollResult::AuthorizationPending)
                .unwrap(),
            None
        );
        assert_eq!(session.poll_interval(), Duration::from_secs(5));

        assert_eq!(
            session
                .apply_poll_result(DevicePollResult::SlowDown)
                .unwrap(),
            None
        );
        assert_eq!(session.poll_interval(), Duration::from_secs(10));

        let token = session
            .apply_poll_result(DevicePollResult::Authorized {
                access_token: "jwt-ok".to_string(),
                refresh_token: Some("ref-1".to_string()),
            })
            .unwrap();
        assert_eq!(
            token,
            Some(("jwt-ok".to_string(), Some("ref-1".to_string())))
        );
    }

    #[test]
    fn device_auth_session_rejects_denied_or_expired() {
        let mut session = DeviceAuthSession::start(DeviceAuthorizationPrompt {
            device_code: "dev-2".to_string(),
            user_code: "AAAA-BBBB".to_string(),
            verification_uri: "https://hivemind.example/activate".to_string(),
            verification_uri_complete: Some(
                "https://hivemind.example/activate?user_code=AAAA-BBBB".to_string(),
            ),
            expires_in_secs: 300,
            interval_secs: 5,
        });
        assert_eq!(
            session.apply_poll_result(DevicePollResult::AccessDenied),
            Err(DeviceAuthError::Denied)
        );
    }
}
