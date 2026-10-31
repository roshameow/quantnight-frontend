//! The existing taskctl/continuation advisory lock, not a second lock registry.
//! Files deliberately survive unlock: unlinking would split lock ownership.
use std::{fs::{File, OpenOptions}, path::Path};

pub struct MissionLocks {
    // Reverse release order, like dataset_mission.mission_mutator_locks.
    files: Vec<File>,
}

impl MissionLocks {
    pub fn acquire(root: &Path, names: &[&str], shared: bool) -> Result<Self, String> {
        Self::try_acquire(root, names, shared)?
            .ok_or_else(|| "another lifecycle mutator owns this mission".into())
    }

    /// Same inode and nonblocking flock. Busy is not an IO failure and callers
    /// must not interpret it as evidence that a generation owner has died.
    pub fn try_acquire(root: &Path, names: &[&str], shared: bool) -> Result<Option<Self>, String> {
        if names.is_empty() || names.iter().any(|name| {
            name.is_empty() || name.len() > 134
                || !name.as_bytes()[0].is_ascii_alphanumeric()
                || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        }) {
            return Err("invalid lifecycle mission names".into());
        }
        let root = root.canonicalize().map_err(|e| format!("mission root unavailable: {e}"))?;
        let directory = root.join(".quantnight/mutators");
        std::fs::create_dir_all(&directory).map_err(|e| format!("mission lock directory: {e}"))?;
        let mut names = names.to_vec();
        names.sort_unstable();
        names.dedup();
        let mut guard = Self { files: Vec::new() };
        for name in names {
            let file = OpenOptions::new().create(true).append(true)
                .open(directory.join(format!("{name}.lock")))
                .map_err(|e| format!("mission lock open: {e}"))?;
            if !lock(&file, shared)? { return Ok(None); }
            guard.files.push(file);
        }
        Ok(Some(guard))
    }
}

#[cfg(unix)]
fn lock(file: &File, shared: bool) -> Result<bool, String> {
    use std::os::fd::AsRawFd;
    let operation = (if shared { libc::LOCK_SH } else { libc::LOCK_EX }) | libc::LOCK_NB;
    // SAFETY: the File owns a live fd for this entire call. flock stores no pointer.
    if unsafe { libc::flock(file.as_raw_fd(), operation) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            return Ok(false);
        }
        return Err(format!("mission lock failed: {error}"));
    }
    Ok(true)
}

#[cfg(not(unix))]
fn lock(_: &File, _: bool) -> Result<bool, String> {
    Err("the original lifecycle requires Unix flock; no alternate lock namespace is available".into())
}

impl Drop for MissionLocks {
    fn drop(&mut self) {
        while let Some(file) = self.files.pop() {
            #[cfg(unix)] {
                use std::os::fd::AsRawFd;
                // SAFETY: still-owned descriptor; closing also releases the lock.
                unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN); }
            }
            drop(file);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct TemporaryRoot(std::path::PathBuf);
    impl TemporaryRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("qn-mission-lock-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&path).unwrap(); Self(path)
        }
    }
    impl Drop for TemporaryRoot {
        fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); }
    }
    #[test]
    fn same_inode_exclusion_shared_readers_and_no_unlink() {
        use std::os::unix::fs::MetadataExt;
        let root = TemporaryRoot::new();
        let first = MissionLocks::acquire(&root.0, &["alpha"], false).unwrap();
        let path = root.0.join(".quantnight/mutators/alpha.lock");
        let inode = path.metadata().unwrap().ino();
        assert!(MissionLocks::acquire(&root.0, &["alpha"], false).is_err());
        assert!(MissionLocks::acquire(&root.0, &["alpha"], true).is_err());
        drop(first);
        let reader_a = MissionLocks::acquire(&root.0, &["alpha"], true).unwrap();
        let reader_b = MissionLocks::acquire(&root.0, &["alpha"], true).unwrap();
        assert!(MissionLocks::acquire(&root.0, &["alpha"], false).is_err());
        drop((reader_a, reader_b));
        let _writer = MissionLocks::acquire(&root.0, &["alpha"], false).unwrap();
        assert_eq!(path.metadata().unwrap().ino(), inode);
    }
    #[test]
    fn partial_acquisition_releases_only_owned_locks() {
        let root = TemporaryRoot::new();
        let held_b = MissionLocks::acquire(&root.0, &["b"], false).unwrap();
        assert!(MissionLocks::acquire(&root.0, &["b", "a", "a"], false).is_err());
        let _a = MissionLocks::acquire(&root.0, &["a"], false).unwrap();
        assert!(MissionLocks::acquire(&root.0, &["b"], false).is_err());
        drop(held_b);
        let _b = MissionLocks::acquire(&root.0, &["b"], false).unwrap();
    }
    #[test]
    fn python_flock_observes_the_same_owned_inode() {
        // Only stdlib fcntl and a synthetic directory; no project imports,
        // database, shell runner, credentials or platform operations.
        let root = TemporaryRoot::new();
        let path = root.0.join(".quantnight/mutators/alpha.lock");
        let lock = MissionLocks::acquire(&root.0, &["alpha"], false).unwrap();
        let python = std::env::var_os("QUANTNIGHT_TEST_PYTHON").unwrap_or_else(|| "python3".into());
        let probe = || std::process::Command::new(&python)
            .args(["-I", "-S", "-c", "import fcntl,sys\nf=open(sys.argv[1], 'a')\ntry: fcntl.flock(f, fcntl.LOCK_EX|fcntl.LOCK_NB)\nexcept BlockingIOError: sys.exit(23)\n"])
            .arg(&path).status().expect("stdlib Python required for the existing flock interop test");
        assert_eq!(probe().code(), Some(23));
        drop(lock);
        assert!(probe().success());
        assert!(path.exists(), "the shared namespace must never be unlinked");
    }
    #[test]
    fn invalid_names_cannot_escape_root() {
        let root = TemporaryRoot::new();
        for invalid in ["", "../escape", "a/b", "-leading", "汉字", "a.lock\n"] {
            assert!(MissionLocks::acquire(&root.0, &[invalid], false).is_err());
        }
        assert!(MissionLocks::acquire(&root.0, &[], false).is_err());
        assert!(!root.0.join(".quantnight").exists());
    }
}
