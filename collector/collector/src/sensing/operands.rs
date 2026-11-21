//! Cumulative counts of what syscalls carry, from the operand maps, for each process.

use std::{
    collections::{HashMap, HashSet},
    fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::Path,
};

use aya::Ebpf;
use collector_common::{
    AF_INET, AF_INET6, ConnectKey, FD_AWAITING, MAX_SERVER_FDS, SOCKET_READ, unpack_key,
};
use log::warn;

use super::{
    listeners::Listener,
    maps,
    // pub inflight: Option<u64>,
    // pub listeners: &'a [Listener],
    syscalls::{self, SyscallCount},
};

// Destination is a connect's address and port.
pub type Destination = (IpAddr, u16);

#[derive(Default)]
pub struct Operands {
    bytes: HashMap<u32, Vec<SyscallCount>>,
    connects: HashMap<u32, Vec<(Destination, u64)>>,
    socket_io: HashMap<u32, Vec<SyscallCount>>,
    inflight: HashMap<u32, u64>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessOperands<'a> {
    pub bytes: &'a [SyscallCount],
    pub connects: &'a [(Destination, u64)],
    // nr holds SOCKET_READ or SOCKET_WRITE.
    pub socket_io: &'a [SyscallCount],
    // inflight is None for a process with no tracked accepted connection.
    pub inflight: Option<u64>,
    pub listeners: &'a [Listener],
}

impl Operands {
    // read takes the SERVER_FDS entries ServerFdPruner kept, so the map is walked once per poll.
    pub fn read(ebpf: &Ebpf, server_fds: &[(u64, u8)]) -> anyhow::Result<Self> {
        let bytes = syscalls::group_by_tgid(maps::syscall_bytes(ebpf)?.iter().flatten());
        let socket_io = syscalls::group_by_tgid(maps::socket_io(ebpf)?.iter().flatten());
        let mut connects: HashMap<u32, Vec<(Destination, u64)>> = HashMap::new();
        let inflight = awaiting_by_tgid(server_fds.iter().copied());
        for (key, count) in maps::connects(ebpf)?.iter().flatten() {
            if let Some(destination) = destination(&key) {
                connects
                    .entry(key.tgid)
                    .or_default()
                    .push((destination, count));
            }
        }
        Ok(Self {
            bytes,
            connects,
            socket_io,
            inflight,
        })
    }

    pub fn of(&self, tgid: u32) -> ProcessOperands<'_> {
        ProcessOperands {
            bytes: self.bytes.get(&tgid).map_or(&[], Vec::as_slice),
            connects: self.connects.get(&tgid).map_or(&[], Vec::as_slice),
            socket_io: self.socket_io.get(&tgid).map_or(&[], Vec::as_slice),
            inflight: self.inflight.get(&tgid).copied(),
            listeners: &[],
        }
    }
}

// awaiting_by_tgid counts, per process with an accepted connection, the connections whose last read has no reply yet.
fn awaiting_by_tgid(entries: impl Iterator<Item = (u64, u8)>) -> HashMap<u32, u64> {
    let mut by_tgid = HashMap::new();
    for (key, state) in entries {
        let count = by_tgid.entry(unpack_key(key).0).or_default();
        if state == FD_AWAITING {
            *count += 1;
        }
    }
    by_tgid
}

// ServerFdPruner removes SERVER_FDS entries that no close syscall cleared, and warns once when the map nears its cap.
#[derive(Default)]
pub struct ServerFdPruner {
    warned: bool,
}

impl ServerFdPruner {
    // prune returns the entries it kept.
    pub fn prune(&mut self, ebpf: &mut Ebpf) -> anyhow::Result<Vec<(u64, u8)>> {
        let mut fds = maps::server_fds(ebpf)?;
        let mut entries: Vec<(u64, u8)> = fds.iter().flatten().collect();
        let stale = stale_keys(&entries, process_alive, fd_is_socket);
        for key in &stale {
            let _ = fds.remove(key);
        }
        entries.retain(|(key, _)| !stale.contains(key));
        let near_cap = entries.len() as u64 * 10 >= u64::from(MAX_SERVER_FDS) * 9;
        if near_cap && !self.warned {
            warn!(
                "SERVER_FDS holds {} of {MAX_SERVER_FDS} connections; the LRU evicts the rest, in-flight requests among them",
                entries.len()
            );
        }
        self.warned = near_cap;
        Ok(entries)
    }
}

