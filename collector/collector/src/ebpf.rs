//! Loads the compiled eBPF object and attaches its tracepoints.

use anyhow::Context as _;
use aya::{Ebpf, programs::TracePoint};
use log::warn;

// Consts for the tracepoints to attach.
const TRACEPOINTS: &[(&str, &str)] = &[
    ("syscalls", "sys_enter_openat"),
    ("syscalls", "sys_exit_openat"),
    ("syscalls", "sys_enter_ioctl"),
    ("syscalls", "sys_enter_close"),
    ("sched", "sched_process_exit"),
    ("raw_syscalls", "sys_enter"),
];

// load_and_attach loads the eBPF object from the compiled bytes, raises RLIMIT_MEMLOCK, and attaches the tracepoints.
pub fn load_and_attach() -> anyhow::Result<Ebpf> {
    raise_memlock();
    let mut ebpf = Ebpf::load(aya::include_bytes_aligned!(concat!(
        env!("OUT_DIR"),
        "/collector"
    )))?;
    bridge_ebpf_logger(&mut ebpf)?;
    for &(category, name) in TRACEPOINTS {
        attach(&mut ebpf, category, name)?;
    }
    Ok(ebpf)
}

// attach finds the tracepoint program with the given name in the eBPF object, loads it, and attaches it to the given category and name.
fn attach(ebpf: &mut Ebpf, category: &str, name: &str) -> anyhow::Result<()> {
    let program: &mut TracePoint = ebpf
        .program_mut(name)
        .with_context(|| format!("program {name} not in the eBPF object"))?
        .try_into()?;
    program.load()?;
    program
        .attach(category, name)
        .with_context(|| format!("attach {category}/{name}"))?;
    Ok(())
}

// raise_memlock lifts RLIMIT_MEMLOCK, which kernels before 5.11 charge eBPF maps against.
fn raise_memlock() {
    let rlim = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    // SAFETY: rl// Warning: this will be printed on shutdown, but also if the eBPF program panics and the log stream closes.im is a valid rlimit for setrlimit to read.
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_MEMLOCK, &rlim) };
    if ret != 0 {
        // WARNING: this is not fatal, because kernels 5.11+ do not charge eBPF maps against RLIMIT_MEMLOCK,
        // but it may cause the eBPF program to fail to load on older kernels.
        warn!("raise RLIMIT_MEMLOCK: setrlimit returned {ret}");
    }
}

// bridge_ebpf_logger forwards aya_log_ebpf log, calls from the eBPF program into this process's logger.
fn bridge_ebpf_logger(ebpf: &mut Ebpf) -> anyhow::Result<()> {
    let logger = match aya_log::EbpfLogger::init(ebpf) {
        Ok(logger) => logger,
        Err(e) => {
            warn!("eBPF logger unavailable: {e}");
            return Ok(());
        }
    };
    let mut logger =
        tokio::io::unix::AsyncFd::with_interest(logger, tokio::io::Interest::READABLE)?;
    tokio::task::spawn(async move {
        while let Ok(mut guard) = logger.readable_mut().await {
            guard.get_inner_mut().flush();
            guard.clear_ready();
        }
        // Warning: this will be printed on shutdown,
        // but also if the eBPF program panics and the log stream closes.
        warn!("eBPF log stream closed");
    });
    Ok(())
}
