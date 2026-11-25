use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, MulDiv, PixelType, ResizeAlg, ResizeOptions, Resizer};
use gif::{ColorOutput, DecodeOptions, DisposalMethod, Encoder, Frame, Repeat};

use crate::i18n::Strings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResizeAlgorithm {
    Nearest,
    Box,
    Bilinear,
    CatmullRom,
    Mitchell,
    Lanczos3,
}

impl ResizeAlgorithm {
    pub const ALL: [ResizeAlgorithm; 6] = [
        ResizeAlgorithm::Nearest,
        ResizeAlgorithm::Box,
        ResizeAlgorithm::Bilinear,
        ResizeAlgorithm::CatmullRom,
        ResizeAlgorithm::Mitchell,
        ResizeAlgorithm::Lanczos3,
    ];

    pub fn to_fir(self) -> ResizeAlg {
        match self {
            ResizeAlgorithm::Nearest => ResizeAlg::Nearest,
            ResizeAlgorithm::Box => ResizeAlg::Convolution(FilterType::Box),
            ResizeAlgorithm::Bilinear => ResizeAlg::Convolution(FilterType::Bilinear),
            ResizeAlgorithm::CatmullRom => ResizeAlg::Convolution(FilterType::CatmullRom),
            ResizeAlgorithm::Mitchell => ResizeAlg::Convolution(FilterType::Mitchell),
            ResizeAlgorithm::Lanczos3 => ResizeAlg::Convolution(FilterType::Lanczos3),
        }
    }

