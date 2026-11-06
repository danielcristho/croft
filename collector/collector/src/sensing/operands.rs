//! Cumulative counts of what syscalls carry, from the operand maps, for each process.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use aya::Ebpf;
use collector_common::{AF_INET, AF_INET6, ConnectKey, SOCKET_READ};

use super::{
    maps,
    syscalls::{self, SyscallCount},
};

// Destination is a connect's address and port.
pub type Destination = (IpAddr, u16);

#[derive(Default)]
pub struct Operands {
    bytes: HashMap<u32, Vec<SyscallCount>>,
    connects: HashMap<u32, Vec<(Destination, u64)>>,
    socket_io: HashMap<u32, Vec<SyscallCount>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessOperands<'a> {
    pub bytes: &'a [SyscallCount],
    pub connects: &'a [(Destination, u64)],
    // nr holds SOCKET_READ or SOCKET_WRITE.
    pub socket_io: &'a [SyscallCount],
}

impl Operands {
    pub fn read(ebpf: &Ebpf) -> anyhow::Result<Self> {
        let bytes = syscalls::group_by_tgid(maps::syscall_bytes(ebpf)?.iter().flatten());
        let socket_io = syscalls::group_by_tgid(maps::socket_io(ebpf)?.iter().flatten());
        let mut connects: HashMap<u32, Vec<(Destination, u64)>> = HashMap::new();
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
        })
    }

    pub fn of(&self, tgid: u32) -> ProcessOperands<'_> {
        ProcessOperands {
            bytes: self.bytes.get(&tgid).map_or(&[], Vec::as_slice),
            connects: self.connects.get(&tgid).map_or(&[], Vec::as_slice),
            socket_io: self.socket_io.get(&tgid).map_or(&[], Vec::as_slice),
        }
    }
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
    fn an_unseen_process_has_no_operands() {
        let operands = Operands::default();
        let process = operands.of(7);
        assert!(process.bytes.is_empty() && process.connects.is_empty());
    }
}
