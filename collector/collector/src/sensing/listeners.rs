//! The connections waiting for accept() on each process's listening sockets, from /proc/net/tcp.

use std::{
    collections::{HashMap, HashSet},
    fs,
};

// Listener is a listening socket's inode and the number of connections waiting for accept().
pub type Listener = (u64, u64);

// TCP_LISTEN is the hex state code /proc/net/tcp uses for a socket in LISTEN.
const TCP_LISTEN: &str = "0A";

struct Scanned {
    poll: u64,
    listeners: Vec<Listener>,
}

// AcceptQueues caches each process's listening sockets, rescanning every `rescan_polls` polls: matching a process's
// fds to /proc/net/tcp is a directory walk, so a process is not rescanned every poll.
pub struct AcceptQueues {
    rescan_polls: u64,
    poll: u64,
    known: HashMap<u32, Scanned>,
    seen: HashSet<u32>,
}

impl AcceptQueues {
    pub fn new(rescan_polls: u64) -> Self {
        Self {
            rescan_polls,
            poll: 0,
            known: HashMap::new(),
            seen: HashSet::new(),
        }
    }

    pub fn listeners(&mut self, tgid: u32) -> Vec<Listener> {
        self.scan(tgid, listening)
    }

    // scan returns a process's listening sockets, rescanning if not known or `rescan_polls` have passed since.
    fn scan(&mut self, tgid: u32, read: impl FnOnce(u32) -> Vec<Listener>) -> Vec<Listener> {
        self.seen.insert(tgid);
        let due = match self.known.get(&tgid) {
            Some(scanned) => self.poll.saturating_sub(scanned.poll) >= self.rescan_polls,
            None => true,
        };
        if due {
            let listeners = read(tgid);
            self.known.insert(
                tgid,
                Scanned {
                    poll: self.poll,
                    listeners: listeners.clone(),
                },
            );
            listeners
        } else {
            self.known[&tgid].listeners.clone()
        }
    }

    // end_poll forgets processes not seen in this poll, so a reused tgid starts over.
    pub fn end_poll(&mut self) {
        self.poll += 1;
        let seen = std::mem::take(&mut self.seen);
        self.known.retain(|tgid, _| seen.contains(tgid));
    }
}

// listening returns the inode and accept queue of each of a process's own listening sockets. /proc/<pid>/net/tcp
// lists every socket in the process's network namespace, so only the inodes among its own fds are kept.
fn listening(tgid: u32) -> Vec<Listener> {
    let mut sockets = Vec::new();
    for file in ["net/tcp", "net/tcp6"] {
        if let Ok(text) = fs::read_to_string(format!("/proc/{tgid}/{file}")) {
            sockets.extend(parse_listening(&text));
        }
    }
    if sockets.is_empty() {
        return sockets;
    }
    let fds = socket_inodes(tgid);
    sockets.retain(|(inode, _)| fds.contains(inode));
    sockets
}

// socket_inodes returns the inode of every fd that names a socket, from each fd's /proc/<pid>/fd/<n> symlink target.
fn socket_inodes(tgid: u32) -> HashSet<u64> {
    let Ok(entries) = fs::read_dir(format!("/proc/{tgid}/fd")) else {
        return HashSet::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_link(entry.path()).ok())
        .filter_map(|target| {
            target
                .to_str()?
                .strip_prefix("socket:[")?
                .strip_suffix(']')?
                .parse()
                .ok()
        })
        .collect()
}

// parse_listening returns the inode and accept queue of each listening socket.
fn parse_listening(text: &str) -> Vec<Listener> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.get(3) != Some(&TCP_LISTEN) {
                return None;
            }
            let (_, rx_queue) = fields.get(4)?.split_once(':')?;
            let queue = u64::from_str_radix(rx_queue, 16).ok()?;
            let inode = fields.get(9)?.parse().ok()?;
            Some((inode, queue))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_rx_queue_and_inode_of_listening_sockets_only() {
        let text = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000:1F90 00000000:0000 0A 00000000:00000005 00:00000000 00000000     0        0 42 1 0 100 0 0 10 0
   1: 0100007F:0277 00000000:0000 0A 00000000:00000001 00:00000000 00000000     0        0 43 1 0 100 0 0 10 0
   2: 0100007F:C350 0100007F:1F90 01 00000000:00000000 00:00000000 00000000     0        0 44 1 0 100 0 0 10 0
";
        assert_eq!(parse_listening(text), vec![(42, 5), (43, 1)]);
    }

    fn tracker(rescan_polls: u64) -> AcceptQueues {
        AcceptQueues::new(rescan_polls)
    }

    #[test]
    fn rescans_only_every_rescan_polls() {
        let mut queues = tracker(2);
        let mut reads = 0;
        let mut poll = |queues: &mut AcceptQueues, listeners: Vec<Listener>| {
            let got = queues.scan(7, |_| {
                reads += 1;
                listeners
            });
            queues.end_poll();
            got
        };
        assert_eq!(poll(&mut queues, vec![(1, 1)]), vec![(1, 1)]);
        assert_eq!(poll(&mut queues, vec![(1, 2)]), vec![(1, 1)]);
        assert_eq!(poll(&mut queues, vec![(1, 3)]), vec![(1, 3)]);
        assert_eq!(reads, 2);
    }

    #[test]
    fn a_process_gone_for_a_poll_starts_over() {
        let mut queues = tracker(5);
        queues.scan(7, |_| vec![(1, 1)]);
        queues.end_poll();
        queues.end_poll();
        let mut reads = 0;
        assert_eq!(
            queues.scan(7, |_| {
                reads += 1;
                vec![(2, 2)]
            }),
            vec![(2, 2)]
        );
        assert_eq!(reads, 1);
    }
}
