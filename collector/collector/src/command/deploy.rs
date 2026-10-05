//! Starts the collector on Docker or Kubernetes, with the flags deploy/README.md explains.

use super::external;
use crate::cli::{DeployArgs, Platform};

// Consts for the manifest template, the container name, the default Docker image, and the paths for the data directory and tracefs.
const MANIFEST: &str = include_str!("../../../../deploy/kubernetes/collector.yaml");
const MANIFEST_IMAGE: &str = "image: croft-collector:dev";
const MANIFEST_WATCH: &str = "value: kubepods";

const CONTAINER: &str = "croft-collector";
const DOCKER_IMAGE: &str = "croft-collector";
const DATA_DIR: &str = "/var/lib/croft";
const TRACEFS: &str = "/sys/kernel/tracing";

// execute deploys the collector on Docker or Kubernetes, with the given image and watch flags, and optionally prints the command or manifest instead of running it.
pub fn execute(args: &DeployArgs) -> anyhow::Result<()> {
    match args.platform {
        Platform::Docker => {
            let docker_args = docker_args(args.image.as_deref(), args.watch.as_deref());
            if args.dry_run {
                println!("{}", external::display("docker", &docker_args));
                return Ok(());
            }
            // Run the docker command with the given arguments and no input.
            external::run("docker", &docker_args, None)
        }
        Platform::Kubernetes => {
            let manifest = manifest(args.image.as_deref(), args.watch.as_deref());
            if args.dry_run {
                print!("{manifest}");
                return Ok(());
            }
            // Run the kubectl apply command with the manifest as input.
            external::run(
                "kubectl",
                &["apply".into(), "-f".into(), "-".into()],
                Some(&manifest),
            )
        }
    }
}

// docker_args needs host PID and cgroup namespaces so backfill and cgroup paths see the host.
fn docker_args(image: Option<&str>, watch: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = [
        "run",
        "-d",
        "--name",
        CONTAINER,
        "--restart",
        "unless-stopped",
        "--privileged",
        "--pid=host",
        "--cgroupns=host",
        "-v",
    ]
    .map(String::from)
    .into();
    args.push(format!("{TRACEFS}:{TRACEFS}:ro"));
    args.extend(["-v".to_string(), format!("{DATA_DIR}:/data")]);
    if let Some(watch) = watch {
        args.extend(["-e".to_string(), format!("WATCH_CGROUPS={watch}")]);
    }
    args.push(image.unwrap_or(DOCKER_IMAGE).to_string());
    args
}

fn manifest(image: Option<&str>, watch: Option<&str>) -> String {
    let mut manifest = MANIFEST.to_string();
    if let Some(image) = image {
        manifest = manifest.replace(MANIFEST_IMAGE, &format!("image: {image}"));
    }
    if let Some(watch) = watch {
        manifest = manifest.replace(MANIFEST_WATCH, &format!("value: \"{watch}\""));
    }
    manifest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_keeps_the_lines_overrides_replace() {
        assert_eq!(MANIFEST.matches(MANIFEST_IMAGE).count(), 1);
        assert_eq!(MANIFEST.matches(MANIFEST_WATCH).count(), 1);
    }

    #[test]
    fn manifest_overrides_image_and_watch() {
        let manifest = manifest(Some("registry.example/croft:v2"), Some("kubepods,docker"));
        assert!(manifest.contains("image: registry.example/croft:v2"));
        assert!(manifest.contains("value: \"kubepods,docker\""));
        assert!(!manifest.contains(MANIFEST_IMAGE));
    }

    #[test]
    fn docker_run_mounts_tracefs_and_data_and_ends_with_the_image() {
        let args = docker_args(None, Some("kubepods"));
        assert!(args.contains(&"--cgroupns=host".to_string()));
        assert!(args.contains(&format!("{TRACEFS}:{TRACEFS}:ro")));
        assert!(args.contains(&"WATCH_CGROUPS=kubepods".to_string()));
        assert_eq!(args.last().map(String::as_str), Some(DOCKER_IMAGE));
    }
}