    pub fn label(self, s: &Strings) -> &'static str {
        match self {
            ResizeAlgorithm::Nearest => s.algo_nearest_name(),
            ResizeAlgorithm::Box => s.algo_box_name(),
            ResizeAlgorithm::Bilinear => s.algo_bilinear_name(),
            ResizeAlgorithm::CatmullRom => s.algo_catmull_name(),
            ResizeAlgorithm::Mitchell => s.algo_mitchell_name(),
            ResizeAlgorithm::Lanczos3 => s.algo_lanczos3_name(),
        }
    }

    pub fn description(self, s: &Strings) -> &'static str {
        match self {
            ResizeAlgorithm::Nearest => s.algo_nearest_desc(),
            ResizeAlgorithm::Box => s.algo_box_desc(),
            ResizeAlgorithm::Bilinear => s.algo_bilinear_desc(),
            ResizeAlgorithm::CatmullRom => s.algo_catmull_desc(),
            ResizeAlgorithm::Mitchell => s.algo_mitchell_desc(),
            ResizeAlgorithm::Lanczos3 => s.algo_lanczos3_desc(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportQuality {
    High,
    Balanced,
    Fast,
}

impl ExportQuality {
    pub const ALL: [ExportQuality; 3] = [
        ExportQuality::High,
        ExportQuality::Balanced,
        ExportQuality::Fast,
    ];

    pub fn neuquant_speed(self) -> i32 {
        match self {
            ExportQuality::High => 1,
            ExportQuality::Balanced => 10,
            ExportQuality::Fast => 20,
        }
    }

    pub fn label(self, s: &Strings) -> &'static str {
        match self {
            ExportQuality::High => s.quality_high(),
            ExportQuality::Balanced => s.quality_balanced(),
            ExportQuality::Fast => s.quality_fast(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CropRect {
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width: width.max(1),
            height: height.max(1),
        }
    }

    pub fn clamp_to(&mut self, max_w: u32, max_h: u32) {
        let max_w = max_w.max(1);
        let max_h = max_h.max(1);
        self.x = self.x.min(max_w - 1);
        self.y = self.y.min(max_h - 1);
        self.width = self.width.clamp(1, max_w - self.x);
        self.height = self.height.clamp(1, max_h - self.y);
    }
}

#[derive(Debug, Clone)]
pub struct GifFrame {
    /// Full-canvas RGBA8 pixel buffer (width * height * 4 bytes).
    pub rgba: Vec<u8>,
    /// Delay in 10ms centiseconds (GIF native unit).
    pub delay_cs: u16,
}

impl GifFrame {
    /// Playback duration in seconds. Per browser convention, 0/1cs frames play at 10cs (100ms) or 20ms min.
    pub fn delay_secs(&self) -> f64 {
        let cs = if self.delay_cs <= 1 { 10 } else { self.delay_cs };
        (cs as f64) * 0.01
    }
}

#[derive(Debug, Clone)]
pub struct GifData {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<GifFrame>,
    pub file_size: u64,
    pub file_name: String,
    pub repeat: Repeat,
}

impl GifData {
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn total_duration_secs(&self) -> f64 {
        self.frames.iter().map(|f| f.delay_secs()).sum()
    }

    pub fn range_duration_secs(&self, start: usize, end: usize) -> f64 {
        if self.frames.is_empty() {
            return 0.0;
        }
        let s = start.min(self.frames.len() - 1);
        let e = end.min(self.frames.len() - 1).max(s);
        self.frames[s..=e].iter().map(|f| f.delay_secs()).sum()
    }
}

/// Decodes a GIF file into full-canvas composited RGBA8 frames, respecting all GIF disposal methods.
pub fn load_gif(path: &Path) -> Result<GifData, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("Cannot read file metadata: {e}"))?;
    let file_size = metadata.len();
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled.gif".to_string());

    let file = File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
    let reader = BufReader::new(file);

    let mut opts = DecodeOptions::new();
    opts.set_color_output(ColorOutput::RGBA);

    let mut decoder = opts
        .read_info(reader)
        .map_err(|e| format!("Invalid GIF header: {e}"))?;

    let width = decoder.width() as u32;
    let height = decoder.height() as u32;
    if width == 0 || height == 0 {
        return Err("GIF has zero width or height".to_string());
    }

    let repeat = decoder.repeat();
    let canvas_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| "GIF dimensions overflow memory".to_string())?;

    let mut canvas = vec![0u8; canvas_len];
    let mut frames = Vec::new();

    while let Some(frame) = decoder
        .read_next_frame()
        .map_err(|e| format!("Error decoding GIF frame {}: {e}", frames.len()))?
    {
        let dispose = frame.dispose;
        let saved_canvas = if dispose == DisposalMethod::Previous {
            Some(canvas.clone())
        } else {
            None
        };

        let f_left = frame.left as u32;
        let f_top = frame.top as u32;
        let f_w = frame.width as u32;
        let f_h = frame.height as u32;
        let buf = &frame.buffer;

        for row in 0..f_h {
            let dst_y = f_top + row;
            if dst_y >= height {
                break;
            }
            for col in 0..f_w {
                let dst_x = f_left + col;
                if dst_x >= width {
                    break;
                }
                let src_idx = ((row * f_w + col) as usize) * 4;
                if src_idx + 3 >= buf.len() {
                    continue;
                }
                let alpha = buf[src_idx + 3];
                if alpha > 0 {
                    let dst_idx = ((dst_y * width + dst_x) as usize) * 4;
                    canvas[dst_idx] = buf[src_idx];
                    canvas[dst_idx + 1] = buf[src_idx + 1];
                    canvas[dst_idx + 2] = buf[src_idx + 2];
                    canvas[dst_idx + 3] = alpha;
                }
            }
        }

        frames.push(GifFrame {
            rgba: canvas.clone(),
            delay_cs: frame.delay,
        });

        match dispose {
            DisposalMethod::Background => {
                for row in 0..f_h {
                    let dst_y = f_top + row;
                    if dst_y >= height {
                        break;
                    }
                    for col in 0..f_w {
                        let dst_x = f_left + col;
                        if dst_x >= width {
                            break;
                        }
                        let dst_idx = ((dst_y * width + dst_x) as usize) * 4;
                        canvas[dst_idx] = 0;
                        canvas[dst_idx + 1] = 0;
                        canvas[dst_idx + 2] = 0;
                        canvas[dst_idx + 3] = 0;
                    }
                }
            }
            DisposalMethod::Previous => {
                if let Some(prev) = saved_canvas {
                    canvas = prev;
                }
            }
            DisposalMethod::Any | DisposalMethod::Keep => {}
        }
    }

    if frames.is_empty() {
        return Err("GIF contains no frames".to_string());
    }

    Ok(GifData {
        width,
        height,
        frames,
        file_size,
        file_name,
        repeat,
    })
}

/// Extracts a cropped RGBA8 region from a full-canvas RGBA8 frame.
pub fn crop_rgba(src: &[u8], src_w: u32, src_h: u32, mut crop: CropRect) -> Vec<u8> {
    crop.clamp_to(src_w, src_h);
    let mut out = vec![0u8; (crop.width as usize) * (crop.height as usize) * 4];
    let row_bytes = (crop.width as usize) * 4;
    let src_stride = (src_w as usize) * 4;
    let x_offset = (crop.x as usize) * 4;

    for row in 0..crop.height as usize {
        let src_start = ((crop.y as usize) + row) * src_stride + x_offset;
        let dst_start = row * row_bytes;
        out[dst_start..dst_start + row_bytes]
            .copy_from_slice(&src[src_start..src_start + row_bytes]);
    }
    out
}