// stale_keys picks the entries of exited processes, and the awaiting entries whose fd no longer names a socket after
// an execve, dup2 or close_range. Only awaiting entries pay a readlink, as only they are counted.
fn stale_keys(
    entries: &[(u64, u8)],
    alive: impl Fn(u32) -> bool,
    socket: impl Fn(u32, u32) -> bool,
) -> HashSet<u64> {
    let mut alive_by_tgid: HashMap<u32, bool> = HashMap::new();
    entries
        .iter()
        .filter(|&&(key, state)| {
            let (tgid, fd) = unpack_key(key);
            let tgid_alive = *alive_by_tgid.entry(tgid).or_insert_with(|| alive(tgid));
            !tgid_alive || (state == FD_AWAITING && !socket(tgid, fd))
        })
        .map(|&(key, _)| key)
        .collect()
}

fn process_alive(tgid: u32) -> bool {
    Path::new(&format!("/proc/{tgid}")).exists()
}

fn fd_is_socket(tgid: u32, fd: u32) -> bool {
    fs::read_link(format!("/proc/{tgid}/fd/{fd}")).is_ok_and(|target| {
        target
            .as_os_str()
            .as_encoded_bytes()
            .starts_with(b"socket:")
    })
}

// socket_direction names a key of SOCKET_IO.
pub fn socket_direction(key: u32) -> &'static str {
    if key == SOCKET_READ { "read" } else { "write" }
}

// destination decodes a key; a v4-mapped IPv6 address reads as its IPv4 address, so one host has one label.
fn destination(key: &ConnectKey) -> Option<Destination> {
    let address = match key.family {
        AF_INET => {
            let [a, b, c, d, ..] = key.addr;
            IpAddr::V4(Ipv4Addr::new(a, b, c, d))
        }
        AF_INET6 => IpAddr::V6(Ipv6Addr::from(key.addr)).to_canonical(),
        _ => return None,
    };
    Some((address, key.port))
}

#[cfg(test)]
mod tests {
    use collector_common::SOCKET_WRITE;

    use super::*;

    fn key(family: u16, addr: [u8; 16]) -> ConnectKey {
        ConnectKey {
            tgid: 1,
            family,
            port: 53,
            addr,
        }
    }

    #[test]
    fn ipv4_destination_uses_the_first_four_bytes() {
        let mut addr = [0; 16];
        addr[..4].copy_from_slice(&[10, 43, 0, 10]);
        assert_eq!(
            destination(&key(AF_INET, addr)),
            Some(("10.43.0.10".parse().unwrap(), 53))
        );
    }

    #[test]
    fn v4_mapped_ipv6_reads_as_ipv4() {
        let mapped: Ipv6Addr = "::ffff:10.43.0.10".parse().unwrap();
        assert_eq!(
            destination(&key(AF_INET6, mapped.octets())),
            Some(("10.43.0.10".parse().unwrap(), 53))
        );
    }

    #[test]
    fn ipv6_destination_stays_ipv6() {
        let address: Ipv6Addr = "fd00::1".parse().unwrap();
        assert_eq!(
            destination(&key(AF_INET6, address.octets())),
            Some((IpAddr::V6(address), 53))
        );
    }

    #[test]
    fn other_families_have_no_destination() {
        assert_eq!(destination(&key(1, [0; 16])), None);
    }

    #[test]
    fn socket_keys_name_their_direction() {
        assert_eq!(socket_direction(SOCKET_READ), "read");
        assert_eq!(socket_direction(SOCKET_WRITE), "write");
    }

    #[test]
    fn only_awaiting_connections_count_as_inflight() {
        use collector_common::{FD_IDLE, pack_key};
        let entries = [
            (pack_key(1, 5), FD_AWAITING),
            (pack_key(1, 6), FD_IDLE),
            (pack_key(1, 7), FD_AWAITING),
            (pack_key(2, 5), FD_IDLE),
        ];
        let inflight = awaiting_by_tgid(entries.into_iter());
        assert_eq!(inflight.get(&1), Some(&2));
        assert_eq!(inflight.get(&2), Some(&0));
        assert_eq!(inflight.get(&3), None);
    }

    #[test]
    fn exited_processes_and_awaiting_fds_that_are_no_socket_are_stale() {
        use collector_common::{FD_IDLE, pack_key};
        let entries = [
            (pack_key(1, 5), FD_AWAITING),
            (pack_key(1, 6), FD_AWAITING),
            (pack_key(1, 7), FD_IDLE),
            (pack_key(2, 5), FD_IDLE),
        ];
        let stale = stale_keys(&entries, |tgid| tgid == 1, |_, fd| fd == 5);
        assert_eq!(stale, HashSet::from([pack_key(1, 6), pack_key(2, 5)]));
    }

    #[test]
    fn an_unseen_process_has_no_operands() {
        let operands = Operands::default();
        let process = operands.of(7);
        assert!(process.bytes.is_empty() && process.connects.is_empty());
        assert_eq!(process.inflight, None);
    }
}
