//! A working directory per test, removed when the test ends.
//!
//! Gateway regressions write journals, locks and checkpoints to real files, and
//! they used to share one directory per process id keyed on nothing else.
//! Operating system pids are recycled and `ModelGateway::open` resumes any
//! journal it finds, so a run that landed on a recycled pid could inherit a
//! crashed run's spend record and fail for a cause that had nothing to do with
//! the test. That happened: a mutation run read 45 where it expected 15.
//!
//! Every directory here carries a counter and a nanosecond stamp as well as the
//! pid, so no two runs can name the same one, and the cleanup is a `Drop` that
//! a panicking test still runs. A test that deliberately leaks a lock leaks it
//! into a directory nothing else will ever open.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A directory that exists for as long as this value does.
#[derive(Debug)]
pub(crate) struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    pub(crate) fn new(tag: &str) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "sb-{tag}-{}-{}-{stamp:x}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).ok();
    }
}
