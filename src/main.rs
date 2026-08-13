#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod embedded;
mod theme;
mod worker;

fn main() -> eframe::Result<()> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("아이콘을 읽지 못했습니다");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DevilConnection-Korean-Patcher")
            .with_inner_size([620.0, 700.0])
            .with_min_inner_size([540.0, 600.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "dc-patcher-gui",
        options,
        Box::new(|cc| Ok(Box::new(app::InstallerApp::new(cc)))),
    )
}
