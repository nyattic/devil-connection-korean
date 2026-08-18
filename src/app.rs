use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use dc_installer::{
    Cancel, Event, InstallConfig, Level, PATCH_DIRS, STEPS, TranslationSource, backup_path,
    detect_game_dirs, locate_asar,
};
use egui::{
    Align, Align2, Color32, CornerRadius, LayerId, Layout, Order, Rect, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, TextureHandle, pos2, vec2,
};

use crate::theme;
use crate::worker::{self, Failure, Job, Msg, Outcome};

const COLUMN_WIDTH: f32 = 540.0;
const ICON_SIZE: f32 = 46.0;

pub const WINDOW_HEIGHT: f32 = 760.0;
const LOG_MIN_HEIGHT: f32 = 80.0;
const LOG_CHROME: f32 = 60.0;

const FONT_NOTICE: &str = "이 패치는 ㈜넥슨코리아의 메이플스토리 서체를 사용합니다. 서체의 지적 재산권은 ㈜넥슨코리아에 있습니다.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Running { step: u32 },
    Restoring,
    Done,
    Failed { intact: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Good,
    Bad,
    Neutral,
}

enum Hint {
    None,
    Ok(String),
    Bad(String),
}

struct Notice {
    title: String,
    body: String,
    footnote: Option<String>,
    tone: Tone,
}

enum Dialog {
    Notice(Notice),
    ConfirmRestore,
}

pub struct InstallerApp {
    embedded: Option<&'static [u8]>,
    icon: Option<TextureHandle>,

    game_path: String,
    data_path: String,
    game_hint: Hint,
    data_hint: Hint,
    patched: bool,
    detected: Vec<PathBuf>,
    checked_game: String,
    checked_data: String,
    hints_dirty: bool,

    log: Vec<(Level, String)>,
    phase: Phase,
    detail: Option<String>,
    step_fraction: f32,

    rx: Option<Receiver<Msg>>,
    cancel: Cancel,
    dialog: Option<Dialog>,
}

