//! Cgroups whose processes are recorded whether or not they hold a GPU, from WATCH_CGROUPS.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::process::CGROUP_ROOT;

// Consts for the WATCH_CGROUPS environment variable and the cgroup.procs file.
const ENV: &str = "WATCH_CGROUPS";
const PROCS_FILE: &str = "cgroup.procs";

#[derive(Debug, Default)]
pub struct CgroupWatch {
    patterns: Vec<String>,
}

impl CgroupWatch {
    pub fn new(spec: &str) -> Self {
        Self {
            patterns: spec
                .split(',')
                .map(str::trim)
                .filter(|pattern| !pattern.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(&std::env::var(ENV).unwrap_or_default())
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    pub fn matches(&self, cgroup: &str) -> bool {
        self.patterns
            .iter()
            .any(|pattern| cgroup.contains(pattern.as_str()))
    }

    // processes walks the cgroup tree rather than trusting syscall activity: a CPU-bound loop makes no syscalls.
    pub fn processes(&self) -> Vec<(u32, String)> {
        self.processes_under(Path::new(CGROUP_ROOT))
    }

    fn processes_under(&self, root: &Path) -> Vec<(u32, String)> {
        let mut found = Vec::new();
        let mut pending = vec![PathBuf::from(root)];
        while let Some(dir) = pending.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            pending.extend(
                entries
                    .flatten()
                    .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                    .map(|entry| entry.path()),
            );
            let cgroup = cgroup_path(root, &dir);
            if self.matches(&cgroup) {
                found.extend(procs(&dir).into_iter().map(|tgid| (tgid, cgroup.clone())));
            }
        }
        found
    }
}

fn cgroup_path(root: &Path, dir: &Path) -> String {
    format!("/{}", dir.strip_prefix(root).unwrap_or(dir).display())
}

fn procs(dir: &Path) -> Vec<u32> {
    fs::read_to_string(dir.join(PROCS_FILE))
        .map(|content| {
            content
                .lines()
                .filter_map(|line| line.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIND_POD: &str = "/system.slice/docker-ab12.scope/kubelet.slice/kubelet-kubepods.slice/kubelet-kubepods-burstable.slice/cri-containerd-cd34.scope";

    #[test]
    fn spec_splits_on_commas_and_drops_blanks() {
        let watch = CgroupWatch::new(" kubepods, ,cloud-exp ,");
        assert_eq!(watch.patterns(), ["kubepods", "cloud-exp"]);
    }

    #[test]
    fn empty_spec_watches_nothing() {
        let watch = CgroupWatch::new("");
        assert!(watch.is_empty());
        assert!(!watch.matches(KIND_POD));
    }

    #[test]
    fn processes_come_from_matching_cgroups_at_any_depth() {
        let root = std::env::temp_dir().join(format!("croft-cgroup-test-{}", std::process::id()));
        let pod = root.join("kubepods.slice/pod1.slice/cri-containerd-cd34.scope");
        let other = root.join("system.slice/nomad.service");
        for (dir, procs) in [(&pod, "101\n102\n"), (&other, "7\n")] {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join(PROCS_FILE), procs).unwrap();
        }

        let mut found = CgroupWatch::new("kubepods").processes_under(&root);
        found.sort();
        let cgroup = "/kubepods.slice/pod1.slice/cri-containerd-cd34.scope".to_string();
        assert_eq!(found, [(101, cgroup.clone()), (102, cgroup)]);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn patterns_match_as_substrings() {
        let watch = CgroupWatch::new("kubepods");
        assert!(watch.matches(KIND_POD));
        assert!(watch.matches("/kubepods.slice/kubepods-pod1.slice/cri-containerd-cd34.scope"));
        assert!(!watch.matches("/system.slice/nomad.service"));
    }
}
