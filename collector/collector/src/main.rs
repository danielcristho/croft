//! Entry point for the croft binary.

mod ansi;
mod backfill;
mod banner;
mod classifier;
mod cli;
mod clock;
mod command;
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
mod rows;
mod syscalls;
mod watch;

use clap::Parser;

use crate::cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    logging::init();
    match cli.command {
        Command::Run(args) => command::run::execute(&args),
        Command::Top(args) => command::top::execute(&args),
        Command::Stats(args) => command::stats::execute(&args),
        Command::Deploy(args) => command::deploy::execute(&args),
        Command::Collect(args) => command::collect::execute(&args),
    }
}
