use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, ColorImage, CornerRadius, CursorIcon, FontId, Id, Key, Modifiers, Pos2,
    Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, Vec2,
};

use crate::gif_engine::{
    crop_and_resize_rgba, crop_rgba, export_gif, format_bytes, load_gif, CropRect, ExportConfig,
    ExportQuality, ExportResult, GifData, ResizeAlgorithm,
};
use crate::i18n::{Lang, Strings};

const ACCENT: Color32 = Color32::from_rgb(79, 156, 249);
const ACCENT_SOFT: Color32 = Color32::from_rgba_premultiplied(79, 156, 249, 45);
const BG_CANVAS: Color32 = Color32::from_rgb(15, 18, 24);
const BG_PANEL: Color32 = Color32::from_rgb(22, 26, 35);
const BG_CARD: Color32 = Color32::from_rgb(30, 35, 47);
const BORDER_SUBTLE: Color32 = Color32::from_rgb(46, 54, 70);
const TEXT_MUTED: Color32 = Color32::from_rgb(148, 160, 180);
const SUCCESS_GREEN: Color32 = Color32::from_rgb(72, 199, 142);
const WARN_AMBER: Color32 = Color32::from_rgb(245, 178, 72);
const ERROR_RED: Color32 = Color32::from_rgb(240, 101, 101);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Crop,
    LivePreview,
    Compare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewZoom {
    Fit,
    Percent(u32),
}

impl PreviewZoom {
    const ALL: [PreviewZoom; 5] = [
        PreviewZoom::Fit,
        PreviewZoom::Percent(50),
        PreviewZoom::Percent(100),
        PreviewZoom::Percent(200),
        PreviewZoom::Percent(400),
    ];

