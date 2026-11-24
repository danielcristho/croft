//! The connections waiting for accept() on each process's listening sockets, from /proc/net/tcp.

use std::{
    collections::{HashMap, HashSet},
    fs,
};

// Listener is a listening socket's inode and the number of connections waiting for accept().
pub type Listener = (u64, u64);

// TCP_LISTEN is the hex state code /proc/net/tcp uses for a socket in LISTEN.
const TCP_LISTEN: &str = "0A";

// Proc reads what AcceptQueues needs from /proc, so the tests can count how often each read runs.
pub trait Proc {
    // netns is the inode of the process's network namespace, from its /proc/<pid>/ns/net link.
    fn netns(&self, tgid: u32) -> Option<u64>;
    // socket_inodes walks the process's fds: the expensive read, so AcceptQueues caches it.
    fn socket_inodes(&self, tgid: u32) -> HashSet<u64>;
    // listening reads the listening sockets of the process's network namespace, inode to accept queue.
    fn listening(&self, tgid: u32) -> HashMap<u64, u64>;
}

pub struct ProcFs;

impl Proc for ProcFs {
    fn netns(&self, tgid: u32) -> Option<u64> {
        let link = fs::read_link(format!("/proc/{tgid}/ns/net")).ok()?;
        link.to_str()?
            .strip_prefix("net:[")?
            .strip_suffix(']')?
            .parse()
            .ok()
    }

    fn socket_inodes(&self, tgid: u32) -> HashSet<u64> {
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

    fn listening(&self, tgid: u32) -> HashMap<u64, u64> {
        ["net/tcp", "net/tcp6"]
            .iter()
            .filter_map(|file| fs::read_to_string(format!("/proc/{tgid}/{file}")).ok())
            .flat_map(|text| parse_listening(&text))
            .collect()
    }
}

// Scanned is what the last fd walk of a process found: its network namespace and the inodes of its listening sockets.
struct Scanned {
    next_scan: u64,
    netns: Option<u64>,
    inodes: HashSet<u64>,
}

// AcceptQueues reads each process's accept queues every poll. The fd walk that finds a process's listening sockets
// runs once per `rescan_polls` polls, spread over the polls by tgid so the walks do not all land in one poll. Each
// network namespace's tcp tables are read at most once per poll, and only for a process that listens: a process in
// the host network namespace reads the node's whole table.
pub struct AcceptQueues<P: Proc = ProcFs> {
    proc: P,
    rescan_polls: u64,
    poll: u64,
    known: HashMap<u32, Scanned>,
    seen: HashSet<u32>,
    tables: HashMap<u64, HashMap<u64, u64>>,
}

impl AcceptQueues {
    pub fn new(rescan_polls: u64) -> Self {
        Self::with(ProcFs, rescan_polls)
    }
}

impl<P: Proc> AcceptQueues<P> {
    pub fn with(proc: P, rescan_polls: u64) -> Self {
        Self {
            proc,
            rescan_polls: rescan_polls.max(1),
            poll: 0,
            known: HashMap::new(),
            seen: HashSet::new(),
            tables: HashMap::new(),
        }
    }

    // listeners returns the accept queue of each of the process's listening sockets at this poll.
    pub fn listeners(&mut self, tgid: u32) -> Vec<Listener> {
        self.seen.insert(tgid);
        let first = !self.known.contains_key(&tgid);
        if first
            || self
                .known
                .get(&tgid)
                .is_some_and(|s| self.poll >= s.next_scan)
        {
            // A new process's second walk comes 1 to rescan_polls polls later, by tgid, and every rescan_polls after.
            let wait = if first {
                1 + u64::from(tgid) % self.rescan_polls
            } else {
                self.rescan_polls
            };
            let netns = self.proc.netns(tgid);
            let inodes = if netns.is_some() {
                self.proc.socket_inodes(tgid)
            } else {
                HashSet::new()
            };
            self.known.insert(
                tgid,
                Scanned {
                    next_scan: self.poll + wait,
                    netns,
                    inodes,
                },
            );
        }
        let scanned = &self.known[&tgid];
        let Some(netns) = scanned.netns else {
            return Vec::new();
        };
        if scanned.inodes.is_empty() {
            return Vec::new();
        }
        let proc = &self.proc;
        let table = self
            .tables
            .entry(netns)
            .or_insert_with(|| proc.listening(tgid));
        // The table lists every socket of the namespace; only the process's own fds are its listeners.
        let mut found: Vec<Listener> = scanned
            .inodes
            .iter()
            .filter_map(|inode| table.get(inode).map(|queue| (*inode, *queue)))
            .collect();
        found.sort_unstable();
        found
    }

    // end_poll forgets processes not seen in this poll, so a reused tgid starts over, and the poll's tables.
    pub fn end_poll(&mut self) {
        self.poll += 1;
        let seen = std::mem::take(&mut self.seen);
        self.known.retain(|tgid, _| seen.contains(tgid));
        self.tables.clear();
    }
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
    use std::cell::{Cell, RefCell};

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

