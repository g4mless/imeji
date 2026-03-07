#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use argh::FromArgs;
use eframe::egui;
use std::path::{Path, PathBuf};

mod wic;

fn main() -> eframe::Result {
    let cli: Cli = argh::from_env();
    let mut options = eframe::NativeOptions::default();
    let initial_path = cli.file.map(PathBuf::from);

    options.persist_window = true;
    options.vsync = true;
    options.multisampling = 0;

    let mut viewport_builder = egui::ViewportBuilder::default().with_min_inner_size([480.0, 480.0]);

    if let Ok(icon) = load_icon() {
        viewport_builder = viewport_builder.with_icon(icon);
    }

    options.viewport = viewport_builder;

    eframe::run_native(
        "Imeji",
        options,
        Box::new(move |_| {
            let mut app = Imeji::new();
            if let Some(p) = initial_path {
                if let Err(err) = app.load_image_from_path(&p) {
                    let filename = p.file_name().map(|n| n.to_string_lossy().to_string());
                    app.last_error = Some(format_load_error(Some(&p), filename.as_deref(), &err));
                }
            }
            Ok(Box::new(app))
        }),
    )
}

fn load_icon() -> Result<egui::IconData, Box<dyn std::error::Error>> {
    let wic = wic::WicContext::new()?;
    let (rgba, width, height) = wic.load_from_memory(include_bytes!("../icon.ico"))?;
    Ok(egui::IconData {
        rgba,
        width,
        height,
    })
}

#[derive(Default)]
struct Imeji {
    image_levels: Vec<egui::ColorImage>,
    base_image_size: Option<egui::Vec2>,
    textures: Vec<egui::TextureHandle>,
    filename: Option<String>,
    current_path: Option<PathBuf>,
    zoom: f32,
    pan_offset: egui::Vec2,
    is_dragging: bool,
    last_mouse_pos: Option<egui::Pos2>,
    last_window_size: Option<egui::Vec2>,
    is_animating_to_center: bool,
    animation_start_offset: egui::Vec2,
    animation_start_time: Option<std::time::Instant>,
    last_title: Option<String>,
    last_error: Option<String>,
}

impl Imeji {
    fn new() -> Self {
        Self {
            image_levels: Vec::new(),
            base_image_size: None,
            textures: Vec::new(),
            filename: None,
            current_path: None,
            zoom: 1.0,
            pan_offset: egui::Vec2::ZERO,
            is_dragging: false,
            last_mouse_pos: None,
            last_window_size: None,
            is_animating_to_center: false,
            animation_start_offset: egui::Vec2::ZERO,
            animation_start_time: None,
            last_title: None,
            last_error: None,
        }
    }
}

#[derive(FromArgs)]
/// load img from file explorer
struct Cli {
    #[argh(positional)]
    file: Option<String>,
}

impl eframe::App for Imeji {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // only repaint on input or when requested (animations, etc.)

        // Poll input once and store results to avoid multiple calls
        let input = ctx.input(|i| {
            let dropped_files = i.raw.dropped_files.clone();
            let smooth_scroll_delta = i.smooth_scroll_delta.y;
            let zoom_delta = i.zoom_delta();
            let mouse_pos = i.pointer.hover_pos();
            let is_primary_down = i.pointer.primary_down();
            let ctrl_down = i.modifiers.ctrl;

            (
                dropped_files,
                smooth_scroll_delta,
                zoom_delta,
                mouse_pos,
                is_primary_down,
                ctrl_down,
            )
        });

        let (dropped_files, smooth_scroll_delta, zoom_delta, mouse_pos, is_primary_down, ctrl_down) =
            input;