    fn label(self, s: &Strings) -> String {
        match self {
            PreviewZoom::Fit => s.zoom_fit().to_string(),
            PreviewZoom::Percent(p) => format!("{p}%"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AspectPreset {
    Free,
    Square1x1,
    Wide16x9,
    Std4x3,
    Photo3x2,
    Original,
}

impl AspectPreset {
    const ALL: [AspectPreset; 6] = [
        AspectPreset::Free,
        AspectPreset::Square1x1,
        AspectPreset::Wide16x9,
        AspectPreset::Std4x3,
        AspectPreset::Photo3x2,
        AspectPreset::Original,
    ];

    fn label(self, s: &Strings) -> &'static str {
        match self {
            AspectPreset::Free => s.ratio_free(),
            AspectPreset::Square1x1 => "1:1",
            AspectPreset::Wide16x9 => "16:9",
            AspectPreset::Std4x3 => "4:3",
            AspectPreset::Photo3x2 => "3:2",
            AspectPreset::Original => s.ratio_original(),
        }
    }

    fn ratio(self, orig_w: u32, orig_h: u32) -> Option<f32> {
        match self {
            AspectPreset::Free => None,
            AspectPreset::Square1x1 => Some(1.0),
            AspectPreset::Wide16x9 => Some(16.0 / 9.0),
            AspectPreset::Std4x3 => Some(4.0 / 3.0),
            AspectPreset::Photo3x2 => Some(3.0 / 2.0),
            AspectPreset::Original => Some((orig_w.max(1) as f32) / (orig_h.max(1) as f32)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CropHandle {
    Move,
    CreateNew,
    NW,
    NE,
    SW,
    SE,
    N,
    S,
    W,
    E,
}

#[derive(Debug, Clone, Copy)]
struct CropDragState {
    handle: CropHandle,
    start_pointer_img: Vec2,
    start_crop: CropRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimelineDrag {
    Playhead,
    TrimStart,
    TrimEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PreviewCacheKey {
    frame_idx: usize,
    crop: CropRect,
    target_w: u32,
    target_h: u32,
    algorithm: ResizeAlgorithm,
    crisp_pixels: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StatusLevel {
    Info,
    Success,
    Error,
}

struct ExportJob {
    progress_done: Arc<AtomicUsize>,
    total_frames: usize,
    cancel_flag: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Result<ExportResult, String>>,
}

pub struct SnipApp {
    lang: Lang,
    gif: Option<Arc<GifData>>,
    source_path: Option<PathBuf>,

    // Spatial crop
    crop: CropRect,
    aspect_preset: AspectPreset,
    show_grid: bool,
    crop_drag: Option<CropDragState>,

    // Scaling
    target_w: u32,
    target_h: u32,
    lock_scale_aspect: bool,
    algorithm: ResizeAlgorithm,

    // Timeline & playback
    start_frame: usize,
    end_frame: usize,
    current_frame: usize,
    is_playing: bool,
    playback_speed: f32,
    apply_speed_to_export: bool,
    last_tick: Instant,
    frame_accumulator: f64,
    timeline_drag: Option<TimelineDrag>,

    // View & export
    view_mode: ViewMode,
    preview_zoom: PreviewZoom,
    quality: ExportQuality,
    export_job: Option<ExportJob>,

    // Status message
    status_text: String,
    status_level: StatusLevel,

    // GPU Texture caches
    checker_tex: Option<TextureHandle>,
    src_tex: Option<TextureHandle>,
    src_tex_frame: Option<usize>,
    preview_tex: Option<TextureHandle>,
    preview_cache_key: Option<PreviewCacheKey>,
    compare_orig_tex: Option<TextureHandle>,
    compare_orig_key: Option<(usize, CropRect)>,
}

impl SnipApp {
    pub fn new(initial_file: Option<PathBuf>) -> Self {
        let mut app = Self {
            lang: Lang::ZhTw,
            gif: None,
            source_path: None,
            crop: CropRect::full(1, 1),
            aspect_preset: AspectPreset::Free,
            show_grid: true,
            crop_drag: None,
            target_w: 1,
            target_h: 1,
            lock_scale_aspect: true,
            algorithm: ResizeAlgorithm::Lanczos3,
            start_frame: 0,
            end_frame: 0,
            current_frame: 0,
            is_playing: true,
            playback_speed: 1.0,
            apply_speed_to_export: false,
            last_tick: Instant::now(),
            frame_accumulator: 0.0,
            timeline_drag: None,
            view_mode: ViewMode::Crop,
            preview_zoom: PreviewZoom::Fit,
            quality: ExportQuality::Balanced,
            export_job: None,
            status_text: Strings::new(Lang::ZhTw).status_ready().to_string(),
            status_level: StatusLevel::Info,
            checker_tex: None,
            src_tex: None,
            src_tex_frame: None,
            preview_tex: None,
            preview_cache_key: None,
            compare_orig_tex: None,
            compare_orig_key: None,
        };

        if let Some(path) = initial_file {
            app.load_from_path(path);
        }
        app
    }

    fn load_from_path(&mut self, path: PathBuf) {
        let s = Strings::new(self.lang);
        match load_gif(&path) {
            Ok(data) => {
                let w = data.width;
                let h = data.height;
                let last_idx = data.frame_count().saturating_sub(1);
                let name = data.file_name.clone();
                let size_str = format_bytes(data.file_size);

                self.gif = Some(Arc::new(data));
                self.source_path = Some(path);
                self.crop = CropRect::full(w, h);
                self.aspect_preset = AspectPreset::Free;
                self.target_w = w;
                self.target_h = h;
                self.start_frame = 0;
                self.end_frame = last_idx;
                self.current_frame = 0;
                self.is_playing = true;
                self.frame_accumulator = 0.0;
                self.last_tick = Instant::now();

                // Invalidate texture caches
                self.src_tex_frame = None;
                self.preview_cache_key = None;
                self.compare_orig_key = None;

                self.status_text = format!("{}: {} ({}×{}, {})", s.status_loaded(), name, w, h, size_str);
                self.status_level = StatusLevel::Info;
            }
            Err(err) => {
                self.status_text = format!("{}{}", s.status_error_prefix(), err);
                self.status_level = StatusLevel::Error;
            }
        }
    }

    fn open_file_dialog(&mut self) {
        let s = Strings::new(self.lang);
        if let Some(path) = rfd::FileDialog::new()
            .set_title(s.dialog_open_title())
            .add_filter("GIF Image", &["gif"])
            .pick_file()
        {
            self.load_from_path(path);
        }
    }

    fn start_export_dialog(&mut self) {
        let Some(ref gif) = self.gif else { return };
        if self.export_job.is_some() {
            return;
        }
        let s = Strings::new(self.lang);

        let default_name = self
            .source_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|stem| format!("{}_snip.gif", stem.to_string_lossy()))
            .unwrap_or_else(|| "output_snip.gif".to_string());

        let mut dialog = rfd::FileDialog::new()
            .set_title(s.dialog_save_title())
            .add_filter("GIF Image", &["gif"])
            .set_file_name(&default_name);

        if let Some(parent) = self.source_path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(parent);
        }

        let Some(output_path) = dialog.save_file() else {
            return;
        };

        let total_frames = self.end_frame.saturating_sub(self.start_frame) + 1;
        let cfg = ExportConfig {
            crop: self.crop,
            start_frame: self.start_frame,
            end_frame: self.end_frame,
            target_w: self.target_w.max(1),
            target_h: self.target_h.max(1),
            algorithm: self.algorithm,
            quality: self.quality,
            speed_multiplier: if self.apply_speed_to_export {
                self.playback_speed
            } else {
                1.0
            },
        };

        let gif_arc = Arc::clone(gif);
        let progress_done = Arc::new(AtomicUsize::new(0));
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let p_clone = Arc::clone(&progress_done);
        let c_clone = Arc::clone(&cancel_flag);
        std::thread::spawn(move || {
            let res = export_gif(&gif_arc, &cfg, &output_path, Some(p_clone), Some(c_clone));
            let _ = tx.send(res);
        });

        self.export_job = Some(ExportJob {
            progress_done,
            total_frames,
            cancel_flag,
            receiver: rx,
        });
    }

    fn poll_export_job(&mut self) {
        let Some(ref job) = self.export_job else {
            return;
        };
        if let Ok(result) = job.receiver.try_recv() {
            let s = Strings::new(self.lang);
            let orig_size = self.gif.as_ref().map(|g| g.file_size).unwrap_or(0);
            match result {
                Ok(res) => {
                    let fname = res
                        .output_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.status_text = format!(
                        "{}: {} ({} → {}, {} {})",
                        s.status_export_done(),
                        fname,
                        format_bytes(orig_size),
                        format_bytes(res.output_size),
                        res.frame_count,
                        s.frames_unit()
                    );
                    self.status_level = StatusLevel::Success;
                }
                Err(err) if err == "CANCELLED" => {
                    self.status_text = s.status_export_cancelled().to_string();
                    self.status_level = StatusLevel::Info;
                }
                Err(err) => {
                    self.status_text = format!("{}{}", s.status_error_prefix(), err);
                    self.status_level = StatusLevel::Error;
                }
            }
            self.export_job = None;
        }
    }

    fn update_crop_with_scale_sync(&mut self, new_crop: CropRect) {
        let Some(ref gif) = self.gif else { return };
        let old_w = self.crop.width.max(1) as f32;
        let old_h = self.crop.height.max(1) as f32;
        let scale_x = (self.target_w.max(1) as f32) / old_w;
        let scale_y = (self.target_h.max(1) as f32) / old_h;

        let mut clamped = new_crop;
        clamped.clamp_to(gif.width, gif.height);
        self.crop = clamped;

        if self.lock_scale_aspect {
            self.target_w = ((self.crop.width as f32) * scale_x).round().max(1.0) as u32;
            self.target_h = ((self.crop.height as f32) * scale_x).round().max(1.0) as u32;
        } else {
            self.target_w = ((self.crop.width as f32) * scale_x).round().max(1.0) as u32;
            self.target_h = ((self.crop.height as f32) * scale_y).round().max(1.0) as u32;
        }
    }

    fn apply_aspect_preset(&mut self, preset: AspectPreset) {
        self.aspect_preset = preset;
        let Some(ref gif) = self.gif else { return };
        let Some(ratio) = preset.ratio(gif.width, gif.height) else {
            return;
        };

        // Fit the largest centered rect with `ratio` inside the current crop box (or full image if tiny)
        let cx = (self.crop.x as f32) + (self.crop.width as f32) * 0.5;
        let cy = (self.crop.y as f32) + (self.crop.height as f32) * 0.5;

        let mut w = self.crop.width as f32;
        let mut h = w / ratio;
        if h > (self.crop.height as f32) {
            h = self.crop.height as f32;
            w = h * ratio;
        }
        w = w.clamp(1.0, gif.width as f32);
        h = h.clamp(1.0, gif.height as f32);

        let w_u = w.round().max(1.0) as u32;
        let h_u = h.round().max(1.0) as u32;
        let x_u = (cx - (w_u as f32) * 0.5)
            .round()
            .clamp(0.0, (gif.width - w_u) as f32) as u32;
        let y_u = (cy - (h_u as f32) * 0.5)
            .round()
            .clamp(0.0, (gif.height - h_u) as f32) as u32;

        self.update_crop_with_scale_sync(CropRect {
            x: x_u,
            y: y_u,
            width: w_u,
            height: h_u,
        });
    }

    fn tick_animation(&mut self, ctx: &egui::Context) {
        let Some(ref gif) = self.gif else { return };
        let frame_count = gif.frame_count();
        if frame_count <= 1 {
            return;
        }

        if self.current_frame < self.start_frame || self.current_frame > self.end_frame {
            self.current_frame = self.start_frame;
        }

        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64().min(0.25);
        self.last_tick = now;

        if !self.is_playing || self.start_frame >= self.end_frame {
            return;
        }

        let speed = self.playback_speed.clamp(0.1, 8.0) as f64;
        self.frame_accumulator += dt * speed;

        // Advance through frames according to their actual GIF delays
        let mut safety = 0;
        while safety < 8 {
            let cur_delay = gif.frames[self.current_frame].delay_secs().max(0.016);
            if self.frame_accumulator >= cur_delay {
                self.frame_accumulator -= cur_delay;
                if self.current_frame >= self.end_frame {
                    self.current_frame = self.start_frame;
                } else {
                    self.current_frame += 1;
                }
                safety += 1;
            } else {
                let remaining = (cur_delay - self.frame_accumulator) / speed;
                ctx.request_repaint_after(Duration::from_secs_f64(remaining.clamp(0.008, 0.05)));
                break;
            }
        }
    }

    fn ensure_checker_texture(&mut self, ctx: &egui::Context) {
        if self.checker_tex.is_some() {
            return;
        }
        let tile = 16usize;
        let mut rgba = vec![0u8; tile * tile * 4];
        for y in 0..tile {
            for x in 0..tile {
                let light = ((x / 8) + (y / 8)) % 2 == 0;
                let v = if light { 54 } else { 38 };
                let idx = (y * tile + x) * 4;
                rgba[idx] = v;
                rgba[idx + 1] = v + 2;
                rgba[idx + 2] = v + 6;
                rgba[idx + 3] = 255;
            }
        }
        let img = ColorImage::from_rgba_unmultiplied([tile, tile], &rgba);
        self.checker_tex = Some(ctx.load_texture(
            "checkerboard",
            img,
            TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Nearest,
                wrap_mode: egui::TextureWrapMode::Repeat,
                mipmap_mode: None,
            },
        ));
    }

    fn draw_checkerboard(&self, painter: &egui::Painter, rect: Rect) {
        if let Some(ref tex) = self.checker_tex {
            let uv = Rect::from_min_max(
                Pos2::ZERO,
                Pos2::new(rect.width() / 16.0, rect.height() / 16.0),
            );
            painter.image(tex.id(), rect, uv, Color32::WHITE);
        }
    }

    fn ensure_src_texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureId> {
        let gif = self.gif.as_ref()?;
        let frame_idx = self.current_frame.min(gif.frame_count().saturating_sub(1));
        if self.src_tex_frame != Some(frame_idx) || self.src_tex.is_none() {
            let frame = &gif.frames[frame_idx];
            let color_img = ColorImage::from_rgba_unmultiplied(
                [gif.width as usize, gif.height as usize],
                &frame.rgba,
            );
            let opts = TextureOptions::LINEAR;
            if let Some(ref mut tex) = self.src_tex {
                tex.set(color_img, opts);
            } else {
                self.src_tex = Some(ctx.load_texture("gif_src", color_img, opts));
            }
            self.src_tex_frame = Some(frame_idx);
        }
        self.src_tex.as_ref().map(|t| t.id())
    }

    fn ensure_preview_texture(
        &mut self,
        ctx: &egui::Context,
        crisp_pixels: bool,
    ) -> Option<egui::TextureId> {
        let gif = self.gif.as_ref()?;
        let frame_idx = self.current_frame.min(gif.frame_count().saturating_sub(1));
        let key = PreviewCacheKey {
            frame_idx,
            crop: self.crop,
            target_w: self.target_w.max(1),
            target_h: self.target_h.max(1),
            algorithm: self.algorithm,
            crisp_pixels,
        };

        let opts = if crisp_pixels {
            TextureOptions::NEAREST
        } else {
            TextureOptions::LINEAR
        };

        if self.preview_cache_key != Some(key) || self.preview_tex.is_none() {
            let frame = &gif.frames[frame_idx];
            if let Ok(rgba) = crop_and_resize_rgba(
                &frame.rgba,
                gif.width,
                gif.height,
                key.crop,
                key.target_w,
                key.target_h,
                key.algorithm,
            ) {
                let color_img = ColorImage::from_rgba_unmultiplied(
                    [key.target_w as usize, key.target_h as usize],
                    &rgba,
                );
                if let Some(ref mut tex) = self.preview_tex {
                    tex.set(color_img, opts);
                } else {
                    self.preview_tex = Some(ctx.load_texture("gif_preview", color_img, opts));
                }
                self.preview_cache_key = Some(key);
            }
        }
        self.preview_tex.as_ref().map(|t| t.id())
    }

    fn ensure_compare_orig_texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureId> {
        let gif = self.gif.as_ref()?;
        let frame_idx = self.current_frame.min(gif.frame_count().saturating_sub(1));
        let key = (frame_idx, self.crop);

        if self.compare_orig_key != Some(key) || self.compare_orig_tex.is_none() {
            let frame = &gif.frames[frame_idx];
            let cropped = crop_rgba(&frame.rgba, gif.width, gif.height, self.crop);
            let color_img = ColorImage::from_rgba_unmultiplied(
                [self.crop.width as usize, self.crop.height as usize],
                &cropped,
            );
            let opts = TextureOptions::NEAREST;
            if let Some(ref mut tex) = self.compare_orig_tex {
                tex.set(color_img, opts);
            } else {
                self.compare_orig_tex = Some(ctx.load_texture("gif_compare_orig", color_img, opts));
            }
            self.compare_orig_key = Some(key);
        }
        self.compare_orig_tex.as_ref().map(|t| t.id())
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let mut do_open = false;
        let mut do_export = false;

        ctx.input(|i| {
            if i.modifiers.ctrl && i.key_pressed(Key::O) {
                do_open = true;
            }
            if i.modifiers.ctrl && i.key_pressed(Key::S) {
                do_export = true;
            }
            if self.gif.is_some() {
                if i.key_pressed(Key::Space) {
                    self.is_playing = !self.is_playing;
                    self.last_tick = Instant::now();
                }
                if i.key_pressed(Key::ArrowLeft) {
                    self.is_playing = false;
                    if self.current_frame > self.start_frame {
                        self.current_frame -= 1;
                    } else {
                        self.current_frame = self.end_frame;
                    }
                }
                if i.key_pressed(Key::ArrowRight) {
                    self.is_playing = false;
                    if self.current_frame < self.end_frame {
                        self.current_frame += 1;
                    } else {
                        self.current_frame = self.start_frame;
                    }
                }
                if i.key_pressed(Key::I) && i.modifiers == Modifiers::NONE {
                    self.start_frame = self.current_frame.min(self.end_frame);
                }
                if i.key_pressed(Key::O) && i.modifiers == Modifiers::NONE {
                    self.end_frame = self.current_frame.max(self.start_frame);
                }
            }
        });

        if do_open {
            self.open_file_dialog();
        }
        if do_export && self.gif.is_some() {
            self.start_export_dialog();
        }
    }

    fn handle_file_drop(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.load_from_path(path);
        }

        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if hovering {
            let s = Strings::new(self.lang);
            let screen = ctx.screen_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                Id::new("drop_overlay"),
            ));
            painter.rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(180));
            painter.rect_stroke(
                screen.shrink(20.0),
                CornerRadius::same(16),
                Stroke::new(3.0, ACCENT),
                StrokeKind::Inside,
            );
            painter.text(
                screen.center(),
                Align2::CENTER_CENTER,
                s.drop_overlay(),
                FontId::proportional(28.0),
                Color32::WHITE,
            );
        }
    }

    fn draw_top_bar(&mut self, ctx: &egui::Context) {
        let s = Strings::new(self.lang);
        egui::TopBottomPanel::top("top_bar")
            .exact_height(50.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Brand title
                    ui.label(
                        egui::RichText::new("[+] Snip")
                            .size(19.0)
                            .strong()
                            .color(ACCENT),
                    );
                    ui.label(
                        egui::RichText::new(s.app_subtitle())
                            .size(12.0)
                            .color(TEXT_MUTED),
                    );

                    ui.separator();

                    // Open button
                    if ui
                        .add(
                            egui::Button::new(s.open_gif())
                                .min_size(Vec2::new(86.0, 30.0))
                                .corner_radius(CornerRadius::same(6)),
                        )
                        .clicked()
                    {
                        self.open_file_dialog();
                    }

                    // File info pill
                    if let Some(ref gif) = self.gif {
                        ui.add_space(6.0);
                        egui::Frame::new()
                            .fill(BG_CARD)
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(10, 4))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{}  ·  {}×{}  ·  {} {}  ·  {}",
                                        gif.file_name,
                                        gif.width,
                                        gif.height,
                                        gif.frame_count(),
                                        s.frames_unit(),
                                        format_bytes(gif.file_size),
                                    ))
                                    .size(12.0)
                                    .color(Color32::from_rgb(215, 224, 238)),
                                );
                            });
                    }

                    // Right-aligned controls
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Language toggle
                        if ui
                            .add(
                                egui::Button::new(self.lang.code())
                                    .min_size(Vec2::new(78.0, 30.0))
                                    .corner_radius(CornerRadius::same(6)),
                            )
                            .clicked()
                        {
                            self.lang = self.lang.toggle();
                        }

                        ui.add_space(6.0);

                        // Export button
                        let can_export = self.gif.is_some() && self.export_job.is_none();
                        let export_label = if self.export_job.is_some() {
                            s.exporting().to_string()
                        } else {
                            s.export_gif().to_string()
                        };
                        let btn = egui::Button::new(
                            egui::RichText::new(export_label)
                                .strong()
                                .color(if can_export {
                                    Color32::WHITE
                                } else {
                                    TEXT_MUTED
                                }),
                        )
                        .fill(if can_export { ACCENT } else { BG_CARD })
                        .min_size(Vec2::new(108.0, 30.0))
                        .corner_radius(CornerRadius::same(6));

                        if ui.add_enabled(can_export, btn).clicked() {
                            self.start_export_dialog();
                        }
                    });
                });
            });
    }

    fn draw_inspector(&mut self, ctx: &egui::Context) {
        let s = Strings::new(self.lang);
        egui::SidePanel::right("inspector")
            .exact_width(305.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(14, 12))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let has_gif = self.gif.is_some();
                    let (orig_w, orig_h) = self
                        .gif
                        .as_ref()
                        .map(|g| (g.width, g.height))
                        .unwrap_or((1, 1));

                    // --- Section 1: Spatial Crop ---
                    section_card(ui, s.section_crop(), |ui| {
                        ui.add_enabled_ui(has_gif, |ui| {
                            let mut crop = self.crop;
                            let mut changed = false;

                            egui::Grid::new("crop_grid")
                                .num_columns(4)
                                .spacing([8.0, 6.0])
                                .show(ui, |ui| {
                                    ui.label("X");
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut crop.x)
                                                .range(0..=orig_w.saturating_sub(1))
                                                .suffix(" px"),
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                    ui.label("Y");
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut crop.y)
                                                .range(0..=orig_h.saturating_sub(1))
                                                .suffix(" px"),
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                    ui.end_row();

                                    ui.label(s.width());
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut crop.width)
                                                .range(1..=orig_w.saturating_sub(crop.x).max(1))
                                                .suffix(" px"),
                                        )
                                        .changed()
                                    {
                                        if let Some(ratio) =
                                            self.aspect_preset.ratio(orig_w, orig_h)
                                        {
                                            crop.height = ((crop.width as f32) / ratio)
                                                .round()
                                                .clamp(1.0, (orig_h - crop.y) as f32)
                                                as u32;
                                        }
                                        changed = true;
                                    }
                                    ui.label(s.height());
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut crop.height)
                                                .range(1..=orig_h.saturating_sub(crop.y).max(1))
                                                .suffix(" px"),
                                        )
                                        .changed()
                                    {
                                        if let Some(ratio) =
                                            self.aspect_preset.ratio(orig_w, orig_h)
                                        {
                                            crop.width = ((crop.height as f32) * ratio)
                                                .round()
                                                .clamp(1.0, (orig_w - crop.x) as f32)
                                                as u32;
                                        }
                                        changed = true;
                                    }
                                    ui.end_row();
                                });

                            if changed {
                                self.update_crop_with_scale_sync(crop);
                            }

                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(s.aspect_ratio())
                                    .size(12.0)
                                    .color(TEXT_MUTED),
                            );
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                                for preset in AspectPreset::ALL {
                                    let selected = self.aspect_preset == preset;
                                    if ui
                                        .selectable_label(selected, preset.label(&s))
                                        .clicked()
                                    {
                                        self.apply_aspect_preset(preset);
                                    }
                                }
                            });

                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.show_grid, s.show_grid());
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button(s.reset_crop()).clicked() {
                                            self.aspect_preset = AspectPreset::Free;
                                            self.update_crop_with_scale_sync(CropRect::full(
                                                orig_w, orig_h,
                                            ));
                                        }
                                    },
                                );
                            });
                        });
                    });

                    ui.add_space(10.0);

                    // --- Section 2: Resize & Algorithm ---
                    section_card(ui, s.section_resize(), |ui| {
                        ui.add_enabled_ui(has_gif, |ui| {
                            ui.label(
                                egui::RichText::new(s.algorithm())
                                    .size(12.0)
                                    .color(TEXT_MUTED),
                            );
                            egui::ComboBox::from_id_salt("resize_algo_combo")
                                .width(ui.available_width())
                                .selected_text(self.algorithm.label(&s))
                                .show_ui(ui, |ui| {
                                    for algo in ResizeAlgorithm::ALL {
                                        ui.selectable_value(
                                            &mut self.algorithm,
                                            algo,
                                            algo.label(&s),
                                        );
                                    }
                                });

                            ui.label(
                                egui::RichText::new(self.algorithm.description(&s))
                                    .size(11.0)
                                    .color(TEXT_MUTED),
                            );

                            ui.add_space(6.0);
                            let crop_aspect =
                                (self.crop.width.max(1) as f32) / (self.crop.height.max(1) as f32);

                            ui.horizontal(|ui| {
                                ui.label(s.width());
                                let w_resp = ui.add(
                                    egui::DragValue::new(&mut self.target_w)
                                        .range(1..=4096)
                                        .suffix(" px"),
                                );
                                if w_resp.changed() && self.lock_scale_aspect {
                                    self.target_h = ((self.target_w as f32) / crop_aspect)
                                        .round()
                                        .clamp(1.0, 4096.0)
                                        as u32;
                                }

                                ui.label("×");

                                let h_resp = ui.add(
                                    egui::DragValue::new(&mut self.target_h)
                                        .range(1..=4096)
                                        .suffix(" px"),
                                );
                                if h_resp.changed() && self.lock_scale_aspect {
                                    self.target_w = ((self.target_h as f32) * crop_aspect)
                                        .round()
                                        .clamp(1.0, 4096.0)
                                        as u32;
                                }
                            });

                            ui.checkbox(&mut self.lock_scale_aspect, s.lock_aspect());

                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(s.quick_scale())
                                    .size(12.0)
                                    .color(TEXT_MUTED),
                            );
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                                for pct in [25u32, 50, 75, 100, 150, 200] {
                                    if ui.small_button(format!("{pct}%")).clicked() {
                                        let factor = (pct as f32) / 100.0;
                                        self.target_w = ((self.crop.width as f32) * factor)
                                            .round()
                                            .clamp(1.0, 4096.0)
                                            as u32;
                                        self.target_h = ((self.crop.height as f32) * factor)
                                            .round()
                                            .clamp(1.0, 4096.0)
                                            as u32;
                                    }
                                }
                            });
                        });
                    });

                    ui.add_space(10.0);

                    // --- Section 3: Export Settings ---
                    section_card(ui, s.section_export(), |ui| {
                        ui.add_enabled_ui(has_gif, |ui| {
                            ui.label(
                                egui::RichText::new(s.quality())
                                    .size(12.0)
                                    .color(TEXT_MUTED),
                            );
                            egui::ComboBox::from_id_salt("export_quality_combo")
                                .width(ui.available_width())
                                .selected_text(self.quality.label(&s))
                                .show_ui(ui, |ui| {
                                    for q in ExportQuality::ALL {
                                        ui.selectable_value(&mut self.quality, q, q.label(&s));
                                    }
                                });

                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(s.speed_label());
                                ui.add(
                                    egui::DragValue::new(&mut self.playback_speed)
                                        .speed(0.05)
                                        .range(0.25..=4.0)
                                        .fixed_decimals(2)
                                        .suffix("x"),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                                for sp in [0.5f32, 1.0, 1.5, 2.0] {
                                    let active = (self.playback_speed - sp).abs() < 0.01;
                                    if ui
                                        .selectable_label(active, format!("{sp}x"))
                                        .clicked()
                                    {
                                        self.playback_speed = sp;
                                    }
                                }
                            });
                            ui.checkbox(
                                &mut self.apply_speed_to_export,
                                s.apply_speed_export(),
                            );

                            // Live output summary
                            if let Some(ref gif) = self.gif {
                                ui.add_space(6.0);
                                let out_frames =
                                    self.end_frame.saturating_sub(self.start_frame) + 1;
                                let base_dur =
                                    gif.range_duration_secs(self.start_frame, self.end_frame);
                                let out_dur = if self.apply_speed_to_export {
                                    base_dur / (self.playback_speed.max(0.1) as f64)
                                } else {
                                    base_dur
                                };
                                egui::Frame::new()
                                    .fill(BG_CANVAS)
                                    .corner_radius(CornerRadius::same(6))
                                    .inner_margin(egui::Margin::symmetric(8, 6))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(s.output_summary())
                                                .size(11.0)
                                                .color(TEXT_MUTED),
                                        );
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{}×{}  ·  {} {}  ·  {:.2}s",
                                                self.target_w,
                                                self.target_h,
                                                out_frames,
                                                s.frames_unit(),
                                                out_dur
                                            ))
                                            .size(12.5)
                                            .strong()
                                            .color(Color32::from_rgb(220, 232, 250)),
                                        );
                                    });
                            }
                        });
                    });

                    // Active export progress bar
                    if let Some(ref job) = self.export_job {
                        ui.add_space(10.0);
                        let done = job.progress_done.load(Ordering::Relaxed);
                        let total = job.total_frames.max(1);
                        let frac = (done as f32) / (total as f32);
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .text(format!("{} ({}/{})", s.exporting(), done, total))
                                .animate(true),
                        );
                        if ui.button(s.cancel()).clicked() {
                            job.cancel_flag.store(true, Ordering::Relaxed);
                        }
                    }

                    // Status box
                    ui.add_space(10.0);
                    let status_color = match self.status_level {
                        StatusLevel::Info => TEXT_MUTED,
                        StatusLevel::Success => SUCCESS_GREEN,
                        StatusLevel::Error => ERROR_RED,
                    };
                    ui.label(
                        egui::RichText::new(&self.status_text)
                            .size(11.5)
                            .color(status_color),
                    );
                });
            });
    }

    fn draw_bottom_timeline(&mut self, ctx: &egui::Context) {
        let s = Strings::new(self.lang);
        egui::TopBottomPanel::bottom("timeline_panel")
            .exact_height(98.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                let Some(gif) = self.gif.clone() else {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new(s.shortcuts_hint())
                                .size(12.0)
                                .color(TEXT_MUTED),
                        );
                    });
                    return;
                };

                let total_frames = gif.frame_count();
                let max_idx = total_frames.saturating_sub(1);

                // Top row: playback controls + frame readout + trim numeric inputs
                ui.horizontal(|ui| {
                    if ui.button("|<").clicked() {
                        self.current_frame = self.start_frame;
                    }
                    if ui.button("<").clicked() {
                        self.is_playing = false;
                        self.current_frame = if self.current_frame > self.start_frame {
                            self.current_frame - 1
                        } else {
                            self.end_frame
                        };
                    }
                    let play_txt = if self.is_playing { "||" } else { "▶" };
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(play_txt).strong())
                                .min_size(Vec2::new(36.0, 24.0))
                                .fill(ACCENT),
                        )
                        .clicked()
                    {
                        self.is_playing = !self.is_playing;
                        self.last_tick = Instant::now();
                    }
                    if ui.button(">").clicked() {
                        self.is_playing = false;
                        self.current_frame = if self.current_frame < self.end_frame {
                            self.current_frame + 1
                        } else {
                            self.start_frame
                        };
                    }
                    if ui.button(">|").clicked() {
                        self.current_frame = self.end_frame;
                    }

                    ui.separator();

                    let trim_dur = gif.range_duration_secs(self.start_frame, self.end_frame);
                    let total_dur = gif.total_duration_secs();
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {} / {}  ({:.2}s / {:.2}s)",
                            s.frame_label(),
                            self.current_frame + 1,
                            total_frames,
                            trim_dur,
                            total_dur
                        ))
                        .size(12.0)
                        .monospace()
                        .color(Color32::from_rgb(215, 224, 238)),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(s.reset_trim()).clicked() {
                            self.start_frame = 0;
                            self.end_frame = max_idx;
                        }
                        if ui.small_button(s.set_out()).clicked() {
                            self.end_frame = self.current_frame.max(self.start_frame);
                        }
                        if ui.small_button(s.set_in()).clicked() {
                            self.start_frame = self.current_frame.min(self.end_frame);
                        }

                        ui.separator();

                        // 1-based UI display for In / Out frames
                        let mut out_1 = self.end_frame + 1;
                        if ui
                            .add(
                                egui::DragValue::new(&mut out_1)
                                    .range((self.start_frame + 1)..=total_frames.max(1)),
                            )
                            .changed()
                        {
                            self.end_frame = (out_1 - 1).clamp(self.start_frame, max_idx);
                            if self.current_frame > self.end_frame {
                                self.current_frame = self.end_frame;
                            }
                        }
                        ui.label(egui::RichText::new(s.timeline_out()).size(12.0).color(TEXT_MUTED));

                        let mut in_1 = self.start_frame + 1;
                        if ui
                            .add(
                                egui::DragValue::new(&mut in_1)
                                    .range(1..=(self.end_frame + 1).max(1)),
                            )
                            .changed()
                        {
                            self.start_frame = (in_1 - 1).min(self.end_frame);
                            if self.current_frame < self.start_frame {
                                self.current_frame = self.start_frame;
                            }
                        }
                        ui.label(egui::RichText::new(s.timeline_in()).size(12.0).color(TEXT_MUTED));
                    });
                });

                ui.add_space(6.0);

                // Interactive custom timeline track
                let desired = Vec2::new(ui.available_width(), 38.0);
                let (rect, response) = ui.allocate_exact_size(desired, Sense::click_and_drag());
                let painter = ui.painter_at(rect);

                let track_rect = Rect::from_min_max(
                    Pos2::new(rect.left() + 8.0, rect.top() + 8.0),
                    Pos2::new(rect.right() - 8.0, rect.bottom() - 8.0),
                );
                let frame_to_x = |idx: usize| -> f32 {
                    if max_idx == 0 {
                        track_rect.left()
                    } else {
                        track_rect.left()
                            + ((idx as f32) / (max_idx as f32)) * track_rect.width()
                    }
                };
                let x_to_frame = |x: f32| -> usize {
                    if max_idx == 0 || track_rect.width() <= 0.0 {
                        0
                    } else {
                        let t = ((x - track_rect.left()) / track_rect.width()).clamp(0.0, 1.0);
                        (t * (max_idx as f32)).round() as usize
                    }
                };

                let start_x = frame_to_x(self.start_frame);
                let end_x = frame_to_x(self.end_frame);
                let cur_x = frame_to_x(self.current_frame);

                // Handle pointer interaction
                if response.drag_started() || response.clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let d_start = (pos.x - start_x).abs();
                        let d_end = (pos.x - end_x).abs();
                        if d_start <= 10.0 && d_start <= d_end {
                            self.timeline_drag = Some(TimelineDrag::TrimStart);
                        } else if d_end <= 10.0 {
                            self.timeline_drag = Some(TimelineDrag::TrimEnd);
                        } else {
                            self.timeline_drag = Some(TimelineDrag::Playhead);
                            self.current_frame = x_to_frame(pos.x);
                        }
                    }
                }
                if response.dragged() {
                    if let (Some(mode), Some(pos)) =
                        (self.timeline_drag, response.interact_pointer_pos())
                    {
                        let f = x_to_frame(pos.x);
                        match mode {
                            TimelineDrag::TrimStart => {
                                self.start_frame = f.min(self.end_frame);
                                self.current_frame = self.start_frame;
                                self.is_playing = false;
                            }
                            TimelineDrag::TrimEnd => {
                                self.end_frame = f.max(self.start_frame);
                                self.current_frame = self.end_frame;
                                self.is_playing = false;
                            }
                            TimelineDrag::Playhead => {
                                self.current_frame = f.clamp(self.start_frame, self.end_frame);
                                self.is_playing = false;
                            }
                        }
                    }
                }
                if response.drag_stopped() {
                    self.timeline_drag = None;
                }

                // Draw background track
                painter.rect_filled(track_rect, CornerRadius::same(5), BG_CANVAS);
                painter.rect_stroke(
                    track_rect,
                    CornerRadius::same(5),
                    Stroke::new(1.0, BORDER_SUBTLE),
                    StrokeKind::Inside,
                );

                // Draw subtle frame ticks if reasonable count
                if total_frames > 1 && total_frames <= 120 {
                    for i in 0..total_frames {
                        let tx = frame_to_x(i);
                        painter.line_segment(
                            [
                                Pos2::new(tx, track_rect.bottom() - 5.0),
                                Pos2::new(tx, track_rect.bottom() - 1.0),
                            ],
                            Stroke::new(1.0, Color32::from_rgb(55, 64, 82)),
                        );
                    }
                }

                // Highlight active trim region
                let active_rect = Rect::from_min_max(
                    Pos2::new(start_x, track_rect.top()),
                    Pos2::new(end_x.max(start_x + 2.0), track_rect.bottom()),
                );
                painter.rect_filled(active_rect, CornerRadius::same(4), ACCENT_SOFT);
                painter.rect_stroke(
                    active_rect,
                    CornerRadius::same(4),
                    Stroke::new(1.5, ACCENT),
                    StrokeKind::Inside,
                );

                // Trim handles
                let handle_w = 8.0;
                let left_handle = Rect::from_center_size(
                    Pos2::new(start_x, track_rect.center().y),
                    Vec2::new(handle_w, track_rect.height() + 6.0),
                );
                let right_handle = Rect::from_center_size(
                    Pos2::new(end_x, track_rect.center().y),
                    Vec2::new(handle_w, track_rect.height() + 6.0),
                );
                painter.rect_filled(left_handle, CornerRadius::same(3), WARN_AMBER);
                painter.rect_filled(right_handle, CornerRadius::same(3), WARN_AMBER);

                // Playhead needle
                painter.line_segment(
                    [
                        Pos2::new(cur_x, rect.top() + 2.0),
                        Pos2::new(cur_x, rect.bottom() - 2.0),
                    ],
                    Stroke::new(2.0, Color32::WHITE),
                );
                painter.circle_filled(Pos2::new(cur_x, rect.top() + 5.0), 4.5, Color32::WHITE);
            });
    }

    fn draw_center_workspace(&mut self, ctx: &egui::Context) {
        let s = Strings::new(self.lang);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG_CANVAS).inner_margin(egui::Margin::same(12)))
            .show(ctx, |ui| {
                // Mode toolbar row
                ui.horizontal(|ui| {
                    for (mode, label) in [
                        (ViewMode::Crop, s.view_crop()),
                        (ViewMode::LivePreview, s.view_preview()),
                        (ViewMode::Compare, s.view_compare()),
                    ] {
                        let active = self.view_mode == mode;
                        let btn = egui::Button::new(
                            egui::RichText::new(label)
                                .strong()
                                .color(if active { Color32::WHITE } else { TEXT_MUTED }),
                        )
                        .fill(if active { ACCENT } else { BG_CARD })
                        .corner_radius(CornerRadius::same(6))
                        .min_size(Vec2::new(92.0, 28.0));
                        if ui.add(btn).clicked() {
                            self.view_mode = mode;
                        }
                    }

                    if self.view_mode == ViewMode::LivePreview && self.gif.is_some() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            for zoom in PreviewZoom::ALL.into_iter().rev() {
                                let selected = self.preview_zoom == zoom;
                                if ui
                                    .selectable_label(selected, zoom.label(&s))
                                    .clicked()
                                {
                                    self.preview_zoom = zoom;
                                }
                            }
                            ui.label(
                                egui::RichText::new(s.zoom_label())
                                    .size(12.0)
                                    .color(TEXT_MUTED),
                            );
                        });
                    }
                });

                ui.add_space(8.0);

                if self.gif.is_none() {
                    self.draw_empty_state(ui);
                    return;
                }

                match self.view_mode {
                    ViewMode::Crop => self.draw_crop_viewport(ctx, ui),
                    ViewMode::LivePreview => self.draw_live_preview_viewport(ctx, ui),
                    ViewMode::Compare => self.draw_compare_viewport(ctx, ui),
                }
            });
    }

    fn draw_empty_state(&mut self, ui: &mut Ui) {
        let s = Strings::new(self.lang);
        let avail = ui.available_rect_before_wrap();
        let box_rect = Rect::from_center_size(
            avail.center(),
            Vec2::new(avail.width().min(440.0), avail.height().min(240.0)),
        );
        let painter = ui.painter();
        painter.rect_filled(box_rect, CornerRadius::same(14), BG_PANEL);
        painter.rect_stroke(
            box_rect,
            CornerRadius::same(14),
            Stroke::new(1.5, BORDER_SUBTLE),
            StrokeKind::Inside,
        );

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(box_rect), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(46.0);
                // Crisp vector crop badge
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
                let p = ui.painter_at(icon_rect);
                let inner = icon_rect.shrink(6.0);
                let arm = 11.0;
                let st = Stroke::new(3.0, ACCENT);
                // 4 corner crop brackets
                p.line_segment([inner.left_top(), Pos2::new(inner.left() + arm, inner.top())], st);
                p.line_segment([inner.left_top(), Pos2::new(inner.left(), inner.top() + arm)], st);
                p.line_segment([inner.right_top(), Pos2::new(inner.right() - arm, inner.top())], st);
                p.line_segment([inner.right_top(), Pos2::new(inner.right(), inner.top() + arm)], st);
                p.line_segment([inner.left_bottom(), Pos2::new(inner.left() + arm, inner.bottom())], st);
                p.line_segment([inner.left_bottom(), Pos2::new(inner.left(), inner.bottom() - arm)], st);
                p.line_segment([inner.right_bottom(), Pos2::new(inner.right() - arm, inner.bottom())], st);
                p.line_segment([inner.right_bottom(), Pos2::new(inner.right(), inner.bottom() - arm)], st);
                p.rect_filled(Rect::from_center_size(inner.center(), Vec2::splat(10.0)), CornerRadius::same(2), WARN_AMBER);

                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(s.drop_hint_title())
                        .size(18.0)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(s.drop_hint_sub())
                        .size(12.5)
                        .color(TEXT_MUTED),
                );
                ui.add_space(16.0);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(s.open_gif())
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(ACCENT)
                        .min_size(Vec2::new(130.0, 34.0))
                        .corner_radius(CornerRadius::same(8)),
                    )
                    .clicked()
                {
                    self.open_file_dialog();
                }
            });
        });
    }

    fn draw_crop_viewport(&mut self, ctx: &egui::Context, ui: &mut Ui) {
        let Some(gif) = self.gif.clone() else { return };
        let Some(tex_id) = self.ensure_src_texture(ctx) else {
            return;
        };

        let avail = ui.available_rect_before_wrap();
        let (viewport_rect, response) = ui.allocate_exact_size(avail.size(), Sense::click_and_drag());
        let painter = ui.painter_at(viewport_rect);

        // Compute centered display rect preserving image aspect ratio
        let pad = 20.0;
        let max_w = (viewport_rect.width() - pad * 2.0).max(16.0);
        let max_h = (viewport_rect.height() - pad * 2.0).max(16.0);
        let img_w = gif.width as f32;
        let img_h = gif.height as f32;
        let scale = (max_w / img_w).min(max_h / img_h).min(6.0);
        let disp_size = Vec2::new(img_w * scale, img_h * scale);
        let img_rect = Rect::from_center_size(viewport_rect.center(), disp_size);

        // Background checkerboard + image
        self.draw_checkerboard(&painter, img_rect);
        painter.image(
            tex_id,
            img_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        // Convert crop rect to screen coords
        let crop_screen = Rect::from_min_size(
            Pos2::new(
                img_rect.left() + (self.crop.x as f32) * scale,
                img_rect.top() + (self.crop.y as f32) * scale,
            ),
            Vec2::new((self.crop.width as f32) * scale, (self.crop.height as f32) * scale),
        );

        // Interactive crop handle hit-testing & cursor feedback
        if let Some(hover_pos) = response.hover_pos() {
            if let Some(handle) = detect_crop_handle(hover_pos, crop_screen) {
                ctx.set_cursor_icon(handle_cursor(handle));
            } else if img_rect.contains(hover_pos) {
                ctx.set_cursor_icon(CursorIcon::Crosshair);
            }
        }

        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                let handle = detect_crop_handle(pos, crop_screen).or_else(|| {
                    if img_rect.contains(pos) {
                        Some(CropHandle::CreateNew)
                    } else {
                        None
                    }
                });
                if let Some(handle) = handle {
                    let img_pt = Vec2::new(
                        ((pos.x - img_rect.left()) / scale).clamp(0.0, img_w),
                        ((pos.y - img_rect.top()) / scale).clamp(0.0, img_h),
                    );
                    self.crop_drag = Some(CropDragState {
                        handle,
                        start_pointer_img: img_pt,
                        start_crop: self.crop,
                    });
                }
            }
        }

        if response.dragged() {
            if let (Some(drag), Some(pos)) = (self.crop_drag, response.interact_pointer_pos()) {
                let cur_img = Vec2::new(
                    (pos.x - img_rect.left()) / scale,
                    (pos.y - img_rect.top()) / scale,
                );
                let delta = cur_img - drag.start_pointer_img;
                let ratio = self.aspect_preset.ratio(gif.width, gif.height);
                let new_crop = apply_crop_drag(drag, delta, gif.width, gif.height, ratio);
                self.update_crop_with_scale_sync(new_crop);
            }
        }

        if response.drag_stopped() {
            self.crop_drag = None;
        }

        // Recompute crop_screen after drag update for crisp zero-lag rendering
        let crop_screen = Rect::from_min_size(
            Pos2::new(
                img_rect.left() + (self.crop.x as f32) * scale,
                img_rect.top() + (self.crop.y as f32) * scale,
            ),
            Vec2::new((self.crop.width as f32) * scale, (self.crop.height as f32) * scale),
        );

        // Dim outer cropped-away area (4 non-overlapping rectangles around crop_screen)
        let dim = Color32::from_black_alpha(150);
        // Top
        painter.rect_filled(
            Rect::from_min_max(img_rect.min, Pos2::new(img_rect.right(), crop_screen.top())),
            CornerRadius::ZERO,
            dim,
        );
        // Bottom
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(img_rect.left(), crop_screen.bottom()), img_rect.max),
            CornerRadius::ZERO,
            dim,
        );
        // Left
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(img_rect.left(), crop_screen.top()),
                Pos2::new(crop_screen.left(), crop_screen.bottom()),
            ),
            CornerRadius::ZERO,
            dim,
        );
        // Right
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(crop_screen.right(), crop_screen.top()),
                Pos2::new(img_rect.right(), crop_screen.bottom()),
            ),
            CornerRadius::ZERO,
            dim,
        );

        // Rule-of-thirds grid
        if self.show_grid && crop_screen.width() > 24.0 && crop_screen.height() > 24.0 {
            let grid_stroke = Stroke::new(1.0, Color32::from_white_alpha(70));
            for i in 1..=2 {
                let f = (i as f32) / 3.0;
                let gx = crop_screen.left() + crop_screen.width() * f;
                let gy = crop_screen.top() + crop_screen.height() * f;
                painter.line_segment(
                    [Pos2::new(gx, crop_screen.top()), Pos2::new(gx, crop_screen.bottom())],
                    grid_stroke,
                );
                painter.line_segment(
                    [Pos2::new(crop_screen.left(), gy), Pos2::new(crop_screen.right(), gy)],
                    grid_stroke,
                );
            }
        }

        // Crop border
        painter.rect_stroke(
            crop_screen,
            CornerRadius::ZERO,
            Stroke::new(2.0, ACCENT),
            StrokeKind::Inside,
        );

        // 8 resize handles
        let handle_pts = [
            crop_screen.left_top(),
            crop_screen.right_top(),
            crop_screen.left_bottom(),
            crop_screen.right_bottom(),
            Pos2::new(crop_screen.center().x, crop_screen.top()),
            Pos2::new(crop_screen.center().x, crop_screen.bottom()),
            Pos2::new(crop_screen.left(), crop_screen.center().y),
            Pos2::new(crop_screen.right(), crop_screen.center().y),
        ];
        for pt in handle_pts {
            let hrect = Rect::from_center_size(pt, Vec2::splat(9.0));
            painter.rect_filled(hrect, CornerRadius::same(2), Color32::WHITE);
            painter.rect_stroke(
                hrect,
                CornerRadius::same(2),
                Stroke::new(1.5, ACCENT),
                StrokeKind::Outside,
            );
        }

        // Live dimension badge above crop box
        let badge_text = format!(
            "{}, {}  ·  {}×{}",
            self.crop.x, self.crop.y, self.crop.width, self.crop.height
        );
        let badge_pos = Pos2::new(
            crop_screen.center().x,
            (crop_screen.top() - 14.0).max(viewport_rect.top() + 14.0),
        );
        let galley = painter.layout_no_wrap(
            badge_text,
            FontId::monospace(11.5),
            Color32::WHITE,
        );
        let badge_bg = Rect::from_center_size(badge_pos, galley.size() + Vec2::new(12.0, 6.0));
        painter.rect_filled(badge_bg, CornerRadius::same(4), Color32::from_black_alpha(200));
        painter.galley(badge_bg.min + Vec2::new(6.0, 3.0), galley, Color32::WHITE);
    }

    fn draw_live_preview_viewport(&mut self, ctx: &egui::Context, ui: &mut Ui) {
        let s = Strings::new(self.lang);
        let crisp = matches!(self.preview_zoom, PreviewZoom::Percent(p) if p >= 100);
        let Some(tex_id) = self.ensure_preview_texture(ctx, crisp) else {
            return;
        };

        let tw = self.target_w.max(1) as f32;
        let th = self.target_h.max(1) as f32;

        egui::ScrollArea::both().show(ui, |ui| {
            let avail = ui.available_size();
            let disp_size = match self.preview_zoom {
                PreviewZoom::Fit => {
                    let pad = 32.0;
                    let max_w = (avail.x - pad).max(16.0);
                    let max_h = (avail.y - pad).max(16.0);
                    let sc = (max_w / tw).min(max_h / th);
                    Vec2::new(tw * sc, th * sc)
                }
                PreviewZoom::Percent(p) => {
                    let sc = (p as f32) / 100.0;
                    Vec2::new(tw * sc, th * sc)
                }
            };

            let alloc_size = Vec2::new(avail.x.max(disp_size.x + 32.0), avail.y.max(disp_size.y + 32.0));
            let (area_rect, _) = ui.allocate_exact_size(alloc_size, Sense::hover());
            let painter = ui.painter_at(area_rect);
            let img_rect = Rect::from_center_size(area_rect.center(), disp_size);

            self.draw_checkerboard(&painter, img_rect);
            painter.image(
                tex_id,
                img_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            painter.rect_stroke(
                img_rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, BORDER_SUBTLE),
                StrokeKind::Outside,
            );

            // Overlay caption pill
            let caption = format!(
                "{}×{}  ·  {}",
                self.target_w,
                self.target_h,
                self.algorithm.label(&s)
            );
            let galley = painter.layout_no_wrap(caption, FontId::proportional(12.0), Color32::WHITE);
            let pill_pos = Pos2::new(area_rect.left() + 12.0, area_rect.top() + 8.0);
            let pill_rect = Rect::from_min_size(pill_pos, galley.size() + Vec2::new(14.0, 8.0));
            painter.rect_filled(pill_rect, CornerRadius::same(6), Color32::from_black_alpha(180));
            painter.galley(pill_pos + Vec2::new(7.0, 4.0), galley, Color32::WHITE);
        });
    }

    fn draw_compare_viewport(&mut self, ctx: &egui::Context, ui: &mut Ui) {
        let s = Strings::new(self.lang);
        let Some(orig_tex) = self.ensure_compare_orig_texture(ctx) else {
            return;
        };
        let Some(resized_tex) = self.ensure_preview_texture(ctx, true) else {
            return;
        };

        let avail = ui.available_rect_before_wrap();
        let (rect, _) = ui.allocate_exact_size(avail.size(), Sense::hover());
        let painter = ui.painter_at(rect);

        let gap = 14.0;
        let half_w = ((rect.width() - gap) * 0.5).max(32.0);
        let left_panel = Rect::from_min_size(rect.min, Vec2::new(half_w, rect.height()));
        let right_panel = Rect::from_min_size(
            Pos2::new(rect.left() + half_w + gap, rect.top()),
            Vec2::new(half_w, rect.height()),
        );

        let panels = [
            (
                left_panel,
                orig_tex,
                self.crop.width as f32,
                self.crop.height as f32,
                format!("{} ({}×{})", s.compare_original(), self.crop.width, self.crop.height),
            ),
            (
                right_panel,
                resized_tex,
                self.target_w as f32,
                self.target_h as f32,
                format!(
                    "{} ({}×{} · {})",
                    s.compare_resized(),
                    self.target_w,
                    self.target_h,
                    self.algorithm.label(&s)
                ),
            ),
        ];

        for (panel_rect, tex_id, w, h, title) in panels {
            painter.rect_filled(panel_rect, CornerRadius::same(8), BG_PANEL);
            painter.rect_stroke(
                panel_rect,
                CornerRadius::same(8),
                Stroke::new(1.0, BORDER_SUBTLE),
                StrokeKind::Inside,
            );

            let header_h = 28.0;
            painter.text(
                Pos2::new(panel_rect.center().x, panel_rect.top() + 14.0),
                Align2::CENTER_CENTER,
                title,
                FontId::proportional(12.0),
                Color32::from_rgb(215, 224, 238),
            );

            let inner = Rect::from_min_max(
                Pos2::new(panel_rect.left() + 12.0, panel_rect.top() + header_h + 6.0),
                Pos2::new(panel_rect.right() - 12.0, panel_rect.bottom() - 12.0),
            );
            let sc = (inner.width() / w.max(1.0)).min(inner.height() / h.max(1.0));
            let img_rect = Rect::from_center_size(inner.center(), Vec2::new(w * sc, h * sc));

            self.draw_checkerboard(&painter, img_rect);
            painter.image(
                tex_id,
                img_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
}

impl eframe::App for SnipApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_checker_texture(ctx);
        self.poll_export_job();
        if self.export_job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        self.handle_shortcuts(ctx);
        self.tick_animation(ctx);

        self.draw_top_bar(ctx);
        self.draw_bottom_timeline(ctx);
        self.draw_inspector(ctx);
        self.draw_center_workspace(ctx);
        self.handle_file_drop(ctx);
    }
}

