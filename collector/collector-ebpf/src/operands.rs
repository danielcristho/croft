//! Records what syscalls carry: bytes moved, connect destinations, reads and writes that reach a socket, and
//! accepted connections holding a request not yet answered.

use aya_ebpf::{
    helpers::bpf_probe_read_user_buf,
    macros::{kprobe, tracepoint},
    maps::LruHashMap,
    programs::{ProbeContext, TracePointContext},
};

helpers::{bpf_get_current_pid_tgid, bpf_probe_read_user_buf}

use collector_common::{
    AF_INET, AF_INET6, ConnectKey, DropSite, FD_AWAITING, FD_IDLE, SOCKET_READ, SOCKET_WRITE,
    is_accept, is_byte_syscall, is_reply_write, is_request_read, pack_key,
};
    maps::{CONNECTS, SERVER_FDS, SOCKET_IO, SYSCALL_BYTES, THREAD_FD, count_on_err},


use crate::{
    context::{arg, current_tgid, ret, syscall_nr, tracepoint_ret},
    maps::{CONNECTS, SOCKET_IO, SYSCALL_BYTES, count_on_err},
};

// Consts for the sockaddr layout.
const SOCKADDR_ARG: usize = 1;
const IPV6_ADDR_OFFSET: u64 = 8;

#[tracepoint]
pub fn sys_exit(ctx: TracePointContext) -> u32 {
    let bytes = add_bytes(&ctx);
    tracepoint_ret(bytes.and_then(|()| track_request(&ctx)))
}

#[tracepoint]
pub fn sys_enter_connect(ctx: TracePointContext) -> u32 {
    tracepoint_ret(count_connect(&ctx))
}

#[kprobe]
pub fn sock_read_iter(_ctx: ProbeContext) -> u32 {
    add(
        &SOCKET_IO,
        pack_key(current_tgid(), SOCKET_READ),
        1,
        DropSite::SocketIo,
    );
    0
}

#[kprobe]
pub fn sock_write_iter(_ctx: ProbeContext) -> u32 {
    add(
        &SOCKET_IO,
        pack_key(current_tgid(), SOCKET_WRITE),
        1,
        DropSite::SocketIo,
    );
    0
}

fn add_bytes(ctx: &TracePointContext) -> Result<(), i32> {
    let nr = syscall_nr(ctx)?;
    if !is_byte_syscall(nr) {
        return Ok(());
    }
    let bytes = ret(ctx)?;
    if bytes > 0 {
        add(
            &SYSCALL_BYTES,
            pack_key(current_tgid(), nr as u32),
            bytes as u64,
            DropSite::SyscallBytes,
        );
    }
    Ok(())
}

// track_request marks an accepted connection as awaiting a reply when a read returns data, and idle when a write
// does. Connections the process opened itself are never in SERVER_FDS, so a client's reads and writes do not count.
fn track_request(ctx: &TracePointContext) -> Result<(), i32> {
    let nr = syscall_nr(ctx)?;
    let result = ret(ctx)?;
    if is_accept(nr) {
        if result >= 0 {
            let key = pack_key(current_tgid(), result as u32);
            count_on_err(SERVER_FDS.insert(&key, &FD_IDLE, 0), DropSite::ServerFd);
        }
        return Ok(());
    }
    let state = if is_request_read(nr) {
        FD_AWAITING
    } else if is_reply_write(nr) {
        FD_IDLE
    } else {
        return Ok(());
    };
    let pid_tgid = bpf_get_current_pid_tgid();
    // SAFETY: the value is copied out before the entry can change.
    let Some(fd) = (unsafe { THREAD_FD.get(&pid_tgid) }).copied() else {
        return Ok(());
    };
    let _ = THREAD_FD.remove(&pid_tgid);
    let key = pack_key(current
    // SAFETY: the reference iopped on this line.
    if result <= 0 || unsafe {ne() {
        return Ok(());
    }
    count_on_err(SERVER_FDS.inte::ServerFd);
    Ok(())
}


// count_connect counts attempts: a nonblocking connect has no result yet at entry.
fn count_connect(ctx: &TracePointContext) -> Result<(), i32> {
    let sockaddr: u64 = arg(ctx, SOCKADDR_ARG)?;
    let mut head = [0u8; 8];
    // SAFETY: a bad user pointer makes the helper return an error, not fault.
    if unsafe { bpf_probe_read_user_buf(sockaddr as *const u8, &mut head) }.is_err() {
        return Ok(());
    }
    let [family_0, family_1, port_0, port_1, a, b, c, d] = head;
    let family = u16::from_ne_bytes([family_0, family_1]);
    let mut addr = [0u8; 16];
    match family {
        AF_INET => {
            addr[0] = a;
            addr[1] = b;
            addr[2] = c;
            addr[3] = d;
        }
        AF_INET6 => {
            let ipv6 = (sockaddr + IPV6_ADDR_OFFSET) as *const u8;
            // SAFETY: as above.
            if unsafe { bpf_probe_read_user_buf(ipv6, &mut addr) }.is_err() {
                return Ok(());
            }
        }
        _ => return Ok(()),
    }
    let key = ConnectKey {
        tgid: current_tgid(),
        family,
        port: u16::from_be_bytes([port_0, port_1]),
        addr,
    };
    add(&CONNECTS, key, 1, DropSite::Connects);
    Ok(())
}

fn add<K>(map: &LruHashMap<K, u64>, key: K, amount: u64, site: DropSite) {
    // SAFETY: a racing update loses one count, it does not corrupt state.
    let total = unsafe { map.get(&key) }.copied().unwrap_or(0) + amount;
    count_on_err(map.insert(&key, &total, 0), site);
}
