//! Warns when the eBPF side loses map inserts.

use aya::Ebpf;
use collector_common::DropSite;
use log::warn;

use crate::sensing::maps;

#[derive(Default)]
pub struct DropWatcher {
    reported: [u64; DropSite::ALL.len()],
}

// DropWatcher tracks the total number of inserts refused by the eBPF side, and warns when new drops are detected.
impl DropWatcher {
    pub fn warn_new(&mut self, ebpf: &Ebpf) -> anyhow::Result<()> {
        let drops = maps::drops(ebpf)?;
        for site in DropSite::ALL {
            let total = drops.get(&site.index(), 0)?.iter().sum();
            if let Some(new) = self.new_since_last(site, total) {
                warn!("{} refused {new} inserts (total {total})", site.map_name());
            }
        }
        Ok(())
    }

    // totals lists each site's map name with its drops since the collector started, as last read.
    pub fn totals(&self) -> impl Iterator<Item = (&'static str, u64)> + '_ {
        DropSite::ALL
            .iter()
            .map(|&site| (site.map_name(), self.reported[site.index() as usize]))
    }

    fn new_since_last(&mut self, site: DropSite, total: u64) -> Option<u64> {
        let reported = &mut self.reported[site.index() as usize];
        let new = total.checked_sub(*reported).filter(|&n| n > 0);
        *reported = total;
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_each_drop_once() {
        let mut watcher = DropWatcher::default();
        assert_eq!(watcher.new_since_last(DropSite::GpuActivity, 3), Some(3));
        assert_eq!(watcher.new_since_last(DropSite::GpuActivity, 3), None);
        assert_eq!(watcher.new_since_last(DropSite::GpuActivity, 5), Some(2));
    }

    #[test]
    fn totals_hold_every_site_as_last_read() {
        let mut watcher = DropWatcher::default();
        watcher.new_since_last(DropSite::Connects, 4);
        let totals: Vec<_> = watcher.totals().collect();
        assert_eq!(totals.len(), DropSite::ALL.len());
        assert!(totals.contains(&("CONNECTS", 4)));
        assert!(totals.contains(&("GPU_FDS", 0)));
    }

    #[test]
    fn sites_are_tracked_apart() {
        let mut watcher = DropWatcher::default();
        watcher.new_since_last(DropSite::GpuActivity, 3);
        assert_eq!(watcher.new_since_last(DropSite::GpuFd, 1), Some(1));
    }
}
