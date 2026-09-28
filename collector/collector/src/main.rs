//! Entry point for the croft collector.

mod ansi;
mod backfill;
mod banner;
mod classifier;
mod clock;
mod dataset;
mod drops;
mod ebpf;
mod logging;
mod maps;
mod process;
mod reporter;
mod syscalls;

use log::info;
use tokio::signal;

use crate::dataset::Dataset;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init();
    banner::print();

    let mut ebpf = ebpf::load_and_attach()?;
    backfill::scan_gpu_fds(&mut ebpf)?;
    tokio::task::spawn(reporter::run(ebpf, Dataset::from_env()));

    info!("collector running, Ctrl-C to stop");
    signal::ctrl_c().await?;
    info!("exiting");
    Ok(())
}
