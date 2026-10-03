//! Identity of a host process from /proc, so dataset rows can be labelled later.

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

pub const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const CONTAINER_ID_LEN: usize = 64;
const RUNTIME_PREFIXES: &[&str] = &["docker-", "cri-containerd-", "crio-", "libpod-"];
const SANDBOX_EXE: &str = "/pause";

// The command line is left out on purpose: its arguments can carry tokens and passwords.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub comm: Option<String>,
    pub exe: Option<String>,
    pub cgroup: Option<String>,
    pub container_id: Option<String>,
}

impl ProcessInfo {
    // is_pod_sandbox spots the pause process, which only holds a pod's namespaces.
    pub fn is_pod_sandbox(&self) -> bool {
        self.exe.as_deref() == Some(SANDBOX_EXE)
    }
}

// read returns empty fields for a process that exited between the poll and this read.
pub fn read(tgid: u32) -> ProcessInfo {
    identify(tgid, cgroup_of(tgid))
}

fn cgroup_of(tgid: u32) -> Option<String> {
    fs::read_to_string(format!("/proc/{tgid}/cgroup"))
        .ok()
        .and_then(|content| unified_cgroup(&content))
}

// identify completes a process whose cgroup the caller has read already.
pub fn identify(tgid: u32, cgroup: Option<String>) -> ProcessInfo {
    let dir = PathBuf::from(format!("/proc/{tgid}"));
    ProcessInfo {
        comm: fs::read_to_string(dir.join("comm"))
            .ok()
            .map(|comm| comm.trim_end().to_string()),
        exe: fs::read_link(dir.join("exe"))
            .ok()
            .map(|exe| exe.display().to_string()),
        container_id: cgroup.as_deref().and_then(container_id),
        cgroup,
    }
}

// unified_cgroup picks the cgroup v2 path, the line with hierarchy id 0.
fn unified_cgroup(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .map(str::to_string)
}

// container_id reads the id out of a scope such as docker-<64 hex>.scope or cri-containerd-<64 hex>.scope.
fn container_id(cgroup: &str) -> Option<String> {
    let scope = cgroup.rsplit('/').next()?;
    let scope = scope.strip_suffix(".scope").unwrap_or(scope);
    let id = RUNTIME_PREFIXES
        .iter()
        .find_map(|prefix| scope.strip_prefix(prefix))
        .unwrap_or(scope);
    is_container_id(id).then(|| id.to_string())
}

// is_container_id checks for a 64-character hex string.
fn is_container_id(id: &str) -> bool {
    id.len() == CONTAINER_ID_LEN && id.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "dfc8d626ea68834e5101df9eb0cb8dc51b1672a2572b7c0ba0a8af953c88da36";

    #[test]
    fn unified_cgroup_takes_the_v2_line() {
        let content = "12:cpu:/legacy\n0::/system.slice/nomad.service\n";
        assert_eq!(
            unified_cgroup(content).as_deref(),
            Some("/system.slice/nomad.service")
        );
        assert_eq!(unified_cgroup("12:cpu:/legacy\n"), None);
    }

    #[test]
    fn container_id_from_runtime_scopes() {
        let cases = [
            (format!("/system.slice/docker-{ID}.scope"), Some(ID)),
            (
                format!("/kubepods.slice/kubepods-pod1.slice/cri-containerd-{ID}.scope"),
                Some(ID),
            ),
            (format!("/docker/{ID}"), Some(ID)),
            ("/system.slice/nomad.service".to_string(), None),
            (
                "/user.slice/user-1004.slice/session.slice/org.gnome.Shell@wayland.service"
                    .to_string(),
                None,
            ),
        ];
        for (cgroup, want) in cases {
            assert_eq!(container_id(&cgroup).as_deref(), want, "{cgroup}");
        }
    }

    #[test]
    fn pause_is_a_pod_sandbox() {
        let pause = ProcessInfo {
            exe: Some("/pause".into()),
            ..ProcessInfo::default()
        };
        let app = ProcessInfo {
            exe: Some("/usr/local/bin/pause".into()),
            ..ProcessInfo::default()
        };
        assert!(pause.is_pod_sandbox());
        assert!(!app.is_pod_sandbox());
    }

    #[test]
    fn exited_process_reads_as_empty() {
        assert_eq!(read(u32::MAX), ProcessInfo::default());
    }
}