fn section_card(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(BG_CARD)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(10))
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .size(13.0)
                    .strong()
                    .color(Color32::WHITE),
            );
            ui.add_space(6.0);
            add_contents(ui);
        });
}

fn detect_crop_handle(pos: Pos2, r: Rect) -> Option<CropHandle> {
    let hit = 10.0;
    let corners = [
        (r.left_top(), CropHandle::NW),
        (r.right_top(), CropHandle::NE),
        (r.left_bottom(), CropHandle::SW),
        (r.right_bottom(), CropHandle::SE),
    ];
    for (pt, h) in corners {
        if pos.distance(pt) <= hit {
            return Some(h);
        }
    }

    let on_x = pos.x >= r.left() - hit && pos.x <= r.right() + hit;
    let on_y = pos.y >= r.top() - hit && pos.y <= r.bottom() + hit;

    if on_x && (pos.y - r.top()).abs() <= hit {
        return Some(CropHandle::N);
    }
    if on_x && (pos.y - r.bottom()).abs() <= hit {
        return Some(CropHandle::S);
    }
    if on_y && (pos.x - r.left()).abs() <= hit {
        return Some(CropHandle::W);
    }
    if on_y && (pos.x - r.right()).abs() <= hit {
        return Some(CropHandle::E);
    }
    if r.contains(pos) {
        return Some(CropHandle::Move);
    }
    None
}

