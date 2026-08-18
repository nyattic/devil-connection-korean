use crate::error::{AsarError, Result};

const TICK_INTERVAL: u64 = 64;

pub trait Observer {
    fn advance(&self, task: &str, done: u64, total: u64);

    fn cancelled(&self) -> bool;
}

pub struct Ignore;

impl Observer for Ignore {
    fn advance(&self, _task: &str, _done: u64, _total: u64) {}

    fn cancelled(&self) -> bool {
        false
    }
}

pub(crate) struct Ticker<'a> {
    observer: &'a dyn Observer,
    task: &'static str,
    total: u64,
    done: u64,
}

impl<'a> Ticker<'a> {
    pub(crate) fn new(observer: &'a dyn Observer, task: &'static str, total: u64) -> Self {
        Ticker {
            observer,
            task,
            total,
            done: 0,
        }
    }

    pub(crate) fn tick(&mut self) -> Result<()> {
        self.done += 1;
        if self.done.is_multiple_of(TICK_INTERVAL) || self.done == self.total {
            self.observer.advance(self.task, self.done, self.total);
        }
        if self.observer.cancelled() {
            return Err(AsarError::Cancelled);
        }
        Ok(())
    }
}
