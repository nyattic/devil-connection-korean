#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub enum Event {
    Step {
        index: u32,
        total: u32,
        message: String,
    },
    Message {
        level: Level,
        text: String,
    },
    Progress {
        label: String,
        done: u64,
        total: u64,
    },
}

pub trait Reporter {
    fn report(&self, event: Event);
}

pub struct SilentReporter;

impl Reporter for SilentReporter {
    fn report(&self, _event: Event) {}
}

impl<F> Reporter for F
where
    F: Fn(Event),
{
    fn report(&self, event: Event) {
        self(event)
    }
}

pub(crate) fn info(reporter: &dyn Reporter, text: impl Into<String>) {
    reporter.report(Event::Message {
        level: Level::Info,
        text: text.into(),
    });
}

pub(crate) fn success(reporter: &dyn Reporter, text: impl Into<String>) {
    reporter.report(Event::Message {
        level: Level::Success,
        text: text.into(),
    });
}

pub(crate) fn warn(reporter: &dyn Reporter, text: impl Into<String>) {
    reporter.report(Event::Message {
        level: Level::Warning,
        text: text.into(),
    });
}