impl InstallerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::install_style(&cc.egui_ctx);

        let mut app = InstallerApp {
            embedded: crate::embedded::translation(),
            icon: load_icon(&cc.egui_ctx),
            game_path: String::new(),
            data_path: String::new(),
            game_hint: Hint::None,
            data_hint: Hint::None,
            patched: false,
            detected: Vec::new(),
            checked_game: String::new(),
            checked_data: String::new(),
            hints_dirty: true,
            log: Vec::new(),
            phase: Phase::Idle,
            detail: None,
            step_fraction: 0.0,
            rx: None,
            cancel: Cancel::new(),
            dialog: None,
        };

        if app.embedded.is_none()
            && let Some(dir) = dc_installer::find_data_dir()
        {
            app.data_path = dir.display().to_string();
        }
        app.autodetect_game(false);
        app
    }

    fn busy(&self) -> bool {
        self.rx.is_some()
    }

    fn restoring(&self) -> bool {
        self.phase == Phase::Restoring
    }

    fn log(&mut self, level: Level, text: impl Into<String>) {
        self.log.push((level, text.into()));
    }

    fn info(&mut self, text: impl Into<String>) {
        self.log(Level::Info, text);
    }

    fn autodetect_game(&mut self, announce: bool) {
        let found: Vec<PathBuf> = detect_game_dirs()
            .into_iter()
            .filter(|dir| locate_asar(dir).is_ok())
            .collect();

        if found.is_empty() {
            self.detected.clear();
            if announce {
                self.log(Level::Warning, "설치된 게임을 찾지 못했습니다.");
                self.dialog = Some(Dialog::Notice(Notice {
                    title: "게임을 찾지 못했습니다".to_string(),
                    body: "Steam 라이브러리에서 게임을 찾지 못했습니다. '찾아보기'로 게임이 설치된 폴더를 직접 지정해주세요.".to_string(),
                    footnote: None,
                    tone: Tone::Neutral,
                }));
            }
            return;
        }

        let current = self.game_path.trim();
        let next = found
            .iter()
            .position(|dir| dir.display().to_string() == current)
            .map(|index| (index + 1) % found.len())
            .unwrap_or(0);

        self.game_path = found[next].display().to_string();
        self.hints_dirty = true;

        if announce {
            let message = if found.len() > 1 {
                format!(
                    "게임을 찾았습니다: {} ({}곳 중 {}번째)",
                    self.game_path,
                    found.len(),
                    next + 1
                )
            } else {
                format!("게임을 찾았습니다: {}", self.game_path)
            };
            self.log(Level::Success, message);
        }

        self.detected = found;
    }

    fn refresh_hints(&mut self) {
        if self.hints_dirty || self.checked_game != self.game_path {
            self.checked_game = self.game_path.clone();
            let (hint, patched) = check_game(&self.game_path);
            self.game_hint = hint;
            self.patched = patched;
        }
        if self.hints_dirty || self.checked_data != self.data_path {
            self.checked_data = self.data_path.clone();
            self.data_hint = check_data(&self.data_path);
        }
        self.hints_dirty = false;
    }

    fn ready(&self) -> bool {
        matches!(self.game_hint, Hint::Ok(_))
            && (self.embedded.is_some() || matches!(self.data_hint, Hint::Ok(_)))
    }

    fn source(&self) -> TranslationSource {
        match self.embedded {
            Some(bytes) => TranslationSource::Embedded(bytes),
            None => TranslationSource::Directory(PathBuf::from(self.data_path.trim())),
        }
    }

    fn status(&self) -> (String, Color32) {
        match self.phase {
            Phase::Running { step } => {
                let label = STEPS
                    .get(step as usize - 1)
                    .map(|info| info.label)
                    .unwrap_or("진행 중");
                match &self.detail {
                    Some(detail) => (format!("{label} · {detail}"), theme::TEXT),
                    None => (label.to_string(), theme::TEXT),
                }
            }
            Phase::Restoring => ("원본으로 되돌리는 중".to_string(), theme::TEXT),
            Phase::Done => ("설치를 마쳤습니다".to_string(), theme::SUCCESS),
            Phase::Failed { intact: true } => (
                "설치를 멈췄습니다. 게임 파일은 그대로입니다".to_string(),
                theme::ERROR,
            ),
            Phase::Failed { intact: false } => (
                "원본 복구에 실패했습니다. 진행 기록을 확인해주세요".to_string(),
                theme::ERROR,
            ),
            Phase::Idle if self.ready() => ("설치할 준비가 됐습니다".to_string(), theme::MUTED),
            Phase::Idle if self.embedded.is_some() => {
                ("게임 폴더를 확인해주세요".to_string(), theme::MUTED)
            }
            Phase::Idle => (
                "게임 폴더와 번역 데이터를 확인해주세요".to_string(),
                theme::MUTED,
            ),
        }
    }

    fn progress(&self) -> f32 {
        match self.phase {
            Phase::Running { step } => {
                let total: u32 = STEPS.iter().map(|info| info.weight).sum();
                let index = step as usize - 1;
                let done: u32 = STEPS.iter().take(index).map(|info| info.weight).sum();
                let current = STEPS.get(index).map(|info| info.weight).unwrap_or(0);
                (done as f32 + current as f32 * self.step_fraction) / total as f32
            }
            Phase::Done => 1.0,
            _ => 0.0,
        }
    }

    fn start_install(&mut self, ctx: &egui::Context) {
        let asar = match locate_asar(&PathBuf::from(self.game_path.trim())) {
            Ok(path) => path,
            Err(e) => return self.refuse(e.to_string()),
        };

        self.log.clear();
        self.info(format!("패치 프로그램 v{}", env!("CARGO_PKG_VERSION")));
        self.info(format!("대상 {}", asar.display()));
        self.phase = Phase::Running { step: 1 };
        self.detail = None;
        self.step_fraction = 0.0;
        self.cancel = Cancel::new();
        self.rx = Some(worker::spawn(
            Job::Install(InstallConfig {
                asar_path: asar,
                source: self.source(),
                keep_work_dir: false,
                cancel: self.cancel.clone(),
            }),
            ctx.clone(),
        ));
    }

    fn start_restore(&mut self, ctx: &egui::Context) {
        let asar = match locate_asar(&PathBuf::from(self.game_path.trim())) {
            Ok(path) => path,
            Err(e) => return self.refuse(e.to_string()),
        };

        self.log.clear();
        self.info("백업에서 원본을 되돌립니다.");
        self.phase = Phase::Restoring;
        self.detail = None;
        self.rx = Some(worker::spawn(Job::Restore(asar), ctx.clone()));
    }

    fn refuse(&mut self, message: String) {
        self.log(Level::Error, message.clone());
        self.dialog = Some(Dialog::Notice(Notice {
            title: "시작할 수 없습니다".to_string(),
            body: message,
            footnote: None,
            tone: Tone::Bad,
        }));
    }

    fn drain_worker(&mut self) {
        let Some(rx) = self.rx.as_ref() else {
            return;
        };

        let mut finished = false;
        for message in rx.try_iter().collect::<Vec<_>>() {
            match message {
                Msg::Progress(Event::Step { index, message, .. }) => {
                    self.phase = Phase::Running { step: index };
                    self.detail = None;
                    self.step_fraction = 0.0;
                    self.info(message);
                }
                Msg::Progress(Event::Message { level, text }) => self.log(level, text),
                Msg::Progress(Event::Progress { label, done, total }) => {
                    self.detail = Some(format!("{label} {done}/{total}"));
                    if total > 0 {
                        let fraction = done as f32 / total as f32;
                        self.step_fraction = self.step_fraction.max(fraction);
                    }
                }
                Msg::Done(result) => {
                    finished = true;
                    self.finish(result);
                }
            }
        }

        if finished {
            self.rx = None;
            self.detail = None;
        }
    }

    fn finish(&mut self, result: Result<Outcome, Failure>) {
        let restoring = self.restoring();
        self.hints_dirty = true;

        match result {
            Ok(Outcome::Installed(report)) => {
                self.phase = Phase::Done;
                self.log(
                    Level::Success,
                    format!(
                        "번역 파일 {}개 적용, {}개 검증 완료",
                        report.copied_files, report.verified_files
                    ),
                );
                self.info(format!("백업 {}", report.backup_path.display()));
                self.dialog = Some(Dialog::Notice(Notice {
                    title: "설치 완료".to_string(),
                    body: completion_message(),
                    footnote: Some(FONT_NOTICE.to_string()),
                    tone: Tone::Good,
                }));
            }
            Ok(Outcome::Restored) => {
                self.phase = Phase::Idle;
                self.log(Level::Success, "원본으로 되돌렸습니다.");
                self.dialog = Some(Dialog::Notice(Notice {
                    title: "되돌렸습니다".to_string(),
                    body: "게임이 패치 이전 상태로 돌아갔습니다.".to_string(),
                    footnote: None,
                    tone: Tone::Good,
                }));
            }
            Err(failure) if failure.cancelled => {
                self.phase = Phase::Idle;
                self.log(Level::Warning, failure.message);
                self.dialog = Some(Dialog::Notice(Notice {
                    title: "설치를 취소했습니다".to_string(),
                    body: "게임 파일은 손대지 않았습니다. 준비되면 다시 설치하세요.".to_string(),
                    footnote: None,
                    tone: Tone::Neutral,
                }));
            }
            Err(failure) => {
                self.phase = Phase::Failed {
                    intact: failure.game_intact,
                };
                self.log(Level::Error, failure.message.clone());

                let title = match (restoring, failure.game_intact) {
                    (true, true) => "되돌리지 못했습니다",
                    (true, false) => "되돌리는 중 멈췄습니다",
                    (false, true) => "설치를 멈췄습니다",
                    (false, false) => "원본 복구에 실패했습니다",
                };
                self.dialog = Some(Dialog::Notice(Notice {
                    title: title.to_string(),
                    body: failure.message,
                    footnote: None,
                    tone: Tone::Bad,
                }));
            }
        }
    }

    fn accept_drop(&mut self, path: PathBuf) {
        if locate_asar(&path).is_ok() || self.embedded.is_some() {
            self.game_path = path.display().to_string();
        } else if matches!(check_data(&path.display().to_string()), Hint::Ok(_)) {
            self.data_path = path.display().to_string();
        } else {
            self.game_path = path.display().to_string();
        }
        self.hints_dirty = true;
    }
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain_worker();
        self.refresh_hints();

        if self.busy() {
            ctx.request_repaint();
        }

        let idle = !self.busy() && self.dialog.is_none();
        if idle {
            let dropped = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .first()
                    .map(|file| file.path().to_path_buf())
            });
            if let Some(path) = dropped {
                self.accept_drop(path);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) && self.ready() {
                self.start_install(&ctx);
            }
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(28, 30)),
            )
            .show(ui, |ui| {
                let width = COLUMN_WIDTH.min(ui.available_width());
                let height = ui.available_height();
                let pad = ((ui.available_width() - width) * 0.5).max(0.0);

                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.add_space(pad);
                            ui.allocate_ui_with_layout(
                                vec2(width, height),
                                Layout::top_down(Align::Min),
                                |ui| self.column(ui, &ctx),
                            );
                        });
                    });
            });

        if idle && ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            drop_overlay(&ctx);
        }

        self.dialog_window(&ctx);
    }
}

