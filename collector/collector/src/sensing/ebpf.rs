//! Loads the compiled eBPF object and attaches its tracepoints.

use anyhow::Context as _;
use aya::{
    Ebpf,
    programs::{KProbe, TracePoint},
};
use log::warn;

// Consts for the tracepoints to attach.
const TRACEPOINTS: &[(&str, &str)] = &[
    ("syscalls", "sys_enter_openat"),
    ("syscalls", "sys_exit_openat"),
    ("syscalls", "sys_enter_ioctl"),
    ("syscalls", "sys_enter_close"),
    ("sched", "sched_process_exit"),
    ("sched", "sched_wakeup"),
    ("sched", "sched_wakeup_new"),
    ("sched", "sched_switch"),
    ("raw_syscalls", "sys_enter"),
    ("raw_syscalls", "sys_exit"),
    ("syscalls", "sys_enter_connect"),
];

// Kernel functions behind reads and writes on a socket. They are not a stable interface, so losing one costs only its count.
const SOCKET_KPROBES: &[&str] = &["sock_read_iter", "sock_write_iter"];

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
    for &name in SOCKET_KPROBES {
        if let Err(e) = attach_kprobe(&mut ebpf, name) {
            warn!("no socket counts from {name}: {e:#}");
        }
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

// attach_kprobe finds the kprobe program named after a kernel function, loads it, and attaches it to that function.
fn attach_kprobe(ebpf: &mut Ebpf, name: &str) -> anyhow::Result<()> {
    let program: &mut KProbe = ebpf
        .program_mut(name)
        .with_context(|| format!("program {name} not in the eBPF object"))?
        .try_into()?;
    program.load()?;
    program
        .attach(name, 0)
        .with_context(|| format!("attach kprobe {name}"))?;
    Ok(())
}

// raise_memlock lifts RLIMIT_MEMLOCK, which kernels before 5.11 charge eBPF maps against.
fn raise_memlock() {
    let rlim = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_MEMLOCK, &rlim) };
    if ret != 0 {
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
    // SAFETY: the logger owns its ring buffer fd, which stays open while the AsyncFd owns the logger.
    let mut logger = unsafe {
        tokio::io::unix::AsyncFd::register_with_interest(logger, tokio::io::Interest::READABLE)
    }
    .map_err(|e| e.into_parts().1)?;
    tokio::task::spawn(async move {
        while let Ok(mut guard) = logger.readable_mut().await {
            guard.get_inner_mut().flush();
            guard.clear_ready();
        }
        warn!("eBPF log stream closed");
    });
    Ok(())
}