        // Handle keyboard shortcut separately (needs mutable access)
        let keyboard_shortcut = ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::W,
            ))
        });
        let nav_prev =
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft));
        let nav_next =
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight));

        // Update window title when it changes
        let title = self.filename.as_deref().unwrap_or("Imeji");
        if self.last_title.as_deref() != Some(title) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.to_string()));
            self.last_title = Some(title.to_string());
        }

        // Handle dropped files
        if !dropped_files.is_empty() {
            let dropped_file = &dropped_files[0];
            let filename = dropped_file
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .or_else(|| {
                    if dropped_file.name.is_empty() {
                        None
                    } else {
                        Some(dropped_file.name.clone())
                    }
                });

            if let Some(bytes) = &dropped_file.bytes {
                if let Err(err) = self.load_image(bytes, filename.clone()) {
                    self.last_error = Some(format_load_error(None, filename.as_deref(), &err));
                }
                ctx.request_repaint();
            } else if let Some(path) = &dropped_file.path {
                if let Err(err) = self.load_image_from_path(path) {
                    self.last_error = Some(format_load_error(Some(path), filename.as_deref(), &err));
                }
                ctx.request_repaint();
            }
        }

        // Ctrl+W = Close Image
        if keyboard_shortcut {
            self.image_levels.clear();
            self.base_image_size = None;
            self.textures.clear();
            self.filename = None;
            self.current_path = None;
            self.zoom = 1.0;
            self.pan_offset = egui::Vec2::ZERO;
            self.last_error = None;
        }

        if nav_prev {
            if let Err(err) = self.load_adjacent_image(-1) {
                self.last_error = Some(err);
            }
            ctx.request_repaint();
        } else if nav_next {
            if let Err(err) = self.load_adjacent_image(1) {
                self.last_error = Some(err);
            }
            ctx.request_repaint();
        }

        let current_window_size = ctx.screen_rect().size();
        if let Some(last_size) = self.last_window_size {
            if current_window_size != last_size {
                let size_increase = current_window_size - last_size;
                if size_increase.x > 100.0 || size_increase.y > 100.0 {
                    self.zoom = 1.0;
                    self.pan_offset = egui::Vec2::ZERO;
                }
                self.last_window_size = Some(current_window_size);
            }
        } else {
            self.last_window_size = Some(current_window_size);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE) // Remove default frame/padding
            .show(ctx, |ui| {
                if let Some(base_image_size) = self.base_image_size {
                    if self.textures.len() != self.image_levels.len() && !self.image_levels.is_empty()
                    {
                        let texture_options = egui::TextureOptions {
                            magnification: egui::TextureFilter::Linear,
                            minification: egui::TextureFilter::Linear,
                            wrap_mode: egui::TextureWrapMode::ClampToEdge,
                            mipmap_mode: None,
                        };
                        let levels = std::mem::take(&mut self.image_levels);
                        self.textures = levels
                            .into_iter()
                            .enumerate()
                            .map(|(i, image)| {
                                ctx.load_texture(
                                    format!("loaded_image_mip_{i}"),
                                    image,
                                    texture_options,
                                )
                            })
                            .collect();
                    }

                    let screen_rect = ctx.screen_rect();
                    // Use screen rect instead of available_size to avoid UI padding
                    let available_size = screen_rect.size();

                    let mut zoom_factor = 1.0;
                    if (zoom_delta - 1.0).abs() > f32::EPSILON {
                        zoom_factor = zoom_delta;
                    } else if ctrl_down && smooth_scroll_delta != 0.0 {
                        // Keep Ctrl+wheel zoom as a desktop fallback without hijacking normal scroll.
                        zoom_factor = 1.0 + smooth_scroll_delta * 0.001;
                    }

                    if (zoom_factor - 1.0).abs() > f32::EPSILON {
                        let old_zoom = self.zoom;
                        self.zoom = (self.zoom * zoom_factor).clamp(1.0, 10.0);

                        // Start animation to center when zoom reaches 1.0
                        if self.zoom == 1.0 && old_zoom > 1.0 {
                            self.is_animating_to_center = true;
                            self.animation_start_offset = self.pan_offset;
                            self.animation_start_time = Some(std::time::Instant::now());
                        } else if self.zoom > 1.0 {
                            // Stop animation if zooming back in
                            self.is_animating_to_center = false;
                            if let Some(current_mouse_pos) = mouse_pos {
                                let center = screen_rect.center();
                                let mouse_offset = current_mouse_pos - center;
                                let zoom_change = self.zoom / old_zoom - 1.0;
                                self.pan_offset -= mouse_offset * zoom_change;
                            }
                        }

                        ctx.request_repaint();
                    }

                    // Handle animation to center
                    if self.is_animating_to_center
                        && let Some(start_time) = self.animation_start_time
                    {
                        let elapsed = start_time.elapsed().as_secs_f32();
                        let animation_duration = 0.3; // 300ms animation

                        if elapsed >= animation_duration {
                            // Animation complete
                            self.pan_offset = egui::Vec2::ZERO;
                            self.is_animating_to_center = false;
                            self.animation_start_time = None;
                        } else {
                            // Smooth easing function (ease-out)
                            let t = elapsed / animation_duration;
                            let eased_t = 1.0 - (1.0 - t).powi(3); // cubic ease-out

                            // Interpolate from start offset to zero
                            self.pan_offset = self.animation_start_offset * (1.0 - eased_t);

                            // Schedule next animation frame (~60 FPS)
                            ctx.request_repaint_after(std::time::Duration::from_millis(16));
                        }
                    }

                    if let Some(current_pos) = mouse_pos.filter(|_| is_primary_down) {
                        if !self.is_dragging {
                            self.is_dragging = true;
                            self.last_mouse_pos = Some(current_pos);
                        } else if let Some(last_pos) = self.last_mouse_pos {
                            let delta = current_pos - last_pos;
                            // Only allow panning when zoom is greater than 1.0
                            if self.zoom > 1.0 {
                                self.pan_offset += delta;
                                ctx.request_repaint();
                            }
                            self.last_mouse_pos = Some(current_pos);
                        }
                    } else {
                        self.is_dragging = false;
                        self.last_mouse_pos = None;
                    }

                    let base_scale = (available_size.x / base_image_size.x)
                        .min(available_size.y / base_image_size.y)
                        .min(1.0);

                    let effective_scale = base_scale * self.zoom;
                    let mip_level = pick_mip_level(effective_scale, self.textures.len());
                    let texture = &self.textures[mip_level];

                    let display_size = base_image_size * effective_scale;
                    let center = ui.available_rect_before_wrap().center();
                    let image_pos = center - display_size * 0.5 + self.pan_offset;

                    let image_rect = egui::Rect::from_min_size(image_pos, display_size);
                    let _response = ui.allocate_rect(
                        ui.available_rect_before_wrap(),
                        egui::Sense::click_and_drag(),
                    );
                    let snapped_image_rect =
                        snap_rect_to_pixels(image_rect, ctx.pixels_per_point());

                    if ui.is_rect_visible(snapped_image_rect) {
                        ui.painter().image(
                            texture.id(),
                            snapped_image_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                }
            });

        if let Some(error) = self.last_error.as_deref() {
            egui::Area::new("load_error".into())
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 16.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(520.0);
                        ui.colored_label(egui::Color32::from_rgb(196, 64, 64), error);
                    });
                });
        }
    }
}