/// Crops and resizes a single RGBA8 frame using the chosen algorithm (with alpha premultiplication for clean edges).
pub fn crop_and_resize_rgba(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    mut crop: CropRect,
    target_w: u32,
    target_h: u32,
    algo: ResizeAlgorithm,
) -> Result<Vec<u8>, String> {
    crop.clamp_to(src_w, src_h);
    let dst_w = target_w.max(1);
    let dst_h = target_h.max(1);

    let cropped = crop_rgba(src, src_w, src_h, crop);
    if crop.width == dst_w && crop.height == dst_h {
        return Ok(cropped);
    }

    let mut dst_image = Image::new(dst_w, dst_h, PixelType::U8x4);
    let mut resizer = Resizer::new();
    let options = ResizeOptions::new().resize_alg(algo.to_fir());

    if algo == ResizeAlgorithm::Nearest {
        let src_image = ImageRef::new(crop.width, crop.height, &cropped, PixelType::U8x4)
            .map_err(|e| format!("Resize source error: {e}"))?;
        resizer
            .resize(&src_image, &mut dst_image, &options)
            .map_err(|e| format!("Resize error: {e}"))?;
    } else {
        // Premultiply alpha before convolution filtering to prevent dark fringes on transparent edges.
        let mut premul_src = Image::from_vec_u8(crop.width, crop.height, cropped, PixelType::U8x4)
            .map_err(|e| format!("Resize source error: {e}"))?;
        let mul_div = MulDiv::default();
        mul_div
            .multiply_alpha_inplace(&mut premul_src)
            .map_err(|e| format!("Alpha premultiply error: {e}"))?;

        resizer
            .resize(&premul_src, &mut dst_image, &options)
            .map_err(|e| format!("Resize error: {e}"))?;

        mul_div
            .divide_alpha_inplace(&mut dst_image)
            .map_err(|e| format!("Alpha unpremultiply error: {e}"))?;
    }

    Ok(dst_image.into_vec())
}

#[derive(Debug, Clone)]
pub struct ExportConfig {
    pub crop: CropRect,
    pub start_frame: usize,
    pub end_frame: usize,
    pub target_w: u32,
    pub target_h: u32,
    pub algorithm: ResizeAlgorithm,
    pub quality: ExportQuality,
    pub speed_multiplier: f32,
}

#[derive(Debug, Clone)]
pub struct ExportResult {
    pub output_path: std::path::PathBuf,
    pub output_size: u64,
    pub frame_count: usize,
}

