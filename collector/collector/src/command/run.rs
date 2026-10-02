//! The collector daemon: loads the eBPF programs and reports each poll to the dataset.

use log::info;
use tokio::signal;

use crate::{
    backfill, banner, cli::RunArgs, dataset::Dataset, ebpf, namespace, reporter, watch::CgroupWatch,
};

pub fn execute(args: &RunArgs) -> anyhow::Result<()> {
    banner::print();
    namespace::join_host_cgroup();
    let dataset = Dataset::new(&args.dataset.dataset);
    let watch = CgroupWatch::new(&args.watch);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(collect(dataset, watch))
}

// Run the collector, polling eBPF maps and reporting to the dataset.
async fn collect(dataset: Dataset, watch: CgroupWatch) -> anyhow::Result<()> {
    let mut ebpf = ebpf::load_and_attach()?;
    backfill::scan_gpu_fds(&mut ebpf)?;
    if !watch.is_empty() {
        info!(
            "recording every process in cgroups matching {:?}",
            watch.patterns()
        );
    }
    tokio::task::spawn(reporter::run(ebpf, dataset, watch));

    info!("collector running, Ctrl-C to stop");
    signal::ctrl_c().await?;
    info!("exiting");
    Ok(())
}
