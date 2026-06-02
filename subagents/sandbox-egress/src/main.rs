use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::PathBuf;
use std::str::FromStr;

/// Egress policy applied per-task before execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressPolicy {
    pub task_id: String,
    /// "allowlist" or "denylist"
    pub mode: EgressMode,
    /// List of CIDR ranges or individual IPs
    pub targets: Vec<String>,
    /// Optional DNS domains to allow/block (if DNS resolution is permitted)
    pub allowed_domains: Vec<String>,
    /// Whether outbound connections are permitted at all
    pub outbound_enabled: bool,
    /// Max outbound bandwidth in bytes/sec (0 = unlimited)
    pub max_bandwidth_bytes_per_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EgressMode {
    Allowlist,
    Denylist,
}

impl Default for EgressPolicy {
    fn default() -> Self {
        Self {
            task_id: String::new(),
            mode: EgressMode::Denylist,
            targets: vec![],
            allowed_domains: vec![],
            outbound_enabled: true,
            max_bandwidth_bytes_per_sec: 0,
        }
    }
}

/// Firewall rule representation (platform-agnostic)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallRule {
    pub direction: RuleDirection,
    pub action: RuleAction,
    pub target: String,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleAction {
    Allow,
    Deny,
}

/// Convert an egress policy into a set of firewall rules for the host.
pub fn policy_to_rules(policy: &EgressPolicy) -> Vec<FirewallRule> {
    let mut rules = Vec::new();

    // Always deny inbound by default
    rules.push(FirewallRule {
        direction: RuleDirection::Inbound,
        action: RuleAction::Deny,
        target: "0.0.0.0/0".into(),
        protocol: "tcp".into(),
    });

    if !policy.outbound_enabled {
        rules.push(FirewallRule {
            direction: RuleDirection::Outbound,
            action: RuleAction::Deny,
            target: "0.0.0.0/0".into(),
            protocol: "tcp".into(),
        });
        return rules;
    }

    match policy.mode {
        EgressMode::Denylist => {
            for target in &policy.targets {
                rules.push(FirewallRule {
                    direction: RuleDirection::Outbound,
                    action: RuleAction::Deny,
                    target: target.clone(),
                    protocol: "tcp".into(),
                });
            }
            // Explicit allow for everything else
            rules.push(FirewallRule {
                direction: RuleDirection::Outbound,
                action: RuleAction::Allow,
                target: "0.0.0.0/0".into(),
                protocol: "tcp".into(),
            });
        }
        EgressMode::Allowlist => {
            // Deny all outbound by default
            rules.push(FirewallRule {
                direction: RuleDirection::Outbound,
                action: RuleAction::Deny,
                target: "0.0.0.0/0".into(),
                protocol: "tcp".into(),
            });
            for target in &policy.targets {
                rules.push(FirewallRule {
                    direction: RuleDirection::Outbound,
                    action: RuleAction::Allow,
                    target: target.clone(),
                    protocol: "tcp".into(),
                });
            }
        }
    }

    rules
}

/// Validate that all targets in the policy are valid CIDR or IP addresses.
pub fn validate_policy(policy: &EgressPolicy) -> Result<()> {
    for target in &policy.targets {
        if target.ends_with('/') {
            anyhow::bail!("Invalid CIDR target '{}': trailing slash", target);
        }
        if target.contains('/') {
            // Try to parse as CIDR
            ipnet::IpNet::from_str(target)
                .with_context(|| format!("Invalid CIDR target '{}'", target))?;
        } else {
            // Try to parse as IP
            IpAddr::from_str(target)
                .with_context(|| format!("Invalid IP target '{}'", target))?;
        }
    }
    Ok(())
}

/// Generate the sandbox configuration file that the executor reads.
pub fn write_policy_file(policy: &EgressPolicy, output_dir: &PathBuf) -> Result<PathBuf> {
    std::fs::create_dir_all(output_dir)?;
    let path = output_dir.join(format!("egress_{}.json", policy.task_id));
    let json = serde_json::to_string_pretty(policy)?;
    std::fs::write(&path, json)?;
    tracing::info!("Wrote egress policy to {}", path.display());
    Ok(path)
}

/// Read a policy from a JSON file.
pub fn read_policy_file(path: &PathBuf) -> Result<EgressPolicy> {
    let content = std::fs::read_to_string(path)?;
    let policy: EgressPolicy = serde_json::from_str(&content)?;
    Ok(policy)
}

/// Sandbox-ready egress configuration that can be embedded into task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxEgressConfig {
    pub task_id: String,
    pub firewall_rules: Vec<FirewallRule>,
    pub policy_file: Option<String>,
    pub max_bandwidth_bytes_per_sec: u64,
}

