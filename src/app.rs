use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use dc_installer::{detect_game_dirs, locate_asar, Event, InstallConfig, Level, PATCH_DIRS, STEPS};
use egui::{vec2, Align, Color32, CornerRadius, Layout, Rect, RichText, ScrollArea, Sense, Stroke};

use crate::theme;
use crate::worker::{self, Job, Msg, Outcome};

const COLUMN_WIDTH: f32 = 540.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Running { step: u32 },
    Done,
    Failed,
}

enum Hint {
    None,
    Ok(String),
    Bad(String),
}

struct Notice {
    title: String,
    body: String,
    is_error: bool,
}

pub struct InstallerApp {
    game_path: String,
    data_path: String,
    game_hint: Hint,
    data_hint: Hint,
    checked_game: String,
    checked_data: String,

    log: Vec<(Level, String)>,
    phase: Phase,
    detail: Option<String>,

    rx: Option<Receiver<Msg>>,
    notice: Option<Notice>,
}

impl InstallerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::install_style(&cc.egui_ctx);

        let mut app = InstallerApp {
            game_path: String::new(),
            data_path: String::new(),
            game_hint: Hint::None,
            data_hint: Hint::None,
            checked_game: String::new(),
            checked_data: String::new(),
            log: Vec::new(),
            phase: Phase::Idle,
            detail: None,
            rx: None,
            notice: None,
        };

        if let Some(dir) = dc_installer::find_data_dir() {
            app.data_path = dir.display().to_string();
        }
        app.autodetect_game(false);
        app
    }

    fn busy(&self) -> bool {
        self.rx.is_some()
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

        match found.first() {
            Some(dir) => {
                self.game_path = dir.display().to_string();
                if announce {
                    self.log(
                        Level::Success,
                        format!("게임을 찾았습니다: {}", dir.display()),
                    );
                }
            }
            None if announce => self.log(
                Level::Warning,
                "설치된 게임을 찾지 못했습니다. 찾아보기로 폴더를 지정해주세요.",
            ),
            None => {}
        }
    }

    fn refresh_hints(&mut self) {
        if self.checked_game != self.game_path {
            self.checked_game = self.game_path.clone();
            self.game_hint = check_game(&self.game_path);
        }
        if self.checked_data != self.data_path {
            self.checked_data = self.data_path.clone();
            self.data_hint = check_data(&self.data_path);
        }
    }

    fn ready(&self) -> bool {
        matches!(self.game_hint, Hint::Ok(_)) && matches!(self.data_hint, Hint::Ok(_))
    }

    fn status(&self) -> (String, Color32) {
        match self.phase {
            Phase::Running { step } => {
                let label = STEPS
                    .get(step as usize - 1)
                    .map(|s| s.label)
                    .unwrap_or("진행 중");
                match &self.detail {
                    Some(detail) => (format!("{label} · {detail}"), theme::TEXT),
                    None => (label.to_string(), theme::TEXT),
                }
            }
            Phase::Done => ("설치를 마쳤습니다".to_string(), theme::SUCCESS),
            Phase::Failed => (
                "설치를 멈췄습니다. 게임 파일은 그대로입니다".to_string(),
                theme::ERROR,
            ),
            Phase::Idle if self.ready() => ("설치할 준비가 됐습니다".to_string(), theme::MUTED),
            Phase::Idle => (
                "게임 폴더와 번역 데이터를 확인해주세요".to_string(),
                theme::MUTED,
            ),
        }
    }

    fn progress(&self) -> f32 {
        match self.phase {
            Phase::Idle => 0.0,
            Phase::Running { step } => step as f32 / STEPS.len() as f32,
            Phase::Done => 1.0,
            Phase::Failed => 0.0,
        }
    }

    fn start_install(&mut self, ctx: &egui::Context) {
        let asar = match locate_asar(&PathBuf::from(self.game_path.trim())) {
            Ok(path) => path,
            Err(e) => return self.fail_now(e.to_string()),
        };

        self.log.clear();
        self.info(format!("대상 {}", asar.display()));
        self.phase = Phase::Running { step: 1 };
        self.detail = None;
        self.rx = Some(worker::spawn(
            Job::Install(InstallConfig {
                asar_path: asar,
                data_dir: PathBuf::from(self.data_path.trim()),
                integrity: false,
                keep_work_dir: false,
            }),
            ctx.clone(),
        ));
    }

    fn start_restore(&mut self, ctx: &egui::Context) {
        let asar = match locate_asar(&PathBuf::from(self.game_path.trim())) {
            Ok(path) => path,
            Err(e) => return self.fail_now(e.to_string()),
        };

        self.log.clear();
        self.info("백업에서 원본을 되돌립니다.");
        self.phase = Phase::Idle;
        self.rx = Some(worker::spawn(Job::Restore(asar), ctx.clone()));
    }

    fn fail_now(&mut self, message: String) {
        self.log(Level::Error, message.clone());
        self.notice = Some(Notice {
            title: "설치할 수 없습니다".to_string(),
            body: message,
            is_error: true,
        });
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
                    self.info(message);
                }
                Msg::Progress(Event::Message { level, text }) => self.log(level, text),
                Msg::Progress(Event::Progress { label, done, total }) => {
                    self.detail = Some(format!("{label} {done}/{total}"));
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

    fn finish(&mut self, result: Result<Outcome, String>) {
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
                self.notice = Some(Notice {
                    title: "설치 완료".to_string(),
                    body: completion_message(),
                    is_error: false,
                });
            }
            Ok(Outcome::Restored) => {
                self.phase = Phase::Idle;
                self.log(Level::Success, "원본으로 되돌렸습니다.");
                self.notice = Some(Notice {
                    title: "되돌렸습니다".to_string(),
                    body: "게임이 패치 이전 상태로 돌아갔습니다.".to_string(),
                    is_error: false,
                });
            }
            Err(message) => {
                self.phase = Phase::Failed;
                self.log(Level::Error, message.clone());
                self.notice = Some(Notice {
                    title: "설치를 멈췄습니다".to_string(),
                    body: message,
                    is_error: true,
                });
            }
        }
    }
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain_worker();
        self.refresh_hints();

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

                ui.horizontal_top(|ui| {
                    ui.add_space(pad);
                    ui.allocate_ui_with_layout(
                        vec2(width, height),
                        Layout::top_down(Align::Min),
                        |ui| self.column(ui, &ctx),
                    );
                });
            });

        self.notice_window(&ctx);
    }
}

