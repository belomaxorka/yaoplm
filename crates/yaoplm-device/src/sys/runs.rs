//! Counting runs of contiguous space from a file's extents, the way OPL's
//! BDM driver counts fragments: it walks the cluster chain and starts a new
//! fragment wherever the next cluster does not follow the current one
//! (`get_frag_list` in `iop/fs/bdmfs_fatfs/src/fs_driver.c`,
//! ps2dev/ps2sdk@329e2bb4; research/06 §1.11).

/// Extents in file order; units are whatever the OS reports (clusters,
/// blocks or bytes), as long as start and length use the same one.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Runs {
    count: u32,
    /// Where the last extent ended on the device; `None` after a hole.
    end: Option<u64>,
}

impl Runs {
    /// An extent of `len` units at `start` on the device. `None` is a hole
    /// (sparse or compressed), never contiguous with anything.
    pub(crate) fn add(&mut self, start: Option<u64>, len: u64) {
        if start.is_none() || start != self.end {
            self.count = self.count.saturating_add(1);
        }
        self.end = start.map(|s| s.saturating_add(len));
    }

    pub(crate) fn count(self) -> u32 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::Runs;

    #[test]
    fn extents_that_continue_on_disk_are_one_run() {
        let mut runs = Runs::default();
        assert_eq!(runs.count(), 0);
        // 0..16 at 100, 16..32 at 116 (continues), 32..40 at 500.
        runs.add(Some(100), 16);
        runs.add(Some(116), 16);
        runs.add(Some(500), 8);
        assert_eq!(runs.count(), 2);
        // A hole is a run of its own and breaks contiguity.
        runs.add(None, 2);
        runs.add(Some(510), 4);
        assert_eq!(runs.count(), 4);
        // Going back on the device is a new run too.
        runs.add(Some(0), 4);
        assert_eq!(runs.count(), 5);
    }
}
