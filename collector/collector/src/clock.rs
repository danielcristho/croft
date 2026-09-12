// A clock comparable to the eBPF side's `bpf_ktime_get_ns`.

// monotonic_ns returns the current monotonic time in nanoseconds, similar to `bpf_ktime_get_ns` in eBPF.
pub fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: ts is a valid, correctly sized out-parameter for clock_gettime.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}
