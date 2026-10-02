//! Entry point for the croft collector.

mod ansi;
mod backfill;
mod banner;
mod classifier;
mod clock;
mod dataset;
mod delta;
mod drops;
mod ebpf;
mod logging;
mod maps;
mod namespace;
mod process;
mod reporter;
mod resources;
mod syscalls;
mod watch;

use log::info;
use tokio::signal;

use crate::{dataset::Dataset, watch::CgroupWatch};

fn main() -> anyhow::Result<()> {
    logging::init();
    banner::print();
    namespace::join_host_cgroup();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let mut ebpf = ebpf::load_and_attach()?;
    backfill::scan_gpu_fds(&mut ebpf)?;
    let watch = CgroupWatch::from_env();
    if !watch.is_empty() {
        info!(
            "recording every process in cgroups matching {:?}",
            watch.patterns()
        );
    }
    tokio::task::spawn(reporter::run(ebpf, Dataset::from_env(), watch));

    info!("collector running, Ctrl-C to stop");
    signal::ctrl_c().await?;
    info!("exiting");
    Ok(())
}