impl InstallerApp {
    fn column(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.header(ui);
        ui.add_space(24.0);

        self.game_field(ui);

        ui.add_space(16.0);
        match self.embedded {
            Some(bytes) => self.bundled_data(ui, bytes.len()),
            None => self.data_field(ui),
        }

        ui.add_space(22.0);
        self.actions(ui, ctx);

        ui.add_space(22.0);
        self.progress_bar(ui);

        ui.add_space(20.0);
        self.log_panel(ui);

        ui.add_space(4.0);
        self.footer(ui);
    }

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(icon) = &self.icon {
                ui.add(egui::Image::new(egui::load::SizedTexture::new(
                    icon.id(),
                    vec2(ICON_SIZE, ICON_SIZE),
                )));
                ui.add_space(6.0);
            }
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("데빌 커넥션 한글패치")
                        .font(theme::semibold(theme::TITLE))
                        .color(theme::TEXT),
                );
                ui.add_space(3.0);
                ui.label(
                    RichText::new("でびるコネクショん")
                        .font(theme::regular(theme::BODY))
                        .color(theme::FAINT),
                );
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                chip(
                    ui,
                    &format!("v{}", env!("CARGO_PKG_VERSION")),
                    theme::MUTED,
                    theme::TRACK,
                );
            });
        });
    }

    fn section_label(&self, ui: &mut egui::Ui, text: &str) {
        ui.label(
            RichText::new(text)
                .font(theme::semibold(theme::BODY))
                .color(theme::TEXT),
        );
        ui.add_space(7.0);
    }

    fn game_field(&mut self, ui: &mut egui::Ui) {
        self.section_label(ui, "게임 폴더");

        let enabled = !self.busy();
        let mut browse = false;
        let mut redetect = false;

        ui.horizontal(|ui| {
            let button = 78.0;
            let gap = ui.spacing().item_spacing.x;
            let width = (ui.available_width() - (button + gap) * 2.0).max(120.0);

            ui.add_enabled(
                enabled,
                path_edit(&mut self.game_path, width, "게임이 설치된 폴더"),
            );
            browse = ui
                .add_enabled(enabled, secondary("찾아보기", button))
                .clicked();
            redetect = ui
                .add_enabled(enabled, secondary("다시 감지", button))
                .on_hover_text("Steam 라이브러리에서 게임을 다시 찾습니다")
                .clicked();
        });

        ui.add_space(7.0);
        ui.horizontal(|ui| {
            hint_label(ui, &self.game_hint);
            if matches!(self.game_hint, Hint::Ok(_)) {
                ui.add_space(2.0);
                if self.patched {
                    chip(ui, "패치 적용됨", theme::SUCCESS, theme::SUCCESS_SOFT);
                } else {
                    chip(ui, "원본", theme::MUTED, theme::TRACK);
                }
            }
        });

        if self.detected.len() > 1 {
            ui.add_space(5.0);
            ui.label(
                RichText::new(format!(
                    "이 컴퓨터에서 {}곳을 찾았습니다 · '다시 감지'로 전환합니다",
                    self.detected.len()
                ))
                .font(theme::regular(theme::CAPTION))
                .color(theme::MUTED),
            );
        }

        if browse
            && let Some(dir) = rfd::FileDialog::new()
                .set_title("게임이 설치된 폴더를 선택하세요")
                .pick_folder()
        {
            self.game_path = dir.display().to_string();
        }
        if redetect {
            self.autodetect_game(true);
        }
    }

    fn data_field(&mut self, ui: &mut egui::Ui) {
        self.section_label(ui, "번역 데이터");

        let enabled = !self.busy();
        let mut browse = false;

        ui.horizontal(|ui| {
            let button = 78.0;
            let gap = ui.spacing().item_spacing.x;
            let width = (ui.available_width() - button - gap).max(120.0);

            ui.add_enabled(
                enabled,
                path_edit(&mut self.data_path, width, "data/, tyrano/를 포함하는 폴더"),
            );
            browse = ui
                .add_enabled(enabled, secondary("찾아보기", button))
                .clicked();
        });

        ui.add_space(7.0);
        hint_label(ui, &self.data_hint);

        if browse
            && let Some(dir) = rfd::FileDialog::new()
                .set_title("번역 데이터 폴더를 선택하세요")
                .pick_folder()
        {
            self.data_path = dir.display().to_string();
        }
    }

    fn bundled_data(&self, ui: &mut egui::Ui, bytes: usize) {
        self.section_label(ui, "번역 데이터");
        ui.label(
            RichText::new(format!(
                "패치 프로그램에 포함되어 있습니다 · 폴더 {}개 · {}MB",
                PATCH_DIRS.len(),
                bytes / (1024 * 1024)
            ))
            .font(theme::regular(theme::BODY))
            .color(theme::SUCCESS),
        );
    }

    fn actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        const SECONDARY: f32 = 132.0;
        const GAP: f32 = 10.0;

        let width = ui.available_width();
        let primary_width = (width - SECONDARY - GAP).max(120.0);

        let mut install = false;
        let mut secondary_clicked = false;

        ui.allocate_ui_with_layout(
            vec2(width, 40.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = GAP;

                let can_install = !self.busy() && self.ready();
                let label = if self.restoring() {
                    "되돌리는 중"
                } else if self.busy() {
                    "설치 중"
                } else {
                    "설치 시작"
                };
                install = ui
                    .add_enabled(can_install, primary(label, primary_width, can_install))
                    .clicked();

                let (label, enabled, tooltip) = self.secondary_action();
                secondary_clicked = ui
                    .add_enabled(
                        enabled,
                        secondary(label, SECONDARY).min_size(vec2(SECONDARY, 40.0)),
                    )
                    .on_hover_text(tooltip)
                    .on_disabled_hover_text(tooltip)
                    .clicked();
            },
        );

        if install {
            self.start_install(ctx);
        }
        if secondary_clicked {
            if self.busy() {
                self.cancel.cancel();
                self.log(Level::Warning, "취소를 요청했습니다. 정리하는 중입니다…");
            } else {
                self.dialog = Some(Dialog::ConfirmRestore);
            }
        }
    }

    fn secondary_action(&self) -> (&'static str, bool, &'static str) {
        if self.restoring() {
            ("되돌리기", false, "되돌리는 중에는 멈출 수 없습니다")
        } else if self.busy() {
            (
                "취소",
                true,
                "진행 중인 설치를 멈춥니다. 게임 파일은 그대로입니다",
            )
        } else if !matches!(self.game_hint, Hint::Ok(_)) {
            ("되돌리기", false, "게임 폴더를 먼저 확인해주세요")
        } else if !self.patched {
            ("되돌리기", false, "되돌릴 백업이 없습니다")
        } else {
            (
                "되돌리기",
                true,
                "백업해 둔 원본 app.asar을 제자리에 돌려놓습니다",
            )
        }
    }

    fn progress_bar(&self, ui: &mut egui::Ui) {
        let (status, color) = self.status();
        let line = match self.phase {
            Phase::Running { step } => format!("{status}  ·  {step} / {}", STEPS.len()),
            _ => status,
        };
        ui.label(
            RichText::new(line)
                .font(theme::regular(theme::BODY))
                .color(color),
        );

        ui.add_space(9.0);

        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
        let radius = CornerRadius::same(2);
        let painter = ui.painter();
        painter.rect_filled(rect, radius, theme::TRACK);

        if self.restoring() {
            let time = ui.input(|i| i.time) as f32;
            let span = rect.width() * 0.3;
            let travel = rect.width() + span;
            let head = (time * 0.5).fract() * travel;
            let start = rect.min.x + (head - span).max(0.0);
            let end = (rect.min.x + head).min(rect.max.x);
            if end > start {
                painter.rect_filled(
                    Rect::from_min_max(pos2(start, rect.min.y), pos2(end, rect.max.y)),
                    radius,
                    theme::ACCENT,
                );
            }
            return;
        }

        let filled = self.progress();
        if filled > 0.0 {
            let color = if self.phase == Phase::Done {
                theme::SUCCESS
            } else {
                theme::ACCENT
            };
            let animated =
                ui.ctx()
                    .animate_value_with_time(egui::Id::new("dc-progress"), filled, 0.35);
            let width = (rect.width() * animated).max(4.0);
            painter.rect_filled(
                Rect::from_min_size(rect.min, vec2(width, rect.height())),
                radius,
                color,
            );
        }
    }

    fn log_panel(&self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("진행 기록")
                .font(theme::semibold(theme::BODY))
                .color(theme::TEXT),
        );
        ui.add_space(8.0);

        let height = (ui.available_height() - LOG_CHROME).max(LOG_MIN_HEIGHT);

        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(height);

                if self.log.is_empty() {
                    ui.label(
                        RichText::new("설치를 시작하면 진행 내용이 여기에 남습니다.")
                            .font(theme::regular(theme::BODY))
                            .color(theme::MUTED),
                    );
                    return;
                }

                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for (level, text) in &self.log {
                                ui.label(
                                    RichText::new(text)
                                        .font(theme::regular(theme::BODY))
                                        .line_height(Some(20.0))
                                        .color(level_color(*level)),
                                );
                            }
                        });
                });
            });
    }

    fn footer(&self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("제작 Nyabi · 번역 Oatone, 체퓨 · 이미지 토니 · 영상 민버드")
                .font(theme::regular(theme::CAPTION))
                .color(theme::FAINT),
        );
    }

    fn dialog_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.as_ref() else {
            return;
        };

        let mut close = false;
        let mut restore = false;

        let response = egui::Modal::new(egui::Id::new("dc-dialog"))
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ctx, |ui| {
                ui.set_max_width(400.0);

                match dialog {
                    Dialog::Notice(notice) => {
                        let accent = match notice.tone {
                            Tone::Good => theme::SUCCESS,
                            Tone::Bad => theme::ERROR,
                            Tone::Neutral => theme::TEXT,
                        };
                        ui.label(
                            RichText::new(&notice.title)
                                .font(theme::semibold(theme::SUBHEAD))
                                .color(accent),
                        );
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(&notice.body)
                                .font(theme::regular(theme::BODY))
                                .line_height(Some(20.0))
                                .color(theme::TEXT),
                        );

                        if let Some(footnote) = &notice.footnote {
                            ui.add_space(14.0);
                            ui.separator();
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(footnote)
                                    .font(theme::regular(theme::CAPTION))
                                    .line_height(Some(17.0))
                                    .color(theme::MUTED),
                            );
                        }

                        ui.add_space(18.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(secondary("확인", 80.0)).clicked() {
                                close = true;
                            }
                        });
                    }
                    Dialog::ConfirmRestore => {
                        ui.label(
                            RichText::new("원본으로 되돌릴까요?")
                                .font(theme::semibold(theme::SUBHEAD))
                                .color(theme::TEXT),
                        );
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(
                                "번역이 사라지고 패치 이전 상태로 돌아갑니다. 나중에 다시 설치할 수 있습니다.",
                            )
                            .font(theme::regular(theme::BODY))
                            .line_height(Some(20.0))
                            .color(theme::TEXT),
                        );
                        ui.add_space(18.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(primary("되돌리기", 110.0, true)).clicked() {
                                restore = true;
                            }
                            if ui.add(secondary("그만두기", 96.0)).clicked() {
                                close = true;
                            }
                        });
                    }
                }
            });

        if restore {
            self.dialog = None;
            self.start_restore(ctx);
            return;
        }
        if close || response.should_close() {
            self.dialog = None;
        }
    }
}

