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
    /// Run the collector on this host; needs root
    Run(RunArgs),
    /// Show the processes of the newest poll in a dataset
    Top(TopArgs),
    /// Summarize datasets: files, or directories of per-node files
    Stats(StatsArgs),
    /// Resource profile of a container over a window, as JSON
    Profile(ProfileArgs),
    /// Start the collector on Docker or Kubernetes
    Deploy(DeployArgs),
    /// Append what each node's dataset gained since the last collect, from the Kubernetes DaemonSet
    Collect(CollectArgs),
    /// Add replicas to a Deployment from the request rate the collectors see, ahead of the HPA
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
    /// Also record every process in cgroups whose path contains one of these comma-separated substrings
    #[arg(long, env = watch::ENV, default_value = "")]
    pub watch: String,
    /// JSON catalog of frameworks to detect from mapped libraries; the built-in one when unset
    #[arg(long, env = framework::ENV)]
    pub frameworks: Option<PathBuf>,
    /// Serve the latest poll's per-container signals at http://ADDR/metrics, e.g. 0.0.0.0:9464; off when empty
    #[arg(long, env = "CROFT_METRICS_ADDR", default_value = "")]
    pub metrics_addr: String,
}

#[derive(Args)]
pub struct TopArgs {
    #[command(flatten)]
    pub dataset: DatasetArg,
    /// Seconds between refreshes
    #[arg(long, default_value_t = 2)]
    pub interval: u64,
    /// Print once and exit
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
    /// Container id, or a prefix of at least 4 hex digits
    pub container: String,
    /// Dataset files or directories of *.jsonl files, one per node
    #[arg(long = "dataset", env = dataset::PATH_ENV, default_value = dataset::DEFAULT_PATH, num_args = 1..)]
    pub paths: Vec<PathBuf>,
    /// Keep rows at or after this Unix time in milliseconds
    #[arg(long)]
    pub from_ms: Option<u64>,
    /// Keep rows at or before this Unix time in milliseconds
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
    /// Collector image; defaults to croft-collector on Docker and the manifest's image on Kubernetes
    #[arg(long)]
    pub image: Option<String>,
    /// WATCH_CGROUPS for the collector; defaults to none on Docker and kubepods on Kubernetes
    #[arg(long)]
    pub watch: Option<String>,
    /// Print the command or manifest instead of applying it
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct ScaleArgs {
    /// Namespace of the Deployment to scale
    #[arg(long)]
    pub namespace: String,
    /// Deployment to scale
    #[arg(long)]
    pub deployment: String,
    /// Syscall counted per request, as named in the dataset
    #[arg(long, default_value = "sendto")]
    pub syscall: String,
    /// Calls of that syscall per request, measured for the application
    #[arg(long)]
    pub calls_per_request: f64,
    /// CPU time one request costs, in milliseconds, measured for the application
    #[arg(long)]
    pub cpu_per_request_ms: f64,
    /// Target CPU utilization of a replica, as the HPA's
    #[arg(long, default_value_t = 0.7)]
    pub target: f64,
    /// Upper bound on replicas, as the HPA's
    #[arg(long, default_value_t = 10)]
    pub max_replicas: u32,
    /// Consecutive polls that must ask for more replicas before scaling
    #[arg(long, default_value_t = 2)]
    pub confirm_polls: u32,
    /// Seconds between polls
    #[arg(long, default_value_t = 2)]
    pub interval: u64,
    /// Kubernetes API through `kubectl proxy`
    #[arg(long, default_value = "http://127.0.0.1:8001")]
    pub api: String,
    /// Namespace of the collector DaemonSet
    #[arg(long, default_value = "croft")]
    pub collectors_namespace: String,
    /// Label selector of the collector pods
    #[arg(long, default_value = "app=croft-collector")]
    pub collectors_selector: String,
    /// Port of the collectors' /metrics
    #[arg(long, default_value_t = 9464)]
    pub metrics_port: u16,
    /// Log decisions without changing replicas
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct CollectArgs {
    /// Namespace of the collector DaemonSet
    #[arg(long, default_value = "croft")]
    pub namespace: String,
    /// Directory for the per-node files, <node>.jsonl
    #[arg(long, default_value = ".")]
    pub out: PathBuf,
    /// Dataset path inside the collector pods
    #[arg(long, default_value = "/data/croft-dataset.jsonl")]
    pub remote: String,
}
