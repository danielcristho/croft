//! Entry point for the croft binary.

mod cli;
mod command;
mod decision;
mod inspect;
mod pipeline;
mod runtime;
mod sensing;

use clap::Parser;

use crate::{
    cli::{Cli, Command},
    runtime::{logging, signals},
};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    logging::init();
    if !matches!(cli.command, Command::Run(_)) {
        signals::exit_on_interrupt();
    }
    match cli.command {
        Command::Run(args) => command::run::execute(&args),
        Command::Top(args) => command::top::execute(&args),
        Command::Stats(args) => command::stats::execute(&args),
        Command::Profile(args) => command::profile::execute(&args),
        Command::Deploy(args) => command::deploy::execute(&args),
        Command::Collect(args) => command::collect::execute(&args),
        Command::Scale(args) => command::scale::execute(&args),
    }
}
