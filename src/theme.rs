use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};

const PRETENDARD_REGULAR: &[u8] = include_bytes!("../assets/PretendardJP-Regular.ttf");
const PRETENDARD_SEMIBOLD: &[u8] = include_bytes!("../assets/PretendardJP-SemiBold.ttf");

const SEMIBOLD: &str = "PretendardJP-SemiBold";

pub const BG: Color32 = Color32::from_rgb(0xfa, 0xfa, 0xfb);
pub const SURFACE: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const BORDER: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xeb);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0xd6, 0xd6, 0xde);
pub const TRACK: Color32 = Color32::from_rgb(0xed, 0xed, 0xf1);

pub const TEXT: Color32 = Color32::from_rgb(0x17, 0x17, 0x1c);
pub const MUTED: Color32 = Color32::from_rgb(0x85, 0x85, 0x8f);
pub const FAINT: Color32 = Color32::from_rgb(0xa8, 0xa8, 0xb2);

pub const ACCENT: Color32 = Color32::from_rgb(0x8a, 0x35, 0x57);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0xf7, 0xed, 0xf1);

pub const SUCCESS: Color32 = Color32::from_rgb(0x0e, 0x8f, 0x72);
pub const ERROR: Color32 = Color32::from_rgb(0xd1, 0x43, 0x43);

pub const RADIUS: u8 = 8;

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "PretendardJP".to_owned(),
        std::sync::Arc::new(FontData::from_static(PRETENDARD_REGULAR)),
    );
    fonts.font_data.insert(
        SEMIBOLD.to_owned(),
        std::sync::Arc::new(FontData::from_static(PRETENDARD_SEMIBOLD)),
    );

    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "PretendardJP".to_owned());
    }

    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), vec![SEMIBOLD.to_owned()]);

    ctx.set_fonts(fonts);
}

pub fn install_style(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Light);
    ctx.set_visuals_of(egui::Theme::Light, visuals());
    ctx.set_visuals_of(egui::Theme::Dark, visuals());

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, semibold(19.0)),
            (TextStyle::Body, regular(13.0)),
            (TextStyle::Button, regular(13.0)),
            (TextStyle::Small, regular(11.5)),
            (TextStyle::Monospace, regular(12.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(14.0, 8.0);
        style.spacing.interact_size.y = 36.0;
        style.spacing.window_margin = egui::Margin::same(0);
    });
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::light();
    let radius = CornerRadius::same(RADIUS);

    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = SURFACE;
    visuals.faint_bg_color = TRACK;
    visuals.override_text_color = Some(TEXT);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.window_shadow = egui::epaint::Shadow::NONE;

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
    ] {
        widget.bg_fill = SURFACE;
        widget.weak_bg_fill = SURFACE;
        widget.bg_stroke = Stroke::new(1.0, BORDER);
        widget.fg_stroke = Stroke::new(1.0, TEXT);
        widget.corner_radius = radius;
    }

    visuals.widgets.hovered.bg_fill = SURFACE;
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xf6, 0xf6, 0xf8);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.corner_radius = radius;

    visuals.widgets.active.bg_fill = Color32::from_rgb(0xef, 0xef, 0xf3);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0xef, 0xef, 0xf3);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.active.corner_radius = radius;

    visuals.widgets.open.corner_radius = radius;

    visuals.selection.bg_fill = ACCENT_SOFT;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    visuals
}