impl From<&EgressPolicy> for SandboxEgressConfig {
    fn from(policy: &EgressPolicy) -> Self {
        Self {
            task_id: policy.task_id.clone(),
            firewall_rules: policy_to_rules(policy),
            policy_file: None,
            max_bandwidth_bytes_per_sec: policy.max_bandwidth_bytes_per_sec,
        }
    }
}

/// Apply the egress config to the current sandbox environment.
/// On Linux this would use iptables/nftables. On Windows, use WFP via netsh.
#[cfg(target_os = "windows")]
pub fn apply_egress_config(config: &SandboxEgressConfig) -> Result<()> {
    use std::process::Command;

    // Remove any existing rules for this task
    let _ = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name=hivemind-{}", config.task_id),
        ])
        .output();

    for rule in &config.firewall_rules {
        let action = match rule.action {
            RuleAction::Allow => "allow",
            RuleAction::Deny => "block",
        };
        let direction = match rule.direction {
            RuleDirection::Inbound => "in",
            RuleDirection::Outbound => "out",
        };

        let status = Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name=hivemind-{}", config.task_id),
                &format!("dir={}", direction),
                &format!("action={}", action),
                &format!("remoteip={}", rule.target),
                &format!("protocol={}", rule.protocol),
            ])
            .status()
            .with_context(|| format!("Failed to apply firewall rule for task {}", config.task_id))?;

        if !status.success() {
            tracing::warn!("Firewall rule may not have applied for task {}", config.task_id);
        }
    }

    tracing::info!(
        "Applied {} egress rules for task {}",
        config.firewall_rules.len(),
        config.task_id
    );
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn apply_egress_config(config: &SandboxEgressConfig) -> Result<()> {
    use std::process::Command;

    // Clean up any existing iptables rules for this task
    let _ = Command::new("iptables")
        .args([
            "-D", "OUTPUT",
            "-m", "comment",
            "--comment", &format!("hivemind-{}", config.task_id),
            "-j", "DROP",
        ])
        .output();

    for rule in &config.firewall_rules {
        let action = match rule.action {
            RuleAction::Allow => "ACCEPT",
            RuleAction::Deny => "DROP",
        };

        let chain = match rule.direction {
            RuleDirection::Outbound => "OUTPUT",
            RuleDirection::Inbound => "INPUT",
        };

        let mut args = vec![
            "-A", chain,
            "-d", &rule.target,
            "-p", &rule.protocol,
            "-m", "comment",
            "--comment", &format!("hivemind-{}", config.task_id),
            "-j", action,
        ];

        let status = Command::new("iptables")
            .args(&args)
            .status()
            .with_context(|| format!("Failed to apply iptables rule for task {}", config.task_id))?;

        if !status.success() {
            tracing::warn!("iptables rule may not have applied for task {}", config.task_id);
        }
    }

    tracing::info!(
        "Applied {} iptables rules for task {}",
        config.firewall_rules.len(),
        config.task_id
    );
    Ok(())
}

/// Clean up all firewall rules for a task.
pub fn cleanup_egress_config(task_id: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let _ = Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "delete",
                "rule",
                &format!("name=hivemind-{}", task_id),
            ])
            .output();
    }

    #[cfg(not(target_os = "windows"))]
    {
        use std::process::Command;
        let _ = Command::new("iptables")
            .args([
                "-D", "OUTPUT",
                "-m", "comment",
                "--comment", &format!("hivemind-{}", task_id),
                "-j", "DROP",
            ])
            .output();
    }

    Ok(())
}

