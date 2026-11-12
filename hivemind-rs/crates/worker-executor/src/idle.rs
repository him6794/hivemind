//! Pure idle-state transitions. Sampling and persistence live in `sharing`.
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum IdleObservation {
    LastInput(Option<Duration>),
    Cpu(Option<f64>),
    Unavailable,
}

#[derive(Default)]
pub struct IdleTracker {
    low_cpu_since: Option<Duration>,
    last_cpu_check: Option<Duration>,
}

impl IdleTracker {
    pub fn idle_for(
        &mut self,
        observation: IdleObservation,
        now: Duration,
        cpu_threshold: f64,
    ) -> Option<Duration> {
        match observation {
            IdleObservation::LastInput(duration) => {
                *self = Self::default();
                duration
            }
            IdleObservation::Cpu(Some(percent))
                if percent.is_finite() && (0.0..=100.0).contains(&percent) =>
            {
                if self.last_cpu_check.is_none_or(|previous| {
                    now < previous || now.saturating_sub(previous) > Duration::from_secs(30)
                }) {
                    self.low_cpu_since = None;
                }
                self.last_cpu_check = Some(now);
                if percent < cpu_threshold {
                    let started = *self.low_cpu_since.get_or_insert(now);
                    Some(now.saturating_sub(started))
                } else {
                    self.low_cpu_since = None;
                    Some(Duration::ZERO)
                }
            }
            _ => {
                *self = Self::default();
                None
            }
        }
    }
}

pub fn accepting_tasks(always: bool, idle_for: Option<Duration>, idle_seconds: u64) -> bool {
    always || idle_for.is_some_and(|duration| duration >= Duration::from_secs(idle_seconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seconds(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    #[test]
    fn last_input_uses_the_exact_threshold_and_new_input_pauses_admission() {
        let mut tracker = IdleTracker::default();
        assert!(!accepting_tasks(
            false,
            tracker.idle_for(
                IdleObservation::LastInput(Some(seconds(59))),
                seconds(0),
                20.0
            ),
            60
        ));
        assert!(accepting_tasks(
            false,
            tracker.idle_for(
                IdleObservation::LastInput(Some(seconds(60))),
                seconds(1),
                20.0
            ),
            60
        ));
        assert!(!accepting_tasks(
            false,
            tracker.idle_for(
                IdleObservation::LastInput(Some(seconds(0))),
                seconds(2),
                20.0
            ),
            60
        ));
    }

    #[test]
    fn cpu_must_remain_low_for_the_whole_idle_period() {
        let mut tracker = IdleTracker::default();
        for now in 0..=6 {
            let idle = tracker.idle_for(IdleObservation::Cpu(Some(10.0)), seconds(now * 10), 20.0);
            assert_eq!(accepting_tasks(false, idle, 60), now == 6);
        }
        assert_eq!(
            tracker.idle_for(IdleObservation::Cpu(Some(20.0)), seconds(61), 20.0),
            Some(Duration::ZERO)
        );
        assert_eq!(
            tracker.idle_for(IdleObservation::Cpu(Some(0.0)), seconds(70), 20.0),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn unavailable_invalid_or_stale_cpu_never_counts_as_idle() {
        for bad in [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(-1.0),
            Some(101.0),
        ] {
            let mut tracker = IdleTracker::default();
            tracker.idle_for(IdleObservation::Cpu(Some(0.0)), seconds(0), 20.0);
            assert_eq!(
                tracker.idle_for(IdleObservation::Cpu(bad), seconds(10), 20.0),
                None
            );
            assert_eq!(
                tracker.idle_for(IdleObservation::Cpu(Some(0.0)), seconds(20), 20.0),
                Some(Duration::ZERO)
            );
        }
        let mut tracker = IdleTracker::default();
        tracker.idle_for(IdleObservation::Cpu(Some(0.0)), seconds(0), 20.0);
        assert_eq!(
            tracker.idle_for(IdleObservation::Cpu(Some(0.0)), seconds(31), 20.0),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn always_mode_does_not_depend_on_an_idle_sensor() {
        assert!(accepting_tasks(true, None, 300));
        assert!(!accepting_tasks(false, None, 300));
        assert_eq!(
            IdleTracker::default().idle_for(IdleObservation::Unavailable, seconds(0), 20.0),
            None
        );
    }
}
