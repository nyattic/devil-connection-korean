#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod theme;
mod worker;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("데빌 커넥션 한글패치")
            .with_inner_size([620.0, 700.0])
            .with_min_inner_size([540.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "dc-patcher-gui",
        options,
        Box::new(|cc| Ok(Box::new(app::InstallerApp::new(cc)))),
    )
}