fn load_icon(ctx: &egui::Context) -> Option<TextureHandle> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    Some(ctx.load_texture("app-icon", image, egui::TextureOptions::LINEAR))
}

fn path_edit<'a>(value: &'a mut String, width: f32, hint: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(value)
        .desired_width(width)
        .margin(egui::Margin::symmetric(12, 10))
        .font(theme::regular(theme::BODY))
        .text_color(theme::TEXT)
        .hint_text(
            RichText::new(hint.to_owned())
                .font(theme::regular(theme::BODY))
                .color(theme::FAINT),
        )
}

fn hint_label(ui: &mut egui::Ui, hint: &Hint) {
    let (text, color) = match hint {
        Hint::None => return,
        Hint::Ok(text) => (text.as_str(), theme::SUCCESS),
        Hint::Bad(text) => (text.as_str(), theme::ERROR),
    };
    ui.label(
        RichText::new(text)
            .font(theme::regular(theme::BODY))
            .color(color),
    );
}

fn chip(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text.to_owned())
                    .font(theme::regular(theme::CAPTION))
                    .color(fg),
            );
        });
}

fn drop_overlay(ctx: &egui::Context) {
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, egui::Id::new("dc-drop")));
    let rect = ctx.viewport_rect();

    painter.rect_filled(
        rect,
        CornerRadius::ZERO,
        Color32::from_rgba_unmultiplied(0xfa, 0xfa, 0xfb, 0xf2),
    );
    painter.rect_stroke(
        rect.shrink(16.0),
        CornerRadius::same(12),
        Stroke::new(1.5, theme::ACCENT),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "폴더를 놓으면 경로가 채워집니다",
        theme::semibold(theme::SUBHEAD),
        theme::ACCENT,
    );
}