impl Imeji {
    fn load_image(&mut self, bytes: &[u8], filename: Option<String>) -> Result<(), String> {
        let wic_result = wic::WicContext::new().and_then(|w| w.load_from_memory(bytes));
        match wic_result {
            Ok((rgba, width, height)) => {
                let size = [width as usize, height as usize];
                let base_image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                self.image_levels = build_mip_chain(base_image);
                self.base_image_size = Some(egui::vec2(width as f32, height as f32));
                self.filename = filename;
                self.current_path = None;
                self.textures.clear();
                // Reset zoom and pan when loading new image
                self.zoom = 1.0;
                self.pan_offset = egui::Vec2::ZERO;
                self.is_dragging = false;
                self.last_mouse_pos = None;
                self.last_window_size = None;
                self.is_animating_to_center = false;
                self.animation_start_time = None;
                self.last_error = None;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn load_image_from_path(&mut self, path: &Path) -> Result<(), String> {
        let filename = path.file_name().map(|n| n.to_string_lossy().to_string());
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        self.load_image(&bytes, filename)?;
        self.current_path = Some(path.to_path_buf());
        Ok(())
    }

    fn load_adjacent_image(&mut self, direction: isize) -> Result<(), String> {
        let Some(current_path) = self.current_path.clone() else {
            return Ok(());
        };
        if direction == 0 {
            return Ok(());
        }

        let parent = current_path
            .parent()
            .ok_or_else(|| "Current image has no parent directory".to_string())?;

        let mut files: Vec<PathBuf> = std::fs::read_dir(parent)
            .map_err(|e| e.to_string())?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.is_file() && is_supported_image(path))
            .collect();

        files.sort_by_key(|path| {
            path.file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        });

        let Some(current_idx) = files.iter().position(|p| p == &current_path) else {
            return Ok(());
        };

        let next_idx = if direction > 0 {
            current_idx + 1
        } else if current_idx == 0 {
            return Ok(());
        } else {
            current_idx - 1
        };

        let Some(next_path) = files.get(next_idx) else {
            return Ok(());
        };

        self.load_image_from_path(next_path).map_err(|e| {
            let filename = next_path.file_name().map(|n| n.to_string_lossy().to_string());
            format_load_error(Some(next_path), filename.as_deref(), &e)
        })
    }
}

fn is_supported_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("jpg")
            | Some("jpeg")
            | Some("png")
            | Some("bmp")
            | Some("gif")
            | Some("webp")
            | Some("tif")
            | Some("tiff")
            | Some("ico")
            | Some("jxl")
            | Some("avif")
    )
}

