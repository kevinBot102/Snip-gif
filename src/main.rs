#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod gif_engine;
mod i18n;

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let initial_file = std::env::args().nth(1).map(PathBuf::from);
    let icon = build_app_icon();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Snip — GIF Crop & Scale")
            .with_inner_size([1200.0, 760.0])
            .with_min_inner_size([880.0, 580.0])
            .with_drag_and_drop(true)
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };

    eframe::run_native(
        "Snip",
        native_options,
        Box::new(move |cc| {
            configure_fonts(&cc.egui_ctx);
            configure_style(&cc.egui_ctx);
            Ok(Box::new(app::SnipApp::new(initial_file)))
        }),
    )
}

/// Loads a Windows system CJK font so Traditional Chinese renders crisply without bloating the portable .exe.
fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let font_candidates = [
        r"C:\Windows\Fonts\msjh.ttc",
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\mingliu.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
    ];

    for path in font_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut font_data = egui::FontData::from_owned(bytes);
            font_data.index = 0;
            fonts
                .font_data
                .insert("cjk_system".to_owned(), Arc::new(font_data));

            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("cjk_system".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("cjk_system".to_owned());
            break;
        }
    }

    ctx.set_fonts(fonts);
}

fn configure_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = egui::Color32::from_rgb(22, 26, 35);
    visuals.panel_fill = egui::Color32::from_rgb(22, 26, 35);
    visuals.faint_bg_color = egui::Color32::from_rgb(30, 35, 47);
    visuals.extreme_bg_color = egui::Color32::from_rgb(15, 18, 24);
    visuals.selection.bg_fill = egui::Color32::from_rgb(79, 156, 249);
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(36, 43, 58);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(48, 57, 76);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(65, 132, 224);
    ctx.set_visuals(visuals);
}

/// Generates a clean 32x32 crop-frame icon programmatically (zero external asset files).
fn build_app_icon() -> egui::IconData {
    let size = 32usize;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) * 4;
            let dx = (x as f32) - 15.5;
            let dy = (y as f32) - 15.5;
            let r_corner = (dx.abs().max(dy.abs()) <= 14.0)
                && ((dx.abs() - 10.0).max(0.0).hypot((dy.abs() - 10.0).max(0.0)) <= 4.0);
            if !r_corner {
                continue;
            }
            // Background gradient
            let bg_r = 28u8;
            let bg_g = 36u8;
            let bg_b = 54u8;

            // Crop corner brackets inside [6..26]
            let in_box = x >= 7 && x <= 24 && y >= 7 && y <= 24;
            let on_border = in_box && (x <= 9 || x >= 22 || y <= 9 || y >= 22);
            let is_crop_mark = on_border && !((x > 12 && x < 19) || (y > 12 && y < 19));
            let is_center_dot = x >= 13 && x <= 18 && y >= 13 && y <= 18;

            if is_crop_mark {
                rgba[idx] = 79;
                rgba[idx + 1] = 156;
                rgba[idx + 2] = 249;
                rgba[idx + 3] = 255;
            } else if is_center_dot {
                rgba[idx] = 245;
                rgba[idx + 1] = 178;
                rgba[idx + 2] = 72;
                rgba[idx + 3] = 255;
            } else {
                rgba[idx] = bg_r;
                rgba[idx + 1] = bg_g;
                rgba[idx + 2] = bg_b;
                rgba[idx + 3] = 255;
            }
        }
    }
    egui::IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_cjk_font_loads_and_renders_glyphs() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);

        let has_cjk_font = [
            r"C:\Windows\Fonts\msjh.ttc",
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\mingliu.ttc",
            r"C:\Windows\Fonts\simhei.ttf",
        ]
        .iter()
        .any(|p| std::path::Path::new(p).exists());

        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            ctx.fonts(|f| {
                let font_id = egui::FontId::proportional(16.0);
                let galley = f.layout_no_wrap(
                    "裁切縮放預覽匯出".to_string(),
                    font_id,
                    egui::Color32::WHITE,
                );
                if has_cjk_font {
                    assert!(
                        galley.size().x > 80.0 && galley.size().y > 10.0,
                        "Traditional Chinese text should measure with real CJK glyphs, got {:?}",
                        galley.size()
                    );
                } else {
                    assert!(galley.size().y > 0.0);
                }
            });
        });
    }
}