fn primary(text: &str, width: f32, enabled: bool) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_owned())
            .font(theme::semibold(theme::BODY))
            .color(if enabled {
                Color32::WHITE
            } else {
                theme::MUTED
            }),
    )
    .fill(if enabled { theme::ACCENT } else { theme::TRACK })
    .stroke(if enabled {
        Stroke::NONE
    } else {
        Stroke::new(1.0, theme::BORDER)
    })
    .corner_radius(CornerRadius::same(theme::RADIUS))
    .min_size(vec2(width, 40.0))
}

fn secondary(text: &str, width: f32) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_owned())
            .font(theme::regular(theme::BODY))
            .color(theme::TEXT),
    )
    .fill(theme::SURFACE)
    .stroke(Stroke::new(1.0, theme::BORDER_STRONG))
    .corner_radius(CornerRadius::same(theme::RADIUS))
    .min_size(vec2(width, 38.0))
}

fn level_color(level: Level) -> Color32 {
    match level {
        Level::Info => theme::MUTED,
        Level::Success => theme::SUCCESS,
        Level::Warning => theme::WARNING,
        Level::Error => theme::ERROR,
    }
}

fn check_game(path: &str) -> (Hint, bool) {
    let path = path.trim();
    if path.is_empty() {
        return (Hint::None, false);
    }

    let entered = PathBuf::from(path);
    match locate_asar(&entered) {
        Ok(asar) => {
            let patched = backup_path(&asar).is_file();
            (
                Hint::Ok(format!("확인됨 · {}", location_label(&entered, &asar))),
                patched,
            )
        }
        Err(e) => (Hint::Bad(e.to_string()), false),
    }
}

