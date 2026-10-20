//! Command line of the croft binary.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::{inspect::framework, pipeline::dataset, sensing::watch};

#[derive(Parser)]
#[command(
    name = "croft",
    version,
    about = "Workload-aware autoscaling: eBPF collector and dataset tools"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Run(RunArgs),
    Top(TopArgs),
    Stats(StatsArgs),
    Profile(ProfileArgs),
    Deploy(DeployArgs),
    Collect(CollectArgs),
    Scale(ScaleArgs),
}

#[derive(Args)]
pub struct DatasetArg {
    /// Dataset file, JSON Lines
    #[arg(long, env = dataset::PATH_ENV, default_value = dataset::DEFAULT_PATH)]
    pub dataset: PathBuf,
}

#[derive(Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub dataset: DatasetArg,
    #[arg(long, env = watch::ENV, default_value = "")]
    pub watch: String,
    #[arg(long, env = framework::ENV)]
    pub frameworks: Option<PathBuf>,
    #[arg(long, env = "CROFT_METRICS_ADDR", default_value = "")]
    pub metrics_addr: String,
    #[arg(long, env = "CROFT_ROTATE_SECS", default_value_t = 3600)]
    pub rotate_secs: u64,
    #[arg(long, env = "CROFT_ROTATE_BYTES", default_value_t = 1 << 30)]
    pub rotate_bytes: u64,
    #[arg(long, env = "CROFT_RETAIN_DAYS", default_value_t = 30)]
    pub retain_days: u64,
    #[arg(long, env = "CROFT_RETAIN_BYTES", default_value_t = 10 << 30)]
    pub retain_bytes: u64,
}

#[derive(Args)]
pub struct TopArgs {
    #[command(flatten)]
    pub dataset: DatasetArg,
    #[arg(long, default_value_t = 2)]
    pub interval: u64,
    #[arg(long)]
    pub once: bool,
}

#[derive(Args)]
pub struct StatsArgs {
    /// Dataset files or directories of *.jsonl files
    #[arg(env = dataset::PATH_ENV, default_value = dataset::DEFAULT_PATH, num_args = 1..)]
    pub paths: Vec<PathBuf>,
}

#[derive(Args)]
pub struct ProfileArgs {
    pub container: String,
    #[arg(long = "dataset", env = dataset::PATH_ENV, default_value = dataset::DEFAULT_PATH, num_args = 1..)]
    pub paths: Vec<PathBuf>,
    #[arg(long)]
    pub from_ms: Option<u64>,
    #[arg(long)]
    pub to_ms: Option<u64>,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Platform {
    Docker,
    Kubernetes,
}

#[derive(Args)]
pub struct DeployArgs {
    #[arg(value_enum)]
    pub platform: Platform,
    #[arg(long)]
    pub image: Option<String>,
    #[arg(long)]
    pub watch: Option<String>,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct ScaleArgs {
    #[arg(long)]
    pub namespace: String,
    #[arg(long)]
    pub deployment: String,
    #[arg(long, default_value = "sendto")]
    pub syscall: String,
    #[arg(long)]
    pub calls_per_request: f64,
    #[arg(long)]
    pub cpu_per_request_ms: f64,
    #[arg(long, default_value_t = 0.7)]
    pub target: f64,
    #[arg(long, default_value_t = 10)]
    pub max_replicas: u32,
    #[arg(long, default_value_t = 2)]
    pub confirm_polls: u32,
    #[arg(long, default_value_t = 2)]
    pub interval: u64,
    #[arg(long, default_value = "http://127.0.0.1:8001")]
    pub api: String,
    #[arg(long, default_value = "croft")]
    pub collectors_namespace: String,
    #[arg(long, default_value = "app=croft-collector")]
    pub collectors_selector: String,
    #[arg(long, default_value_t = 9464)]
    pub metrics_port: u16,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct CollectArgs {
    #[arg(long, default_value = "croft")]
    pub namespace: String,
    #[arg(long, default_value = ".")]
    pub out: PathBuf,
    #[arg(long, default_value = "/data/croft-dataset.jsonl")]
    pub remote: String,
}
