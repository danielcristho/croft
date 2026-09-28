//! Syscall counts from SYSCALL_BY_TYPE: grouping, naming and rates.

mod names;
mod rates;

use std::collections::HashMap;

use collector_common::unpack_key;
pub use rates::{RateTracker, SyscallRate};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyscallCount {
    pub nr: u32,
    pub count: u64,
}

pub fn group_by_tgid(entries: impl Iterator<Item = (u64, u64)>) -> HashMap<u32, Vec<SyscallCount>> {
    let mut by_tgid: HashMap<u32, Vec<SyscallCount>> = HashMap::new();
    for (key, count) in entries {
        let (tgid, nr) = unpack_key(key);
        by_tgid
            .entry(tgid)
            .or_default()
            .push(SyscallCount { nr, count });
    }
    by_tgid
}

#[cfg(test)]
mod tests {
    use collector_common::pack_key;

    use super::*;

    #[test]
    fn entries_group_under_their_tgid() {
        let entries = [
            (pack_key(1, 16), 5),
            (pack_key(2, 0), 7),
            (pack_key(1, 0), 3),
        ];
        let grouped = group_by_tgid(entries.into_iter());
        assert_eq!(
            grouped[&1],
            vec![
                SyscallCount { nr: 16, count: 5 },
                SyscallCount { nr: 0, count: 3 }
            ]
        );
        assert_eq!(grouped[&2], vec![SyscallCount { nr: 0, count: 7 }]);
    }
}