/// Exports cropped, trimmed, and resized frames to a GIF file on disk.
pub fn export_gif(
    gif: &GifData,
    cfg: &ExportConfig,
    output_path: &Path,
    progress_done: Option<Arc<AtomicUsize>>,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> Result<ExportResult, String> {
    if gif.frames.is_empty() {
        return Err("No frames to export".to_string());
    }

    let out_w = cfg.target_w.clamp(1, u16::MAX as u32) as u16;
    let out_h = cfg.target_h.clamp(1, u16::MAX as u32) as u16;
    let start = cfg.start_frame.min(gif.frames.len() - 1);
    let end = cfg.end_frame.min(gif.frames.len() - 1).max(start);
    let total = end - start + 1;

    let file = File::create(output_path).map_err(|e| format!("Cannot create output file: {e}"))?;
    let writer = BufWriter::new(file);

    let mut encoder = Encoder::new(writer, out_w, out_h, &[])
        .map_err(|e| format!("Cannot initialize GIF encoder: {e}"))?;
    encoder
        .set_repeat(gif.repeat)
        .map_err(|e| format!("Cannot set GIF repeat: {e}"))?;

    let speed = cfg.quality.neuquant_speed();
    let mult = cfg.speed_multiplier.clamp(0.1, 10.0);

    for (idx, frame_idx) in (start..=end).enumerate() {
        if let Some(ref cancel) = cancel_flag {
            if cancel.load(Ordering::Relaxed) {
                drop(encoder);
                let _ = std::fs::remove_file(output_path);
                return Err("CANCELLED".to_string());
            }
        }

        let src_frame = &gif.frames[frame_idx];
        let mut rgba = crop_and_resize_rgba(
            &src_frame.rgba,
            gif.width,
            gif.height,
            cfg.crop,
            out_w as u32,
            out_h as u32,
            cfg.algorithm,
        )?;

        // GIF only supports 1-bit transparency (alpha == 0 or opaque).
        // Snap semi-transparent border pixels cleanly so NeuQuant doesn't turn faint edge halos opaque.
        for px in rgba.chunks_exact_mut(4) {
            if px[3] < 128 {
                px[0] = 0;
                px[1] = 0;
                px[2] = 0;
                px[3] = 0;
            } else {
                px[3] = 255;
            }
        }

        let mut gif_frame = Frame::from_rgba_speed(out_w, out_h, &mut rgba, speed);
        let base_cs = if src_frame.delay_cs == 0 {
            10
        } else {
            src_frame.delay_cs
        };
        let adjusted_cs = ((base_cs as f32) / mult).round().clamp(2.0, 65535.0) as u16;
        gif_frame.delay = adjusted_cs;
        gif_frame.dispose = DisposalMethod::Background;

        encoder
            .write_frame(&gif_frame)
            .map_err(|e| format!("Failed writing frame {idx}: {e}"))?;

        if let Some(ref done) = progress_done {
            done.store(idx + 1, Ordering::Relaxed);
        }
    }

    drop(encoder);

    let output_size = std::fs::metadata(output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(ExportResult {
        output_path: output_path.to_path_buf(),
        output_size,
        frame_count: total,
    })
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    if (bytes as f64) >= MB {
        format!("{:.2} MB", (bytes as f64) / MB)
    } else if (bytes as f64) >= KB {
        format!("{:.1} KB", (bytes as f64) / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_synthetic_gif(path: &Path) {
        let file = File::create(path).unwrap();
        let mut encoder = Encoder::new(file, 16, 16, &[]).unwrap();
        encoder.set_repeat(Repeat::Infinite).unwrap();

        // Frame 0: Red top-left quadrant, transparent elsewhere, Keep disposal
        let mut rgba0 = vec![0u8; 16 * 16 * 4];
        for y in 0..8 {
            for x in 0..8 {
                let i = (y * 16 + x) * 4;
                rgba0[i] = 255;
                rgba0[i + 3] = 255;
            }
        }
        let mut f0 = Frame::from_rgba_speed(16, 16, &mut rgba0, 1);
        f0.delay = 10;
        f0.dispose = DisposalMethod::Keep;
        encoder.write_frame(&f0).unwrap();

        // Frame 1: Green bottom-right quadrant, Background disposal
        let mut rgba1 = vec![0u8; 16 * 16 * 4];
        for y in 8..16 {
            for x in 8..16 {
                let i = (y * 16 + x) * 4;
                rgba1[i + 1] = 255;
                rgba1[i + 3] = 255;
            }
        }
        let mut f1 = Frame::from_rgba_speed(16, 16, &mut rgba1, 1);
        f1.delay = 20;
        f1.dispose = DisposalMethod::Background;
        encoder.write_frame(&f1).unwrap();

        // Frame 2: Blue full frame
        let mut rgba2 = vec![0u8; 16 * 16 * 4];
        for px in rgba2.chunks_exact_mut(4) {
            px[2] = 255;
            px[3] = 255;
        }
        let mut f2 = Frame::from_rgba_speed(16, 16, &mut rgba2, 1);
        f2.delay = 15;
        encoder.write_frame(&f2).unwrap();
    }

    #[test]
    fn test_decode_crop_resize_and_export_all_algorithms() {
        let tmp = std::env::temp_dir();
        let src_path = tmp.join("snip_test_input.gif");
        let out_path = tmp.join("snip_test_output.gif");

        create_synthetic_gif(&src_path);
        let gif = load_gif(&src_path).expect("should decode synthetic gif");
        assert_eq!(gif.width, 16);
        assert_eq!(gif.height, 16);
        assert_eq!(gif.frame_count(), 3);

        // Verify compositing on Frame 1 kept Frame 0's red pixel at (0,0) AND added green at (15,15)
        let f1 = &gif.frames[1].rgba;
        assert!(f1[0] > 200 && f1[3] == 255, "red pixel should persist from Keep disposal");
        let br = (15 * 16 + 15) * 4;
        assert!(f1[br + 1] > 200 && f1[br + 3] == 255, "green pixel should be present");

        // Test every resize algorithm with crop
        let crop = CropRect {
            x: 2,
            y: 2,
            width: 12,
            height: 12,
        };
        for algo in ResizeAlgorithm::ALL {
            let resized = crop_and_resize_rgba(&gif.frames[0].rgba, 16, 16, crop, 24, 18, algo)
                .expect("resize should succeed");
            assert_eq!(resized.len(), 24 * 18 * 4);
        }

        // Test export with trim [1..=2] and Lanczos3
        let cfg = ExportConfig {
            crop,
            start_frame: 1,
            end_frame: 2,
            target_w: 24,
            target_h: 24,
            algorithm: ResizeAlgorithm::Lanczos3,
            quality: ExportQuality::Balanced,
            speed_multiplier: 1.0,
        };
        let res = export_gif(&gif, &cfg, &out_path, None, None).expect("export should succeed");
        assert_eq!(res.frame_count, 2);
        assert!(res.output_size > 0);

        let redecoded = load_gif(&out_path).expect("exported gif should be decodable");
        assert_eq!(redecoded.width, 24);
        assert_eq!(redecoded.height, 24);
        assert_eq!(redecoded.frame_count(), 2);

        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&out_path);
    }
}
