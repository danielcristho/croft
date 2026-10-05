//! The collector daemon: loads the eBPF programs and reports each poll to the dataset.

use anyhow::Context;
use log::info;

use crate::{
    backfill, banner, cli::RunArgs, dataset::Dataset, ebpf, namespace, reporter, signals,
    watch::CgroupWatch,
};

// execute runs the collector daemon, loading the eBPF programs and reporting each poll to the dataset.
pub fn execute(args: &RunArgs) -> anyhow::Result<()> {
    banner::print();
    namespace::join_host_cgroup();
    let path = &args.dataset.dataset;
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let dataset = Dataset::new(path);
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
    info!(
        "config version={} dataset={} watch={} poll={}s",
        env!("CARGO_PKG_VERSION"),
        dataset.path().display(),
        if watch.is_empty() {
            "-".to_string()
        } else {
            watch.patterns().join(",")
        },
        reporter::POLL_INTERVAL.as_secs()
    );
    tokio::task::spawn(reporter::run(ebpf, dataset, watch));

    info!("collector running, Ctrl-C to stop");
    let received = signals::shutdown().await?;
    info!("exiting signal={received}");
    Ok(())
}
