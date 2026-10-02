//! Joins the host cgroup namespace, which Kubernetes cannot grant to a pod.

use std::{fs, fs::File, os::fd::AsRawFd};

use log::{info, warn};

const OWN_CGROUP_NS: &str = "/proc/self/ns/cgroup";
const HOST_CGROUP_NS: &str = "/proc/1/ns/cgroup";

// join_host_cgroup must run before any thread starts: setns changes the calling thread, and threads inherit it.
pub fn join_host_cgroup() {
    let (Ok(own), Ok(host)) = (fs::read_link(OWN_CGROUP_NS), fs::read_link(HOST_CGROUP_NS)) else {
        return;
    };
    if own == host {
        return;
    }
    let file = match File::open(HOST_CGROUP_NS) {
        Ok(file) => file,
        Err(e) => {
            warn!("open the host cgroup namespace: {e}");
            return;
        }
    };
    // SAFETY: file holds an open namespace fd for the whole call.
    if unsafe { libc::setns(file.as_raw_fd(), libc::CLONE_NEWCGROUP) } != 0 {
        warn!(
            "join the host cgroup namespace: {}",
            std::io::Error::last_os_error()
        );
        return;
    }
    info!("joined the host cgroup namespace");
}