fn check_data(path: &str) -> Hint {
    let path = path.trim();
    if path.is_empty() {
        return Hint::None;
    }
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Hint::Bad("폴더가 없습니다.".to_string());
    }

    let missing: Vec<&str> = PATCH_DIRS
        .iter()
        .copied()
        .filter(|dir| !root.join(dir).is_dir())
        .collect();

    if missing.is_empty() {
        Hint::Ok(format!("확인됨 · 폴더 {}개", PATCH_DIRS.len()))
    } else {
        Hint::Bad(format!("빠진 폴더: {}", missing.join(", ")))
    }
}

fn location_label(entered: &Path, asar: &Path) -> String {
    entered
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| tail(asar, 2))
}

fn tail(path: &Path, count: usize) -> String {
    let parts: Vec<String> = path
        .components()
        .rev()
        .take(count)
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}

fn completion_message() -> String {
    let mut message = String::from(
        "Steam에서 게임을 실행하면 한국어로 표시됩니다. 되돌리려면 '되돌리기'를 누르세요.",
    );

    if cfg!(target_os = "macos") {
        message.push_str(
            "\n\n게임이 '손상되었습니다'라고 나오면 시스템 설정 > 개인정보 보호 및 보안에서 '그래도 열기'를 누르세요.",
        );
    }

    message
}
