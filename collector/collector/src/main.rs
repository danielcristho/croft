use std::time::Duration;

use aya::{maps::HashMap, programs::TracePoint};
use collector_common::GpuActivity;
#[rustfmt::skip]
use log::{debug, info, warn};
use tokio::signal;

// Consts
const TRACEPOINTS: &[&str] = &[
    "sys_enter_openat",
    "sys_exit_openat",
    "sys_enter_ioctl",
    "sys_enter_close",
    "sys_enter_exit_group",
];

// Main
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let rlim = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_MEMLOCK, &rlim) };
    if ret != 0 {
        debug!("remove limit on locked memory failed, ret is: {ret}");
    }

    let mut ebpf = aya::Ebpf::load(aya::include_bytes_aligned!(concat!(
        env!("OUT_DIR"),
        "/collector"
    )))?;
    match aya_log::EbpfLogger::init(&mut ebpf) {
        Err(e) => {
            warn!("failed to initialize eBPF logger: {e}");
        }
        Ok(logger) => {
            let mut logger =
                tokio::io::unix::AsyncFd::with_interest(logger, tokio::io::Interest::READABLE)?;
            tokio::task::spawn(async move {
                loop {
                    let mut guard = logger.readable_mut().await.unwrap();
                    guard.get_inner_mut().flush();
                    guard.clear_ready();
                }
            });
        }
    }

    for name in TRACEPOINTS {
        let program: &mut TracePoint = ebpf.program_mut(name).unwrap().try_into()?;
        program.load()?;
        program.attach("syscalls", name)?;
    }

    tokio::task::spawn(report_gpu_activity(ebpf));

    let ctrl_c = signal::ctrl_c();
    println!("Waiting for Ctrl-C...");
    ctrl_c.await?;
    println!("Exiting...");

    Ok(())
}

// report_gpu_activity periodically reads the GPU_ACTIVITY map and logs the activity for each process.
async fn report_gpu_activity(ebpf: aya::Ebpf) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    loop {
        interval.tick().await;
        let Ok(map) = HashMap::<_, u32, GpuActivity>::try_from(ebpf.map("GPU_ACTIVITY").unwrap())
        else {
            continue;
        };
        for (tgid, activity) in map.iter().flatten() {
            info!(
                "gpu activity: tgid={tgid} count={} last_ns={}",
                activity.count, activity.last_ns
            );
        }
    }
}
