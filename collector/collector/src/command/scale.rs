//! The scaler loop: reads every collector's /metrics, sums the Deployment's request rate and adds replicas.

use std::{
    collections::HashSet,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow};
use log::{debug, info, warn};
use serde_json::{Value, json};

use crate::{
    cli::ScaleArgs,
    decision::scaler::{self, Calibration, Confirm},
    pipeline::metrics,
    runtime::http,
};

struct Target {
    replicas: u32,
    cpu_request_cores: f64,
    containers: HashSet<String>,
}

pub fn execute(args: &ScaleArgs) -> anyhow::Result<()> {
    let calibration = Calibration {
        syscall: args.syscall.clone(),
        calls_per_request: args.calls_per_request,
        cpu_seconds_per_request: args.cpu_per_request_ms / 1000.0,
        target_utilization: args.target,
    };
    let mut confirm = Confirm::new(args.confirm_polls);
    let interval = Duration::from_secs(args.interval);
    info!(
        "scaling {}/{} from {} per request ({} ms CPU, {} calls), target {}, confirm {} polls{}",
        args.namespace,
        args.deployment,
        args.syscall,
        args.cpu_per_request_ms,
        args.calls_per_request,
        args.target,
        args.confirm_polls,
        if args.dry_run { ", dry run" } else { "" }
    );
    loop {
        let started = Instant::now();
        if let Err(e) = poll(args, &calibration, &mut confirm) {
            warn!("scale poll: {e:#}");
        }
        thread::sleep(interval.saturating_sub(started.elapsed()));
    }
}

fn poll(args: &ScaleArgs, calibration: &Calibration, confirm: &mut Confirm) -> anyhow::Result<()> {
    let target = target(args)?;
    let samples: Vec<metrics::Sample> = collector_addresses(args)?
        .iter()
        .filter_map(|ip| {
            let url = format!("http://{ip}:{}/metrics", args.metrics_port);
            http::get(&url).map_err(|e| debug!("{e:#}")).ok()
        })
        .flat_map(|text| metrics::parse(&text))
        .collect();
    let rate = scaler::requests_per_sec(&samples, &target.containers, calibration);
    let needed = scaler::needed(
        rate,
        target.cpu_request_cores,
        calibration,
        args.max_replicas,
    );
    debug!(
        "rate={rate:.1}/s needed={needed} replicas={}",
        target.replicas
    );
    let Some(replicas) = confirm.step(needed, target.replicas) else {
        return Ok(());
    };
    info!(
        "scale up from={} to={replicas} rate={rate:.1}/s",
        target.replicas
    );
    if !args.dry_run {
        let url = format!(
            "{}/apis/apps/v1/namespaces/{}/deployments/{}/scale",
            args.api, args.namespace, args.deployment
        );
        http::merge_patch(&url, &json!({"spec": {"replicas": replicas}}).to_string())?;
    }
    Ok(())
}

// target reads the Deployment's replicas, CPU request and the container ids of its pods.
fn target(args: &ScaleArgs) -> anyhow::Result<Target> {
    let url = format!(
        "{}/apis/apps/v1/namespaces/{}/deployments/{}",
        args.api, args.namespace, args.deployment
    );
    let deployment: Value = serde_json::from_str(&http::get(&url)?).context("parse deployment")?;
    let replicas = deployment["spec"]["replicas"].as_u64().unwrap_or(1) as u32;
    // The HPA's utilization is relative to the request, so the scaler sizes replicas against it too.
    let cpu_request_cores =
        deployment["spec"]["template"]["spec"]["containers"][0]["resources"]["requests"]["cpu"]
            .as_str()
            .and_then(scaler::cpu_cores)
            .ok_or_else(|| anyhow!("{} has no CPU request", args.deployment))?;
    let selector = match_labels(&deployment["spec"]["selector"]["matchLabels"]);
    let containers = pods(&args.api, &args.namespace, &selector)?
        .iter()
        .flat_map(|pod| {
            pod["status"]["containerStatuses"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|status| {
            let id = status["containerID"].as_str()?;
            Some(id.split_once("://").map_or(id, |(_, id)| id).to_string())
        })
        .collect();
    Ok(Target {
        replicas,
        cpu_request_cores,
        containers,
    })
}

fn collector_addresses(args: &ScaleArgs) -> anyhow::Result<Vec<String>> {
    Ok(pods(
        &args.api,
        &args.collectors_namespace,
        &args.collectors_selector,
    )?
    .iter()
    .filter_map(|pod| pod["status"]["podIP"].as_str().map(str::to_string))
    .collect())
}

fn pods(api: &str, namespace: &str, selector: &str) -> anyhow::Result<Vec<Value>> {
    let selector = selector.replace('=', "%3D").replace(',', "%2C");
    let url = format!("{api}/api/v1/namespaces/{namespace}/pods?labelSelector={selector}");
    let list: Value = serde_json::from_str(&http::get(&url)?).context("parse pod list")?;
    Ok(list["items"].as_array().cloned().unwrap_or_default())
}

fn match_labels(labels: &Value) -> String {
    labels
        .as_object()
        .map(|labels| {
            labels
                .iter()
                .filter_map(|(key, value)| Some(format!("{key}={}", value.as_str()?)))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_from_match_labels() {
        assert_eq!(
            match_labels(&json!({"app": "webapp", "tier": "web"})),
            "app=webapp,tier=web"
        );
        assert_eq!(match_labels(&json!(null)), "");
    }
}