    // Fake is a /proc whose tables the test changes between polls, and that counts its reads.
    #[derive(Default)]
    struct Fake {
        netns: HashMap<u32, u64>,
        inodes: HashMap<u32, HashSet<u64>>,
        tables: RefCell<HashMap<u64, HashMap<u64, u64>>>,
        walks: RefCell<Vec<u32>>,
        table_reads: Cell<u32>,
    }

    impl Proc for Fake {
        fn netns(&self, tgid: u32) -> Option<u64> {
            self.netns.get(&tgid).copied()
        }

        fn socket_inodes(&self, tgid: u32) -> HashSet<u64> {
            self.walks.borrow_mut().push(tgid);
            self.inodes.get(&tgid).cloned().unwrap_or_default()
        }

        fn listening(&self, tgid: u32) -> HashMap<u64, u64> {
            self.table_reads.set(self.table_reads.get() + 1);
            let netns = self.netns[&tgid];
            self.tables
                .borrow()
                .get(&netns)
                .cloned()
                .unwrap_or_default()
        }
    }

    fn listening_process(tgid: u32, netns: u64, inode: u64) -> Fake {
        let mut fake = Fake::default();
        fake.netns.insert(tgid, netns);
        fake.inodes.insert(tgid, HashSet::from([inode]));
        fake
    }

    fn set_queue(queues: &AcceptQueues<Fake>, netns: u64, inode: u64, queue: u64) {
        queues
            .proc
            .tables
            .borrow_mut()
            .insert(netns, HashMap::from([(inode, queue)]));
    }

    #[test]
    fn the_queue_follows_the_table_every_poll_between_fd_walks() {
        let mut queues = AcceptQueues::with(listening_process(7, 1, 42), 4);
        let mut seen = Vec::new();
        for queue in [1, 2, 3, 4, 5, 6] {
            set_queue(&queues, 1, 42, queue);
            seen.push(queues.listeners(7));
            queues.end_poll();
        }
        let want: Vec<Vec<Listener>> = (1..=6).map(|queue| vec![(42, queue)]).collect();
        assert_eq!(seen, want);
        // tgid 7 with 4 polls per walk: at poll 0, again at 1 + 7 % 4 = 4, then at 8.
        assert_eq!(queues.proc.walks.borrow().len(), 2);
        assert_eq!(queues.proc.table_reads.get(), 6);
    }

    #[test]
    fn one_table_read_per_namespace_per_poll() {
        let mut fake = listening_process(7, 1, 42);
        fake.netns.insert(8, 1);
        fake.inodes.insert(8, HashSet::from([42]));
        let mut queues = AcceptQueues::with(fake, 30);
        set_queue(&queues, 1, 42, 3);
        // Two workers of one container share the listening socket.
        assert_eq!(queues.listeners(7), vec![(42, 3)]);
        assert_eq!(queues.listeners(8), vec![(42, 3)]);
        assert_eq!(queues.proc.table_reads.get(), 1);
        queues.end_poll();
        queues.listeners(7);
        assert_eq!(queues.proc.table_reads.get(), 2);
    }

    #[test]
    fn fd_walks_are_spread_over_the_polls() {
        let mut fake = Fake::default();
        for tgid in 1..=3 {
            fake.netns.insert(tgid, 1);
            fake.inodes.insert(tgid, HashSet::from([u64::from(tgid)]));
        }
        let mut queues = AcceptQueues::with(fake, 3);
        let mut walks_per_poll = Vec::new();
        for _ in 0..7 {
            let before = queues.proc.walks.borrow().len();
            for tgid in 1..=3 {
                queues.listeners(tgid);
            }
            walks_per_poll.push(queues.proc.walks.borrow().len() - before);
            queues.end_poll();
        }
        // All three are new at poll 0; afterwards one walk per poll, never all three in the same poll again.
        assert_eq!(walks_per_poll, vec![3, 1, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn a_process_without_a_listener_reads_no_table() {
        let mut fake = Fake::default();
        fake.netns.insert(9, 1);
        let mut queues = AcceptQueues::with(fake, 30);
        assert_eq!(queues.listeners(9), vec![]);
        assert_eq!(queues.proc.table_reads.get(), 0);
    }

    #[test]
    fn a_closed_listener_drops_out_before_the_next_walk() {
        let mut queues = AcceptQueues::with(listening_process(7, 1, 42), 30);
        set_queue(&queues, 1, 42, 2);
        assert_eq!(queues.listeners(7), vec![(42, 2)]);
        queues.end_poll();
        queues.proc.tables.borrow_mut().clear();
        assert_eq!(queues.listeners(7), vec![]);
    }

    #[test]
    fn a_process_gone_for_a_poll_starts_over() {
        let mut queues = AcceptQueues::with(listening_process(7, 1, 42), 30);
        queues.listeners(7);
        queues.end_poll();
        queues.end_poll();
        queues.listeners(7);
        assert_eq!(queues.proc.walks.borrow().len(), 2);
    }
}
