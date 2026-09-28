#![cfg_attr(not(test), no_std)]

//! Types and helpers shared across the kernel/userspace boundary.

pub const GPU_PATH_PREFIXES: &[&[u8]] = &[b"/dev/nvidia", b"/dev/dri/"];

// PRESENT is the value of maps used as sets.
pub const PRESENT: u8 = 1;

// Each name must match its `#[map]` static in collector-ebpf.
pub mod map_names {
    pub const PENDING_GPU_OPEN: &str = "PENDING_GPU_OPEN";
    pub const GPU_FDS: &str = "GPU_FDS";
    pub const GPU_ACTIVITY: &str = "GPU_ACTIVITY";
    pub const SYSCALL_BY_TYPE: &str = "SYSCALL_BY_TYPE";
    pub const DROPS: &str = "DROPS";
}

pub fn is_gpu_path(path: &[u8]) -> bool {
    GPU_PATH_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

// pack_key packs a tgid and a per-tgid id, an fd or a syscall number, into one map key.
pub fn pack_key(tgid: u32, id: u32) -> u64 {
    ((tgid as u64) << 32) | id as u64
}

pub fn unpack_key(key: u64) -> (u32, u32) {
    ((key >> 32) as u32, key as u32)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuActivity {
    pub count: u64,
    pub last_ns: u64,
}

impl GpuActivity {
    pub fn record(prev: Option<&GpuActivity>, now_ns: u64) -> GpuActivity {
        GpuActivity {
            count: prev.map_or(0, |prev| prev.count) + 1,
            last_ns: now_ns,
        }
    }
}

#[cfg(feature = "user")]
// SAFETY: repr(C), two u64 fields, no padding or invalid bit patterns.
unsafe impl aya::Pod for GpuActivity {}

// DropSite names a map insert the eBPF side can lose; each has a slot in DROPS.
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum DropSite {
    PendingOpen,
    GpuFd,
    GpuActivity,
    SyscallCount,
}

impl DropSite {
    pub const ALL: [DropSite; 4] = [
        DropSite::PendingOpen,
        DropSite::GpuFd,
        DropSite::GpuActivity,
        DropSite::SyscallCount,
    ];

    pub const fn index(self) -> u32 {
        self as u32
    }

    pub const fn map_name(self) -> &'static str {
        match self {
            DropSite::PendingOpen => map_names::PENDING_GPU_OPEN,
            DropSite::GpuFd => map_names::GPU_FDS,
            DropSite::GpuActivity => map_names::GPU_ACTIVITY,
            DropSite::SyscallCount => map_names::SYSCALL_BY_TYPE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpack_key_reverses_pack_key() {
        assert_eq!(unpack_key(pack_key(7, 16)), (7, 16));
        assert_eq!(
            unpack_key(pack_key(u32::MAX, u32::MAX)),
            (u32::MAX, u32::MAX)
        );
    }

    #[test]
    fn gpu_paths_match_by_prefix() {
        let cases: &[(&[u8], bool)] = &[
            (b"/dev/nvidia0", true),
            (b"/dev/nvidiactl", true),
            (b"/dev/dri/renderD128", true),
            (b"/dev/null", false),
            (b"nvidia0", false),
        ];
        for (path, want) in cases {
            assert_eq!(is_gpu_path(path), *want, "{path:?}");
        }
    }

    #[test]
    fn first_record_starts_the_count() {
        let activity = GpuActivity::record(None, 10);
        assert_eq!(
            activity,
            GpuActivity {
                count: 1,
                last_ns: 10
            }
        );
    }

    #[test]
    fn next_record_bumps_count_and_time() {
        let prev = GpuActivity {
            count: 4,
            last_ns: 10,
        };
        let activity = GpuActivity::record(Some(&prev), 20);
        assert_eq!(
            activity,
            GpuActivity {
                count: 5,
                last_ns: 20
            }
        );
    }

    #[test]
    fn drop_site_indexes_match_their_position() {
        for (i, site) in DropSite::ALL.iter().enumerate() {
            assert_eq!(site.index() as usize, i);
        }
    }
}
