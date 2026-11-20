//! Counting failed boots, so a broken install cannot loop forever.
//!
//! The counter's failure mode is deliberately safe: a counter that cannot be read is treated as
//! zero, so a damaged state directory gives the normal boot one more try rather than locking the
//! user out of the machine entirely.

use std::path::{Path, PathBuf};

/// How many failed boots are tolerated before the maintenance path is taken.
pub const MAX_ATTEMPTS: u32 = 3;

/// Which path the next boot takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The normal path: start the guest and show macOS.
    Normal,
    /// The maintenance path: a text console, because the normal path has failed too often.
    Maintenance,
}

/// The counter, and where it lives.
#[derive(Debug, Clone)]
pub struct Attempts {
    path: PathBuf,
    count: u32,
}

impl Attempts {
    /// Reads the counter at `path`, treating an unreadable or malformed file as zero.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let count = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        Attempts { path, count }
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The path this boot takes.
    pub fn mode(&self) -> Mode {
        if self.count >= MAX_ATTEMPTS {
            Mode::Maintenance
        } else {
            Mode::Normal
        }
    }

    /// Records that a boot is being attempted, before the guest is started.
    ///
    /// Writing before the start is what makes a hard crash visible: the increment is already on
    /// disk when the machine dies, so the next boot sees it.
    pub fn record_attempt(&mut self) -> Result<(), String> {
        self.count = self.count.saturating_add(1);
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        // Write to a sibling and rename, so a crash mid-write leaves the old count rather than a
        // truncated file that reads as zero.
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, self.count.to_string())
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("{}: {e}", self.path.display()))?;
        Ok(())
    }

    /// Clears the counter: the guest came up, so the boot succeeded.
    pub fn record_success(&mut self) -> Result<(), String> {
        self.count = 0;
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", self.path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "oem-attempts-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_fresh_host_boots_normally() {
        let a = Attempts::load(temp("fresh").join("attempts"));
        assert_eq!(a.count(), 0);
        assert_eq!(a.mode(), Mode::Normal);
    }

    #[test]
    fn attempts_accumulate_until_maintenance() {
        let p = temp("count").join("attempts");
        let mut a = Attempts::load(&p);
        for expected in 1..=MAX_ATTEMPTS {
            a.record_attempt().expect("record");
            assert_eq!(a.count(), expected);
        }
        assert_eq!(a.mode(), Mode::Maintenance);
        // The count survives a reload, which is the whole point.
        assert_eq!(Attempts::load(&p).mode(), Mode::Maintenance);
    }

    #[test]
    fn a_successful_boot_clears_the_counter() {
        let p = temp("clear").join("attempts");
        let mut a = Attempts::load(&p);
        a.record_attempt().unwrap();
        a.record_attempt().unwrap();
        assert_eq!(a.count(), 2);
        a.record_success().unwrap();
        assert_eq!(a.count(), 0);
        assert_eq!(Attempts::load(&p).count(), 0);
        // Clearing an already-clear counter is not an error.
        a.record_success().unwrap();
    }

    /// A damaged counter must not lock the user out of the machine.
    #[test]
    fn a_damaged_counter_reads_as_zero() {
        let p = temp("damaged").join("attempts");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "not a number").unwrap();
        assert_eq!(Attempts::load(&p).count(), 0);
        assert_eq!(Attempts::load(&p).mode(), Mode::Normal);
        std::fs::write(&p, "").unwrap();
        assert_eq!(Attempts::load(&p).count(), 0);
    }

    /// The increment is on disk before the caller continues, so a hard power loss still counts.
    #[test]
    fn the_attempt_is_written_immediately() {
        let p = temp("immediate").join("attempts");
        let mut a = Attempts::load(&p);
        a.record_attempt().unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap().trim(), "1");
    }

    #[test]
    fn the_counter_survives_a_saturating_overflow() {
        let p = temp("overflow").join("attempts");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, u32::MAX.to_string()).unwrap();
        let mut a = Attempts::load(&p);
        a.record_attempt().unwrap();
        assert_eq!(a.count(), u32::MAX);
        assert_eq!(a.mode(), Mode::Maintenance);
    }
}
