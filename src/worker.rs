use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use dc_installer::{Event, InstallConfig, InstallReport, Reporter, install, restore};

pub enum Job {
    Install(InstallConfig),
    Restore(PathBuf),
}

pub enum Msg {
    Progress(Event),
    Done(Result<Outcome, String>),
}

pub enum Outcome {
    Installed(Box<InstallReport>),
    Restored,
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
                .map_err(|e| e.to_string()),
            Job::Restore(asar) => restore(&asar, &reporter)
                .map(|()| Outcome::Restored)
                .map_err(|e| e.to_string()),
        };

        let _ = tx.send(Msg::Done(result));
        ctx.request_repaint();
    });

    rx
}
