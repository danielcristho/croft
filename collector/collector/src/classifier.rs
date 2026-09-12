// Classifies a process's GPU activity as either onset or idle, based on the last time it drove the GPU.

use collector_common::GpuActivity;

// Consts
const RECENT_WINDOW_NS: u64 = 5_000_000_000;

pub enum Workload {
    GpuOnset,
    Idle,
}

// classify returns the workload classification for a process based on its GPU activity and the current time.
pub fn classify(activity: &GpuActivity, now_ns: u64) -> Workload {
    if now_ns.saturating_sub(activity.last_ns) <= RECENT_WINDOW_NS {
        Workload::GpuOnset
    } else {
        Workload::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // recent_activity_is_onset tests that classify returns GpuOnset for recent activity.
    // stale_activity_is_idle tests that classify returns Idle for stale activity.
    #[test]
    fn recent_activity_is_onset() {
        let activity = GpuActivity {
            count: 1,
            last_ns: 1_000,
        };
        assert!(matches!(
            classify(&activity, 1_000 + RECENT_WINDOW_NS),
            Workload::GpuOnset
        ));
    }

    #[test]
    fn stale_activity_is_idle() {
        let activity = GpuActivity {
            count: 1,
            last_ns: 1_000,
        };
        assert!(matches!(
            classify(&activity, 1_000 + RECENT_WINDOW_NS + 1),
            Workload::Idle
        ));
    }
}
