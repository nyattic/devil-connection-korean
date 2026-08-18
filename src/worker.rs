use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use dc_installer::{Event, InstallConfig, InstallError, InstallReport, Reporter, install, restore};

pub enum Job {
    Install(InstallConfig),
    Restore(PathBuf),
}

pub enum Msg {
    Progress(Event),
    Done(Result<Outcome, Failure>),
}

pub enum Outcome {
    Installed(Box<InstallReport>),
    Restored,
}

pub struct Failure {
    pub message: String,
    pub game_intact: bool,
    pub cancelled: bool,
}

impl Failure {
    fn from_install(error: InstallError) -> Self {
        Failure {
            message: error.to_string(),
            game_intact: error.leaves_game_intact(),
            cancelled: error.is_cancelled(),
        }
    }

    fn from_restore(error: InstallError) -> Self {
        let game_intact = !matches!(
            error,
            InstallError::Io { .. } | InstallError::RollbackFailed { .. }
        );
        Failure {
            message: error.to_string(),
            game_intact,
            cancelled: false,
        }
    }
}

struct ChannelReporter {
    tx: Sender<Msg>,
    ctx: egui::Context,
}

impl Reporter for ChannelReporter {
    fn report(&self, event: Event) {
        let _ = self.tx.send(Msg::Progress(event));
        self.ctx.request_repaint();
    }
}

pub fn spawn(job: Job, ctx: egui::Context) -> Receiver<Msg> {
    let (tx, rx) = mpsc::channel();
    let reporter = ChannelReporter {
        tx: tx.clone(),
        ctx: ctx.clone(),
    };

    std::thread::spawn(move || {
        let result = match job {
            Job::Install(config) => install(&config, &reporter)
                .map(|report| Outcome::Installed(Box::new(report)))
                .map_err(Failure::from_install),
            Job::Restore(asar) => restore(&asar, &reporter)
                .map(|()| Outcome::Restored)
                .map_err(Failure::from_restore),
        };

        let _ = tx.send(Msg::Done(result));
        ctx.request_repaint();
    });

    rx
}
