// Main entry point for the collector binary.

mod backfill;
mod banner;
mod classifier;
mod clock;
mod dataset;
mod ebpf;
mod logging;
mod reporter;
mod syscalls;

use tokio::signal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init();
    banner::print();

    let ebpf = ebpf::load_and_attach()?;
    tokio::task::spawn(reporter::run(ebpf));

    let ctrl_c = signal::ctrl_c();
    println!("Waiting for Ctrl-C...");
    ctrl_c.await?;
    println!("Exiting...");

    Ok(())
}