fn format_load_error(path: Option<&Path>, filename: Option<&str>, error: &str) -> String {
    let target = path
        .map(|p| p.display().to_string())
        .or_else(|| filename.map(ToOwned::to_owned))
        .unwrap_or_else(|| "image".to_string());
    format!("Failed to open {target}: {error}")
}

fn snap_rect_to_pixels(rect: egui::Rect, pixels_per_point: f32) -> egui::Rect {
    let min = (rect.min.to_vec2() * pixels_per_point).round() / pixels_per_point;
    let max = (rect.max.to_vec2() * pixels_per_point).round() / pixels_per_point;
    egui::Rect::from_min_max(min.to_pos2(), max.to_pos2())
}

fn pick_mip_level(scale: f32, max_levels: usize) -> usize {
    if max_levels <= 1 || scale >= 1.0 {
        return 0;
    }

    let desired = (1.0 / scale).log2().floor().max(0.0) as usize;
    desired.min(max_levels.saturating_sub(1))
}

fn build_mip_chain(base: egui::ColorImage) -> Vec<egui::ColorImage> {
    let mut levels = vec![base];

    let mut srgb_to_linear_lut = [0u16; 256];
    for (i, v) in srgb_to_linear_lut.iter_mut().enumerate() {
        *v = srgb_u8_to_linear_u16(i as u8);
    }

    loop {
        let Some(prev) = levels.last() else {
            break;
        };

        let prev_w = prev.size[0];
        let prev_h = prev.size[1];
        if prev_w == 1 && prev_h == 1 {
            break;
        }

        let next_w = (prev_w / 2).max(1);
        let next_h = (prev_h / 2).max(1);
        let mut next_pixels = Vec::with_capacity(next_w * next_h);

        // Fast 2x2 mip generation, but average colors in linear space (gamma-correct).
        for y in 0..next_h {
            for x in 0..next_w {
                let mut r_sum = 0u64;
                let mut g_sum = 0u64;
                let mut b_sum = 0u64;
                let mut alpha_sum = 0u64;
                let mut count = 0u64;

                for oy in 0..2usize {
                    for ox in 0..2usize {
                        let sx = (x * 2 + ox).min(prev_w - 1);
                        let sy = (y * 2 + oy).min(prev_h - 1);
                        let p = prev.pixels[sy * prev_w + sx];
                        let alpha = p.a() as u64;
                        r_sum += srgb_to_linear_lut[p.r() as usize] as u64 * alpha;
                        g_sum += srgb_to_linear_lut[p.g() as usize] as u64 * alpha;
                        b_sum += srgb_to_linear_lut[p.b() as usize] as u64 * alpha;
                        alpha_sum += alpha;
                        count += 1;
                    }
                }

                let (r, g, b) = if alpha_sum == 0 {
                    (0, 0, 0)
                } else {
                    (
                        linear_u16_to_srgb_u8((r_sum / alpha_sum) as u16),
                        linear_u16_to_srgb_u8((g_sum / alpha_sum) as u16),
                        linear_u16_to_srgb_u8((b_sum / alpha_sum) as u16),
                    )
                };

                next_pixels.push(egui::Color32::from_rgba_unmultiplied(
                    r,
                    g,
                    b,
                    (alpha_sum / count) as u8,
                ));
            }
        }

        levels.push(egui::ColorImage {
            size: [next_w, next_h],
            pixels: next_pixels,
            source_size: egui::vec2(next_w as f32, next_h as f32),
        });
    }

    levels
}

fn srgb_u8_to_linear_u16(v: u8) -> u16 {
    let srgb = (v as f32) / 255.0;
    let linear = if srgb <= 0.04045 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    };
    (linear.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16
}

fn linear_u16_to_srgb_u8(v: u16) -> u8 {
    let linear = (v as f32) / 65535.0;
    let srgb = if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (srgb.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}
