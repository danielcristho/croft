//! Dataset rows read back for inspection, in the types the collector writes.

use serde::Deserialize;

use crate::{
    process::ProcessInfo,
    resources::{CgroupUsage, ProcessUsage},
    syscalls::SyscallRate,
};

// Rows from before resource tracking lack usage; they parse with empty usage.
#[derive(Debug, Deserialize)]
pub struct Row {
    pub unix_ms: u64,
    pub tgid: u32,
    #[serde(default)]
    pub process: ProcessInfo,
    pub workload: String,
    #[serde(default)]
    pub usage: ProcessUsage,
    pub cgroup_usage: Option<CgroupUsage>,
    #[serde(default)]
    pub syscalls: Vec<SyscallRate>,
    pub label: Option<String>,
}

pub enum Line {
    Blank,
    Row(Box<Row>),
    Unreadable,
}

// parse treats NUL padding as blank: a crash during a write leaves it behind.
pub fn parse(line: &str) -> Line {
    let line = line.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    if line.is_empty() {
        return Line::Blank;
    }
    serde_json::from_str(line).map_or(Line::Unreadable, |row| Line::Row(Box::new(row)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_parse_by_kind() {
        let row = r#"{"unix_ms":5,"tgid":7,"process":{"comm":"python3","exe":null,"cgroup":null,"container_id":null},"workload":"gpu-held","ioctl_count":3,"usage":{"cpu_cores":1.5},"cgroup_usage":null,"syscalls":[{"name":"futex","rate_per_sec":2.0}],"label":null}"#;
        let old = r#"{"unix_ms":5,"tgid":7,"workload":"gpu-onset","label":null}"#;
        let Line::Row(parsed) = parse(row) else {
            panic!("row did not parse");
        };
        assert_eq!(parsed.process.comm.as_deref(), Some("python3"));
        assert_eq!(parsed.usage.cpu_cores, Some(1.5));
        assert_eq!(parsed.syscalls[0].name, "futex");
        assert!(matches!(parse(old), Line::Row(_)));
        assert!(matches!(parse("\0\0\0"), Line::Blank));
        assert!(matches!(parse("  "), Line::Blank));
        assert!(matches!(parse(r#"{"unix_ms":"#), Line::Unreadable));
    }
}