impl InstallerApp {
    fn column(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.vertical_centered(|ui| self.centered_column(ui, ctx));
    }

    fn centered_column(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.title(ui);
        ui.add_space(26.0);

        if self.field(ui, "게임 폴더", true) {
            if let Some(dir) = rfd::FileDialog::new()
                .set_title("게임이 설치된 폴더를 선택하세요")
                .pick_folder()
            {
                self.game_path = dir.display().to_string();
            }
        }

        ui.add_space(16.0);
        if self.field(ui, "번역 데이터", false) {
            if let Some(dir) = rfd::FileDialog::new()
                .set_title("번역 데이터 폴더를 선택하세요")
                .pick_folder()
            {
                self.data_path = dir.display().to_string();
            }
        }

        ui.add_space(22.0);
        self.actions(ui, ctx);

        ui.add_space(22.0);
        self.progress_bar(ui);

        ui.add_space(20.0);
        self.log_panel(ui);

        ui.add_space(12.0);
        ui.label(
            RichText::new("제작 Nyabi · 적용 Oatone · 번역 체퓨 · 이미지 토니, 체퓨 · 영상 민버드")
                .font(theme::regular(11.0))
                .color(theme::FAINT),
        );
    }

    fn title(&self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("데빌 커넥션 한글패치")
                .font(theme::semibold(22.0))
                .color(theme::TEXT),
        );
        ui.add_space(5.0);
        ui.label(
            RichText::new("でびるコネクショん")
                .font(theme::regular(12.5))
                .color(theme::FAINT),
        );
    }

    fn field(&mut self, ui: &mut egui::Ui, label: &str, is_game: bool) -> bool {
        ui.label(
            RichText::new(label)
                .font(theme::semibold(12.5))
                .color(theme::TEXT),
        );
        ui.add_space(7.0);

        let mut browse = false;
        let mut redetect = false;
        let enabled = !self.busy();

        ui.horizontal(|ui| {
            let button = 78.0;
            let gap = ui.spacing().item_spacing.x;
            let buttons = if is_game { 2.0 } else { 1.0 };
            let field_width = (ui.available_width() - (button + gap) * buttons).max(120.0);
            let value = if is_game {
                &mut self.game_path
            } else {
                &mut self.data_path
            };

            ui.add_enabled(
                enabled,
                egui::TextEdit::singleline(value)
                    .desired_width(field_width)
                    .margin(egui::Margin::symmetric(12, 10))
                    .font(theme::regular(12.5))
                    .text_color(theme::TEXT)
                    .hint_text(
                        RichText::new(if is_game {
                            "게임이 설치된 폴더"
                        } else {
                            "data/, tyrano/를 포함하는 폴더"
                        })
                        .font(theme::regular(12.5))
                        .color(theme::FAINT),
                    ),
            );

            if ui
                .add_enabled(enabled, secondary("찾아보기", button))
                .clicked()
            {
                browse = true;
            }
            if is_game
                && ui
                    .add_enabled(enabled, secondary("다시 감지", button))
                    .clicked()
            {
                redetect = true;
            }
        });

        if redetect {
            self.autodetect_game(true);
        }

        let hint = if is_game {
            &self.game_hint
        } else {
            &self.data_hint
        };
        if let Some((text, color)) = match hint {
            Hint::None => None,
            Hint::Ok(text) => Some((text.as_str(), theme::SUCCESS)),
            Hint::Bad(text) => Some((text.as_str(), theme::ERROR)),
        } {
            ui.add_space(7.0);
            ui.label(RichText::new(text).font(theme::regular(11.5)).color(color));
        }

        browse
    }

    fn actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        const SECONDARY: f32 = 132.0;
        const GAP: f32 = 10.0;

        let width = ui.available_width();
        let primary = (width - SECONDARY - GAP).max(120.0);

        ui.allocate_ui_with_layout(
            vec2(width, 40.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = GAP;

                let can_install = !self.busy() && self.ready();
                let install = egui::Button::new(
                    RichText::new(if self.busy() {
                        "설치 중"
                    } else {
                        "설치 시작"
                    })
                    .font(theme::semibold(13.0))
                    .color(if can_install {
                        Color32::WHITE
                    } else {
                        theme::FAINT
                    }),
                )
                .fill(if can_install {
                    theme::ACCENT
                } else {
                    theme::TRACK
                })
                .stroke(if can_install {
                    Stroke::NONE
                } else {
                    Stroke::new(1.0, theme::BORDER)
                })
                .corner_radius(CornerRadius::same(theme::RADIUS))
                .min_size(vec2(primary, 40.0));

                if ui.add_enabled(can_install, install).clicked() {
                    self.start_install(ctx);
                }

                let can_restore = !self.busy() && matches!(self.game_hint, Hint::Ok(_));
                if ui
                    .add_enabled(
                        can_restore,
                        secondary("되돌리기", SECONDARY).min_size(vec2(SECONDARY, 40.0)),
                    )
                    .on_hover_text("백업해 둔 원본 app.asar을 제자리에 돌려놓습니다")
                    .clicked()
                {
                    self.start_restore(ctx);
                }
            },
        );
    }

    fn progress_bar(&self, ui: &mut egui::Ui) {
        let (status, color) = self.status();

        let line = match self.phase {
            Phase::Running { step } => format!("{status}  ·  {step} / {}", STEPS.len()),
            _ => status,
        };
        ui.label(RichText::new(line).font(theme::regular(12.0)).color(color));

        ui.add_space(9.0);

        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
        let radius = CornerRadius::same(2);
        let painter = ui.painter();
        painter.rect_filled(rect, radius, theme::TRACK);

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
        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height((ui.available_height() - 30.0).max(96.0));

                if self.log.is_empty() {
                    ui.label(
                        RichText::new("설치를 시작하면 진행 내용이 여기에 남습니다.")
                            .font(theme::regular(12.0))
                            .color(theme::FAINT),
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
                                        .font(theme::regular(12.0))
                                        .line_height(Some(20.0))
                                        .color(level_color(*level)),
                                );
                            }
                        });
                });
            });
    }

    fn notice_window(&mut self, ctx: &egui::Context) {
        let Some(notice) = self.notice.as_ref() else {
            return;
        };

        let mut close = false;
        let accent = if notice.is_error {
            theme::ERROR
        } else {
            theme::SUCCESS
        };

        let response = egui::Modal::new(egui::Id::new("dc-notice"))
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ctx, |ui| {
                ui.set_max_width(400.0);

                ui.label(
                    RichText::new(&notice.title)
                        .font(theme::semibold(15.0))
                        .color(accent),
                );
                ui.add_space(10.0);
                ui.label(
                    RichText::new(&notice.body)
                        .font(theme::regular(12.5))
                        .line_height(Some(20.0))
                        .color(theme::TEXT),
                );
                ui.add_space(18.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(secondary("확인", 80.0)).clicked() {
                        close = true;
                    }
                });
            });

        if close || response.should_close() {
            self.notice = None;
        }
    }
}

