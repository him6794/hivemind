//! Local availability preferences. This never changes or cancels running work.
use crate::idle::{accepting_tasks, IdleObservation, IdleTracker};
use crate::ResourceSample;
use anyhow::{Context, Result};
use hivemind_config::{WorkerSharingConfig, WorkerSharingMode};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
pub struct SharingStatus {
    pub success: bool,
    pub settings: WorkerSharingConfig,
    pub accepts_tasks: bool,
    pub source: &'static str,
    pub idle_for_seconds: Option<u64>,
}

struct SharingState {
    settings: WorkerSharingConfig,
    tracker: IdleTracker,
}

pub struct SharingController {
    state: Mutex<SharingState>,
    started: Instant,
    path: Option<PathBuf>,
    #[cfg(test)]
    pub(crate) observation_override: Mutex<Option<IdleObservation>>,
}

impl SharingController {
    pub fn new(settings: WorkerSharingConfig, state_root: Option<PathBuf>) -> Result<Self> {
        settings.validate()?;
        let path = state_root.map(|root| root.join("sharing-v1.json"));
        let settings = if let Some(path) = path.as_ref().filter(|path| path.exists()) {
            let metadata = std::fs::symlink_metadata(path)?;
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 4096,
                "invalid worker sharing settings file"
            );
            let saved: WorkerSharingConfig = serde_json::from_slice(&std::fs::read(path)?)
                .context("invalid saved worker sharing settings")?;
            saved.validate()?;
            saved
        } else {
            settings
        };
        Ok(Self {
            state: Mutex::new(SharingState {
                settings,
                tracker: IdleTracker::default(),
            }),
            started: Instant::now(),
            path,
            #[cfg(test)]
            observation_override: Mutex::new(None),
        })
    }

    pub fn configure(&self, settings: WorkerSharingConfig) -> Result<()> {
        settings.validate()?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(path) = &self.path {
            let parent = path
                .parent()
                .context("worker sharing settings have no parent")?;
            std::fs::create_dir_all(parent)?;
            // Save before changing live settings so a failed write cannot claim success.
            let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
            let result = (|| -> Result<()> {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)?;
                file.write_all(&serde_json::to_vec_pretty(&settings)?)?;
                file.sync_all()?;
                drop(file);
                std::fs::rename(&temporary, path)?;
                Ok(())
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&temporary);
            }
            result.context("could not save worker sharing settings")?;
        }
        state.settings = settings;
        state.tracker = IdleTracker::default();
        Ok(())
    }

    pub fn status(&self, sample: Option<&ResourceSample>) -> SharingStatus {
        let observation = platform_observation(sample);
        #[cfg(test)]
        let observation = self
            .observation_override
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .unwrap_or(observation);
        self.observe(observation, self.started.elapsed())
    }

    fn observe(&self, observation: IdleObservation, now: Duration) -> SharingStatus {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let settings = state.settings.clone();
        let source = match observation {
            IdleObservation::LastInput(_) => "last_input",
            IdleObservation::Cpu(_) => "cpu_usage",
            IdleObservation::Unavailable => "unavailable",
        };
        let idle_for = state
            .tracker
            .idle_for(observation, now, settings.cpu_percent_threshold);
        SharingStatus {
            success: true,
            accepts_tasks: accepting_tasks(
                settings.mode == WorkerSharingMode::Always,
                idle_for,
                settings.idle_seconds,
            ),
            settings,
            source,
            idle_for_seconds: idle_for.map(|value| value.as_secs()),
        }
    }
}

#[cfg(windows)]
fn platform_observation(_sample: Option<&ResourceSample>) -> IdleObservation {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut input = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // Both timestamps are milliseconds since boot modulo u32. LASTINPUTINFO is
    // initialized with its exact size; the API only writes that stack buffer.
    let idle = unsafe {
        if GetLastInputInfo(&mut input) == 0 {
            None
        } else {
            Some(Duration::from_millis(u64::from(
                GetTickCount().wrapping_sub(input.dwTime),
            )))
        }
    };
    IdleObservation::LastInput(idle)
}

#[cfg(target_os = "linux")]
fn platform_observation(sample: Option<&ResourceSample>) -> IdleObservation {
    let cpu = sample
        .filter(|sample| {
            let age = chrono::Utc::now().signed_duration_since(sample.sampled_at);
            sample.resources.cpu_usage_supported
                && age >= chrono::Duration::zero()
                && age <= chrono::Duration::seconds(30)
        })
        .map(|sample| sample.resources.cpu_usage_percent);
    IdleObservation::Cpu(cpu)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn platform_observation(_sample: Option<&ResourceSample>) -> IdleObservation {
    IdleObservation::Unavailable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_restart_and_switching_modes_resets_cpu_idle_time() {
        let root = tempfile::tempdir().unwrap();
        let controller =
            SharingController::new(WorkerSharingConfig::default(), Some(root.path().into()))
                .unwrap();
        assert!(
            controller
                .observe(IdleObservation::Unavailable, Duration::ZERO)
                .accepts_tasks
        );
        let idle = WorkerSharingConfig {
            mode: WorkerSharingMode::Idle,
            idle_seconds: 20,
            cpu_percent_threshold: 15.0,
        };
        controller.configure(idle.clone()).unwrap();
        for seconds in [0, 10, 20] {
            let status = controller.observe(
                IdleObservation::Cpu(Some(0.0)),
                Duration::from_secs(seconds),
            );
            assert_eq!(status.accepts_tasks, seconds == 20);
        }
        let restored =
            SharingController::new(WorkerSharingConfig::default(), Some(root.path().into()))
                .unwrap();
        let status = restored.observe(IdleObservation::Cpu(Some(0.0)), Duration::from_secs(50));
        assert_eq!(status.settings, idle);
        assert!(!status.accepts_tasks);
        controller
            .configure(WorkerSharingConfig::default())
            .unwrap();
        assert!(
            controller
                .observe(IdleObservation::Unavailable, Duration::ZERO)
                .accepts_tasks
        );
    }

    #[test]
    fn invalid_settings_or_failed_save_do_not_change_live_mode() {
        let root = tempfile::tempdir().unwrap();
        let controller =
            SharingController::new(WorkerSharingConfig::default(), Some(root.path().into()))
                .unwrap();
        let invalid = WorkerSharingConfig {
            idle_seconds: 0,
            ..WorkerSharingConfig::default()
        };
        assert!(controller.configure(invalid).is_err());
        std::fs::create_dir(root.path().join("sharing-v1.json")).unwrap();
        assert!(controller
            .configure(WorkerSharingConfig {
                mode: WorkerSharingMode::Idle,
                ..WorkerSharingConfig::default()
            })
            .is_err());
        assert!(
            controller
                .observe(IdleObservation::Unavailable, Duration::ZERO)
                .accepts_tasks
        );
    }
}