/// Integration with the executor: wraps task execution with egress policy.
pub async fn execute_with_egress<F, Fut>(policy: &EgressPolicy, f: F) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let config = SandboxEgressConfig::from(policy);
    apply_egress_config(&config)?;

    let result = f().await;

    // Always clean up, even on error
    cleanup_egress_config(&policy.task_id)?;

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_default_is_denylist() {
        let policy = EgressPolicy::default();
        assert!(matches!(policy.mode, EgressMode::Denylist));
        assert!(policy.targets.is_empty());
        assert!(policy.outbound_enabled);
    }

    #[test]
    fn test_allowlist_generates_correct_rules() {
        let policy = EgressPolicy {
            task_id: "task-1".into(),
            mode: EgressMode::Allowlist,
            targets: vec!["10.0.0.0/8".into(), "8.8.8.8".into()],
            allowed_domains: vec![],
            outbound_enabled: true,
            max_bandwidth_bytes_per_sec: 0,
        };

        let rules = policy_to_rules(&policy);

        // Should have: 1 inbound deny + 1 outbound deny-all + 2 allows
        assert_eq!(rules.len(), 4);

        let allows: Vec<_> = rules.iter().filter(|r| matches!(r.action, RuleAction::Allow)).collect();
        assert_eq!(allows.len(), 2);
        assert!(allows.iter().any(|r| r.target == "10.0.0.0/8"));
        assert!(allows.iter().any(|r| r.target == "8.8.8.8"));
    }

    #[test]
    fn test_denylist_generates_correct_rules() {
        let policy = EgressPolicy {
            task_id: "task-2".into(),
            mode: EgressMode::Denylist,
            targets: vec!["192.168.1.0/24".into()],
            allowed_domains: vec![],
            outbound_enabled: true,
            max_bandwidth_bytes_per_sec: 0,
        };

        let rules = policy_to_rules(&policy);

        // Should have: 1 inbound deny + 1 deny + 1 allow all
        assert_eq!(rules.len(), 3);
        let denies: Vec<_> = rules.iter().filter(|r| matches!(r.action, RuleAction::Deny)).collect();
        assert_eq!(denies.len(), 2); // inbound + target deny
    }

    #[test]
    fn test_outbound_disabled_blocks_all() {
        let policy = EgressPolicy {
            task_id: "task-3".into(),
            mode: EgressMode::Allowlist,
            targets: vec![],
            allowed_domains: vec![],
            outbound_enabled: false,
            max_bandwidth_bytes_per_sec: 0,
        };

        let rules = policy_to_rules(&policy);
        // Should have: 1 inbound deny + 1 outbound deny-all
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn test_validate_valid_targets() {
        let policy = EgressPolicy {
            task_id: "task-v".into(),
            mode: EgressMode::Allowlist,
            targets: vec!["10.0.0.0/8".into(), "1.1.1.1".into(), "::1".into()],
            allowed_domains: vec![],
            outbound_enabled: true,
            max_bandwidth_bytes_per_sec: 0,
        };
        assert!(validate_policy(&policy).is_ok());
    }

    #[test]
    fn test_validate_invalid_target() {
        let policy = EgressPolicy {
            task_id: "task-x".into(),
            mode: EgressMode::Allowlist,
            targets: vec!["not-an-ip".into()],
            allowed_domains: vec![],
            outbound_enabled: true,
            max_bandwidth_bytes_per_sec: 0,
        };
        assert!(validate_policy(&policy).is_err());
    }

    #[test]
    fn test_sandbox_egress_config_from_policy() {
        let policy = EgressPolicy {
            task_id: "task-cfg".into(),
            mode: EgressMode::Denylist,
            targets: vec!["10.0.0.0/8".into()],
            allowed_domains: vec![],
            outbound_enabled: true,
            max_bandwidth_bytes_per_sec: 1024000,
        };

        let config = SandboxEgressConfig::from(&policy);
        assert_eq!(config.task_id, "task-cfg");
        assert!(!config.firewall_rules.is_empty());
        assert_eq!(config.max_bandwidth_bytes_per_sec, 1024000);
    }

    #[test]
    fn test_policy_file_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let policy = EgressPolicy {
            task_id: "task-rt".into(),
            mode: EgressMode::Allowlist,
            targets: vec!["10.0.0.0/8".into()],
            allowed_domains: vec!["example.com".into()],
            outbound_enabled: true,
            max_bandwidth_bytes_per_sec: 0,
        };

        let path = write_policy_file(&policy, &tmp.path().to_path_buf()).unwrap();
        let restored = read_policy_file(&path).unwrap();

        assert_eq!(restored.task_id, policy.task_id);
        assert_eq!(restored.targets, policy.targets);
        assert_eq!(restored.allowed_domains, policy.allowed_domains);
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    let policy = EgressPolicy {
        task_id: "cli-test-task".into(),
        mode: EgressMode::Allowlist,
        targets: vec!["8.8.8.8".into(), "1.1.1.1".into()],
        allowed_domains: vec!["registry.hivemind.local".into()],
        outbound_enabled: true,
        max_bandwidth_bytes_per_sec: 10_000_000, // 10 MB/s
    };

    match validate_policy(&policy) {
        Ok(()) => {
            let rules = policy_to_rules(&policy);
            println!("Generated {} firewall rules for task {}", rules.len(), policy.task_id);
            for rule in &rules {
                println!(
                    "  {:?} {:?} target={} proto={}",
                    rule.direction, rule.action, rule.target, rule.protocol
                );
            }

            let config = SandboxEgressConfig::from(&policy);
            println!(
                "\nEgress config: {} rules, {} bytes/sec max bandwidth",
                config.firewall_rules.len(),
                config.max_bandwidth_bytes_per_sec
            );
        }
        Err(e) => {
            eprintln!("Policy validation failed: {}", e);
            std::process::exit(1);
        }
    }
}
