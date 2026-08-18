use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{InstallError, Result};

#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Cancel::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            return Err(InstallError::Cancelled);
        }
        Ok(())
    }
}
