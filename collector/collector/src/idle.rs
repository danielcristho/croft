//! Thins the rows of idle processes to a heartbeat; an idle process writes the same row every poll.

use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
struct Written {
    poll: u64,
    idle: bool,
}

pub struct IdleThrottle {
    every: u64,
    poll: u64,
    written: HashMap<u32, Written>,
    seen: HashSet<u32>,
}

// IdleThrottle keeps track of which processes have been written recently, and thins the rows of idle processes to a heartbeat.
impl IdleThrottle {
    pub fn new(every: u64) -> Self {
        Self {
            every,
            poll: 0,
            written: HashMap::new(),
            seen: HashSet::new(),
        }
    }

    // should_write keeps every active row, the first idle row after activity, and one idle row every `every` polls.
    pub fn should_write(&mut self, tgid: u32, idle: bool) -> bool {
        self.seen.insert(tgid);
        let write = match self.written.get(&tgid) {
            None => true,
            Some(last) => !idle || !last.idle || self.poll - last.poll >= self.every,
        };
        if write {
            self.written.insert(
                tgid,
                Written {
                    poll: self.poll,
                    idle,
                },
            );
        }
        write
    }

    // end_poll increments the poll counter and removes any processes that were not seen in this poll from the written map.
    pub fn end_poll(&mut self) {
        self.poll += 1;
        let seen = std::mem::take(&mut self.seen);
        self.written.retain(|tgid, _| seen.contains(tgid));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(throttle: &mut IdleThrottle, idle: &[bool]) -> Vec<bool> {
        idle.iter()
            .map(|&idle| {
                let write = throttle.should_write(7, idle);
                throttle.end_poll();
                write
            })
            .collect()
    }

    #[test]
    fn idle_processes_write_a_heartbeat() {
        let mut throttle = IdleThrottle::new(3);
        let idle = [true, true, true, true, true, true, true];
        assert_eq!(
            run(&mut throttle, &idle),
            [true, false, false, true, false, false, true]
        );
    }

    #[test]
    fn activity_is_written_and_the_first_idle_poll_after_it() {
        let mut throttle = IdleThrottle::new(3);
        let idle = [true, false, false, true, true, true, true];
        assert_eq!(
            run(&mut throttle, &idle),
            [true, true, true, true, false, false, true]
        );
    }

    #[test]
    fn a_process_gone_for_a_poll_starts_over() {
        let mut throttle = IdleThrottle::new(3);
        assert!(throttle.should_write(7, true));
        throttle.end_poll();
        throttle.end_poll();
        assert!(throttle.should_write(7, true));
    }
}