fn handle_cursor(handle: CropHandle) -> CursorIcon {
    match handle {
        CropHandle::Move => CursorIcon::Grab,
        CropHandle::CreateNew => CursorIcon::Crosshair,
        CropHandle::NW | CropHandle::SE => CursorIcon::ResizeNwSe,
        CropHandle::NE | CropHandle::SW => CursorIcon::ResizeNeSw,
        CropHandle::N | CropHandle::S => CursorIcon::ResizeVertical,
        CropHandle::W | CropHandle::E => CursorIcon::ResizeHorizontal,
    }
}

fn apply_crop_drag(
    drag: CropDragState,
    delta: Vec2,
    img_w: u32,
    img_h: u32,
    ratio: Option<f32>,
) -> CropRect {
    let max_w = img_w as f32;
    let max_h = img_h as f32;
    let mut left = drag.start_crop.x as f32;
    let mut top = drag.start_crop.y as f32;
    let mut right = (drag.start_crop.x + drag.start_crop.width) as f32;
    let mut bottom = (drag.start_crop.y + drag.start_crop.height) as f32;

    match drag.handle {
        CropHandle::CreateNew => {
            let p0 = drag.start_pointer_img;
            let p1 = Vec2::new(
                (p0.x + delta.x).clamp(0.0, max_w),
                (p0.y + delta.y).clamp(0.0, max_h),
            );
            let dx = p1.x - p0.x;
            let dy = p1.y - p0.y;
            let (w, h) = if let Some(r) = ratio {
                let abs_w = dx.abs().max(1.0);
                let abs_h = (abs_w / r).max(1.0);
                (abs_w, abs_h)
            } else {
                (dx.abs().max(1.0), dy.abs().max(1.0))
            };
            let x0 = if dx >= 0.0 { p0.x } else { (p0.x - w).max(0.0) };
            let y0 = if dy >= 0.0 { p0.y } else { (p0.y - h).max(0.0) };
            let x = x0.round().clamp(0.0, max_w - 1.0) as u32;
            let y = y0.round().clamp(0.0, max_h - 1.0) as u32;
            let w_u = w.round().clamp(1.0, (img_w - x) as f32) as u32;
            let h_u = h.round().clamp(1.0, (img_h - y) as f32) as u32;
            return CropRect {
                x,
                y,
                width: w_u,
                height: h_u,
            };
        }
        CropHandle::Move => {
            let w = right - left;
            let h = bottom - top;
            let nx = (left + delta.x).clamp(0.0, max_w - w);
            let ny = (top + delta.y).clamp(0.0, max_h - h);
            return CropRect {
                x: nx.round() as u32,
                y: ny.round() as u32,
                width: w.round().max(1.0) as u32,
                height: h.round().max(1.0) as u32,
            };
        }
        CropHandle::NW => {
            left = (left + delta.x).clamp(0.0, right - 1.0);
            top = (top + delta.y).clamp(0.0, bottom - 1.0);
            if let Some(r) = ratio {
                let w = right - left;
                let h = (w / r).clamp(1.0, bottom);
                top = bottom - h;
                left = right - h * r;
            }
        }
        CropHandle::NE => {
            right = (right + delta.x).clamp(left + 1.0, max_w);
            top = (top + delta.y).clamp(0.0, bottom - 1.0);
            if let Some(r) = ratio {
                let w = right - left;
                let h = (w / r).clamp(1.0, bottom);
                top = bottom - h;
                right = left + h * r;
            }
        }
        CropHandle::SW => {
            left = (left + delta.x).clamp(0.0, right - 1.0);
            bottom = (bottom + delta.y).clamp(top + 1.0, max_h);
            if let Some(r) = ratio {
                let w = right - left;
                let h = (w / r).clamp(1.0, max_h - top);
                bottom = top + h;
                left = right - h * r;
            }
        }
        CropHandle::SE => {
            right = (right + delta.x).clamp(left + 1.0, max_w);
            bottom = (bottom + delta.y).clamp(top + 1.0, max_h);
            if let Some(r) = ratio {
                let w = right - left;
                let h = (w / r).clamp(1.0, max_h - top);
                bottom = top + h;
                right = left + h * r;
            }
        }
        CropHandle::N => {
            top = (top + delta.y).clamp(0.0, bottom - 1.0);
            if let Some(r) = ratio {
                let h = bottom - top;
                let cx = (left + right) * 0.5;
                let w = (h * r).min(max_w);
                left = (cx - w * 0.5).clamp(0.0, max_w - w);
                right = left + w;
                top = bottom - (w / r);
            }
        }
        CropHandle::S => {
            bottom = (bottom + delta.y).clamp(top + 1.0, max_h);
            if let Some(r) = ratio {
                let h = bottom - top;
                let cx = (left + right) * 0.5;
                let w = (h * r).min(max_w);
                left = (cx - w * 0.5).clamp(0.0, max_w - w);
                right = left + w;
                bottom = top + (w / r);
            }
        }
        CropHandle::W => {
            left = (left + delta.x).clamp(0.0, right - 1.0);
            if let Some(r) = ratio {
                let w = right - left;
                let cy = (top + bottom) * 0.5;
                let h = (w / r).min(max_h);
                top = (cy - h * 0.5).clamp(0.0, max_h - h);
                bottom = top + h;
                left = right - (h * r);
            }
        }
        CropHandle::E => {
            right = (right + delta.x).clamp(left + 1.0, max_w);
            if let Some(r) = ratio {
                let w = right - left;
                let cy = (top + bottom) * 0.5;
                let h = (w / r).min(max_h);
                top = (cy - h * 0.5).clamp(0.0, max_h - h);
                bottom = top + h;
                right = left + (h * r);
            }
        }
    }

    let x = left.round().clamp(0.0, max_w - 1.0) as u32;
    let y = top.round().clamp(0.0, max_h - 1.0) as u32;
    let w = (right - left).round().clamp(1.0, (img_w - x) as f32) as u32;
    let h = (bottom - top).round().clamp(1.0, (img_h - y) as f32) as u32;
    CropRect {
        x,
        y,
        width: w,
        height: h,
    }
}