fn secondary(text: &str, width: f32) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.to_owned())
            .font(theme::regular(12.5))
            .color(theme::TEXT),
    )
    .fill(theme::SURFACE)
    .stroke(Stroke::new(1.0, theme::BORDER_STRONG))
    .corner_radius(CornerRadius::same(theme::RADIUS))
    .min_size(vec2(width, 38.0))
}

fn level_color(level: Level) -> Color32 {
    match level {
        Level::Info => theme::TEXT,
        Level::Success => theme::SUCCESS,
        Level::Warning => theme::ACCENT,
        Level::Error => theme::ERROR,
    }
}

fn check_game(path: &str) -> Hint {
    let path = path.trim();
    if path.is_empty() {
        return Hint::None;
    }
    match locate_asar(&PathBuf::from(path)) {
        Ok(asar) => Hint::Ok(format!("확인됨 · {}", tail(&asar, 2))),
        Err(e) => Hint::Bad(e.to_string()),
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

fn tail(path: &std::path::Path, count: usize) -> String {
    let parts: Vec<String> = path
        .components()
        .rev()
        .take(count)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
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

    message.push_str(
        "\n\n이 패치는 ㈜넥슨코리아의 메이플스토리 서체를 사용합니다. 서체의 지적 재산권은 ㈜넥슨코리아에 있습니다.",
    );

    message
}
