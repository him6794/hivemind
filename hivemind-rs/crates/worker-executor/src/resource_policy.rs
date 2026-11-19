//! Worker host-protection & resource sharing policy engine (Milestone 3.2 #1-#4).
//!
//! Governs:
//! - Sharing schedule (`Always`, `IdleOnly`, `TimeWindow`, `AcPowerOnly`)
//! - Hard resource caps (CPU %, VRAM %, disk GB, upload/download bandwidth Mbps)
//! - Foreground conflict detection (full-screen game/video, external GPU pressure)
//! - Thermal & battery safety throttling / automatic pause
//! - Auto-recommended vs manually pinned model hosting

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SharingSchedule {
    Always,
    IdleOnly { min_idle_secs: u64 },
    TimeWindow { start_hour: u8, end_hour: u8 },
    AcPowerOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceProtectionPolicy {
    pub schedule: SharingSchedule,
    pub max_cpu_percent: u8,
    pub max_vram_percent: u8,
    pub max_disk_gb: u64,
    pub max_upload_mbps: u32,
    pub max_download_mbps: u32,
    pub yield_on_fullscreen: bool,
    pub yield_external_gpu_util_pct: u8,
    pub thermal_throttle_cpu_c: f32,
    pub thermal_pause_cpu_c: f32,
    pub thermal_throttle_gpu_c: f32,
    pub thermal_pause_gpu_c: f32,
    pub max_battery_temp_c: f32,
    pub min_battery_pct: u8,
    pub require_ac_power: bool,
}

impl Default for ResourceProtectionPolicy {
    fn default() -> Self {
        Self {
            schedule: SharingSchedule::IdleOnly { min_idle_secs: 300 },
            max_cpu_percent: 70,
            max_vram_percent: 80,
            max_disk_gb: 30,
            max_upload_mbps: 100,
            max_download_mbps: 200,
            yield_on_fullscreen: true,
            yield_external_gpu_util_pct: 65,
            thermal_throttle_cpu_c: 80.0,
            thermal_pause_cpu_c: 90.0,
            thermal_throttle_gpu_c: 78.0,
            thermal_pause_gpu_c: 86.0,
            max_battery_temp_c: 40.0,
            min_battery_pct: 35,
            require_ac_power: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostTelemetrySample {
    pub idle_secs: u64,
    pub local_hour: u8,
    pub on_ac_power: bool,
    pub battery_pct: Option<u8>,
    pub fullscreen_foreground: bool,
    pub external_gpu_util_pct: u8,
    pub cpu_temp_c: Option<f32>,
    pub gpu_temp_c: Option<f32>,
    pub battery_temp_c: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SharingDecision {
    Allow {
        effective_cpu_percent: u8,
        effective_vram_percent: u8,
    },
    Throttle {
        effective_cpu_percent: u8,
        effective_vram_percent: u8,
        reason: String,
    },
    Pause {
        reason: String,
    },
}

#[must_use]
pub fn evaluate_sharing_decision(
    policy: &ResourceProtectionPolicy,
    sample: &HostTelemetrySample,
) -> SharingDecision {
    // 1. Power & battery safety gates.
    if (policy.require_ac_power || matches!(policy.schedule, SharingSchedule::AcPowerOnly))
        && !sample.on_ac_power
    {
        return SharingDecision::Pause {
            reason: "on_battery_power".to_string(),
        };
    }
    if let Some(pct) = sample.battery_pct {
        if pct < policy.min_battery_pct && !sample.on_ac_power {
            return SharingDecision::Pause {
                reason: format!("battery_below_minimum_{pct}%"),
            };
        }
    }
    if let Some(bt) = sample.battery_temp_c {
        if bt >= policy.max_battery_temp_c {
            return SharingDecision::Pause {
                reason: format!("battery_overtemp_{bt:.1}C"),
            };
        }
    }

    // 2. Thermal hard-pause checks.
    if let Some(cpu_c) = sample.cpu_temp_c {
        if cpu_c >= policy.thermal_pause_cpu_c {
            return SharingDecision::Pause {
                reason: format!("cpu_overtemp_{cpu_c:.1}C"),
            };
        }
    }
    if let Some(gpu_c) = sample.gpu_temp_c {
        if gpu_c >= policy.thermal_pause_gpu_c {
            return SharingDecision::Pause {
                reason: format!("gpu_overtemp_{gpu_c:.1}C"),
            };
        }
    }

    // 3. Foreground user conflict yield (games / video / high external GPU usage).
    if policy.yield_on_fullscreen && sample.fullscreen_foreground {
        return SharingDecision::Pause {
            reason: "fullscreen_foreground_active".to_string(),
        };
    }
    if sample.external_gpu_util_pct >= policy.yield_external_gpu_util_pct {
        return SharingDecision::Pause {
            reason: format!("external_gpu_busy_{}%", sample.external_gpu_util_pct),
        };
    }

    // 4. Schedule checks.
    match policy.schedule {
        SharingSchedule::Always | SharingSchedule::AcPowerOnly => {}
        SharingSchedule::IdleOnly { min_idle_secs } => {
            if sample.idle_secs < min_idle_secs {
                return SharingDecision::Pause {
                    reason: format!(
                        "user_active_idle_{}s_lt_{}s",
                        sample.idle_secs, min_idle_secs
                    ),
                };
            }
        }
        SharingSchedule::TimeWindow {
            start_hour,
            end_hour,
        } => {
            let in_window = if start_hour <= end_hour {
                sample.local_hour >= start_hour && sample.local_hour < end_hour
            } else {
                // Overnight window, e.g., 22 -> 07
                sample.local_hour >= start_hour || sample.local_hour < end_hour
            };
            if !in_window {
                return SharingDecision::Pause {
                    reason: format!("outside_schedule_window_{start_hour:02}h_{end_hour:02}h"),
                };
            }
        }
    }

    // 5. Thermal soft-throttle checks.
    let cpu_warm = sample
        .cpu_temp_c
        .is_some_and(|c| c >= policy.thermal_throttle_cpu_c);
    let gpu_warm = sample
        .gpu_temp_c
        .is_some_and(|c| c >= policy.thermal_throttle_gpu_c);
    if cpu_warm || gpu_warm {
        let throttled_cpu = (policy.max_cpu_percent / 2).max(15);
        let throttled_vram = (policy.max_vram_percent * 3 / 4).max(25);
        return SharingDecision::Throttle {
            effective_cpu_percent: throttled_cpu,
            effective_vram_percent: throttled_vram,
            reason: if gpu_warm {
                "gpu_thermal_throttle".to_string()
            } else {
                "cpu_thermal_throttle".to_string()
            },
        };
    }

    SharingDecision::Allow {
        effective_cpu_percent: policy.max_cpu_percent.clamp(10, 100),
        effective_vram_percent: policy.max_vram_percent.clamp(10, 100),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCandidate {
    pub model_id: String,
    pub size_mb: u64,
    pub min_vram_mb: u64,
    pub demand_score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ModelHostingPolicy {
    AutoRecommended,
    ManualPinned { pinned_model_ids: Vec<String> },
}

/// Select which models the Worker should download & host under disk and VRAM budgets.
#[must_use]
pub fn select_models_to_host(
    policy: &ModelHostingPolicy,
    catalog: &[ModelCandidate],
    available_vram_mb: u64,
    max_disk_mb: u64,
) -> Vec<String> {
    let mut selected = Vec::new();
    let mut used_disk_mb = 0u64;

    match policy {
        ModelHostingPolicy::ManualPinned { pinned_model_ids } => {
            for id in pinned_model_ids {
                if let Some(entry) = catalog.iter().find(|c| &c.model_id == id) {
                    if entry.min_vram_mb <= available_vram_mb
                        && used_disk_mb + entry.size_mb <= max_disk_mb
                    {
                        used_disk_mb += entry.size_mb;
                        selected.push(entry.model_id.clone());
                    }
                }
            }
        }
        ModelHostingPolicy::AutoRecommended => {
            let mut eligible: Vec<&ModelCandidate> = catalog
                .iter()
                .filter(|c| c.min_vram_mb <= available_vram_mb && c.size_mb <= max_disk_mb)
                .collect();
            eligible.sort_by(|a, b| {
                b.demand_score
                    .cmp(&a.demand_score)
                    .then_with(|| a.size_mb.cmp(&b.size_mb))
            });
            for entry in eligible {
                if used_disk_mb + entry.size_mb <= max_disk_mb {
                    used_disk_mb += entry.size_mb;
                    selected.push(entry.model_id.clone());
                }
            }
        }
    }

    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nominal_sample() -> HostTelemetrySample {
        HostTelemetrySample {
            idle_secs: 600,
            local_hour: 23,
            on_ac_power: true,
            battery_pct: Some(100),
            fullscreen_foreground: false,
            external_gpu_util_pct: 5,
            cpu_temp_c: Some(55.0),
            gpu_temp_c: Some(50.0),
            battery_temp_c: Some(30.0),
        }
    }

    #[test]
    fn fullscreen_and_high_external_gpu_yield_immediately() {
        let policy = ResourceProtectionPolicy::default();
        let mut s = nominal_sample();
        s.fullscreen_foreground = true;
        assert!(matches!(
            evaluate_sharing_decision(&policy, &s),
            SharingDecision::Pause { reason } if reason == "fullscreen_foreground_active"
        ));

        let mut s2 = nominal_sample();
        s2.external_gpu_util_pct = 80;
        assert!(matches!(
            evaluate_sharing_decision(&policy, &s2),
            SharingDecision::Pause { reason } if reason.starts_with("external_gpu_busy")
        ));
    }

    #[test]
    fn thermal_throttle_and_hard_pause_protect_hardware() {
        let policy = ResourceProtectionPolicy::default();
        let mut warm = nominal_sample();
        warm.gpu_temp_c = Some(80.0);
        assert!(matches!(
            evaluate_sharing_decision(&policy, &warm),
            SharingDecision::Throttle {
                effective_cpu_percent: 35,
                ..
            }
        ));

        let mut hot = nominal_sample();
        hot.gpu_temp_c = Some(88.0);
        assert!(matches!(
            evaluate_sharing_decision(&policy, &hot),
            SharingDecision::Pause { reason } if reason.starts_with("gpu_overtemp")
        ));
    }

    #[test]
    fn overnight_time_window_schedule_works() {
        let policy = ResourceProtectionPolicy {
            schedule: SharingSchedule::TimeWindow {
                start_hour: 22,
                end_hour: 7,
            },
            ..Default::default()
        };
        let mut late = nominal_sample();
        late.local_hour = 2;
        assert!(matches!(
            evaluate_sharing_decision(&policy, &late),
            SharingDecision::Allow { .. }
        ));

        let mut noon = nominal_sample();
        noon.local_hour = 13;
        assert!(matches!(
            evaluate_sharing_decision(&policy, &noon),
            SharingDecision::Pause { reason } if reason.starts_with("outside_schedule_window")
        ));
    }

    #[test]
    fn auto_and_manual_model_hosting_respect_vram_and_disk_caps() {
        let catalog = vec![
            ModelCandidate {
                model_id: "qwen-7b".to_string(),
                size_mb: 4500,
                min_vram_mb: 6000,
                demand_score: 90,
            },
            ModelCandidate {
                model_id: "qwen-3b".to_string(),
                size_mb: 2000,
                min_vram_mb: 3000,
                demand_score: 80,
            },
            ModelCandidate {
                model_id: "llama-70b".to_string(),
                size_mb: 40000,
                min_vram_mb: 48000,
                demand_score: 99,
            },
        ];

        let auto =
            select_models_to_host(&ModelHostingPolicy::AutoRecommended, &catalog, 8000, 7000);
        assert_eq!(auto, vec!["qwen-7b".to_string(), "qwen-3b".to_string()]);

        let manual = select_models_to_host(
            &ModelHostingPolicy::ManualPinned {
                pinned_model_ids: vec!["qwen-3b".to_string(), "llama-70b".to_string()],
            },
            &catalog,
            8000,
            7000,
        );
        assert_eq!(manual, vec!["qwen-3b".to_string()]);
    }
}
