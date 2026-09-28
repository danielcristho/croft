//! Classifies a GPU-holding process by how recently it sent an ioctl to the device.

use collector_common::GpuActivity;

use crate::clock::NANOS_PER_SEC;

const RECENT_WINDOW_NS: u64 = 5 * NANOS_PER_SEC;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Workload {
    GpuOnset,
    GpuHeld,
}

impl Workload {
    pub fn label(self) -> &'static str {
        match self {
            Workload::GpuOnset => "gpu-onset",
            Workload::GpuHeld => "gpu-held",
        }
    }
}

pub fn classify(activity: &GpuActivity, now_ns: u64) -> Workload {
    if now_ns.saturating_sub(activity.last_ns) <= RECENT_WINDOW_NS {
        return Workload::GpuOnset;
    }
    Workload::GpuHeld
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTIVITY: GpuActivity = GpuActivity {
        count: 1,
        last_ns: 1_000,
    };

    #[test]
    fn recent_activity_is_onset() {
        let now = ACTIVITY.last_ns + RECENT_WINDOW_NS;
        assert_eq!(classify(&ACTIVITY, now), Workload::GpuOnset);
    }

    #[test]
    fn stale_activity_is_held() {
        let now = ACTIVITY.last_ns + RECENT_WINDOW_NS + 1;
        assert_eq!(classify(&ACTIVITY, now), Workload::GpuHeld);
    }
}
