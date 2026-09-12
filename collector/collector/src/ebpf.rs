// Loads the compiled eBPF object and attaches its tracepoints.

use aya::programs::TracePoint;
use log::{debug, warn};

// sys_enter is the one raw_syscalls hook; the rest are under "syscalls".
const TRACEPOINTS: &[(&str, &str)] = &[
    ("syscalls", "sys_enter_openat"),
    ("syscalls", "sys_exit_openat"),
    ("syscalls", "sys_enter_ioctl"),
    ("syscalls", "sys_enter_close"),
    ("syscalls", "sys_enter_exit_group"),
    ("raw_syscalls", "sys_enter"),
];

// load_and_attach loads the compiled eBPF object and attaches its tracepoints.
pub fn load_and_attach() -> anyhow::Result<aya::Ebpf> {
    raise_memlock();

    let mut ebpf = aya::Ebpf::load(aya::include_bytes_aligned!(concat!(
        env!("OUT_DIR"),
        "/collector"
    )))?;
    bridge_ebpf_logger(&mut ebpf)?;

    for (category, name) in TRACEPOINTS {
        let program: &mut TracePoint = ebpf.program_mut(name).unwrap().try_into()?;
        program.load()?;
        program.attach(category, name)?;
    }

    Ok(ebpf)
}

// raise_memlock raises the RLIMIT_MEMLOCK limit to infinity, allowing the eBPF program to be loaded.
fn raise_memlock() {
    let rlim = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    // SAFETY: rlim is a valid, correctly sized rlimit for setrlimit.
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_MEMLOCK, &rlim) };
    if ret != 0 {
        debug!("remove limit on locked memory failed, ret is: {ret}");
    }
}

// bridge_ebpf_logger forwards aya_log_ebpf's own log! calls from the eBPF program into this process's logger.
fn bridge_ebpf_logger(ebpf: &mut aya::Ebpf) -> anyhow::Result<()> {
    let logger = match aya_log::EbpfLogger::init(ebpf) {
        Err(e) => {
            warn!("failed to initialize eBPF logger: {e}");
            return Ok(());
        }
        Ok(logger) => logger,
    };

    let mut logger =
        tokio::io::unix::AsyncFd::with_interest(logger, tokio::io::Interest::READABLE)?;
    tokio::task::spawn(async move {
        loop {
            let mut guard = logger.readable_mut().await.unwrap();
            guard.get_inner_mut().flush();
            guard.clear_ready();
        }
    });
    Ok(())
}
