//! Monotonic clock on the same base as the eBPF side's `bpf_ktime_get_ns`.

pub const NANOS_PER_SEC: u64 = 1_000_000_000;
pub const NANOS_PER_MS: u64 = 1_000_000;

pub fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: ts is a valid timespec for clock_gettime to write into.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * NANOS_PER_SEC + ts.tv_nsec as u64
}
