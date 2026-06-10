#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use argh::FromArgs;
use eframe::egui;
use std::cmp::Ordering;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use windows::Win32::UI::Shell::StrCmpLogicalW;
use windows::core::PCWSTR;

mod explorer;
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
        Box::new(move |cc| {
            let mut app = Imeji::new(cc.egui_ctx.clone());
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

struct Imeji {
    wic: Option<wic::WicContext>,
    decode_tx: mpsc::Sender<DecodeRequest>,
    decode_rx: mpsc::Receiver<DecodeResult>,
    generation_counter: u64,
    /// The image currently on screen.
    current: Option<LoadedImage>,
    /// A newly opened image that keeps `current` visible until its first
    /// texture level finishes decoding.
    pending_image: Option<LoadedImage>,
    current_path: Option<PathBuf>,
    current_dir_images: Vec<PathBuf>,
    current_dir_index: Option<usize>,
    current_dir_parent: Option<PathBuf>,
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
    fn new(ctx: egui::Context) -> Self {
        let (decode_tx, decode_rx) = spawn_decode_worker(ctx);
        Self {
            wic: None,
            decode_tx,
            decode_rx,
            generation_counter: 0,
            current: None,
            pending_image: None,
            current_path: None,
            current_dir_images: Vec::new(),
            current_dir_index: None,
            current_dir_parent: None,
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
        let title = self
            .current
            .as_ref()
            .and_then(|img| img.filename.as_deref())
            .unwrap_or("Imeji");
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
                if let Err(err) = self.load_image(bytes.clone(), filename.clone()) {
                    self.last_error = Some(format_load_error(None, filename.as_deref(), &err));
                } else {
                    self.current_path = None;
                    self.current_dir_images.clear();
                    self.current_dir_index = None;
                    self.current_dir_parent = None;
                }
                ctx.request_repaint();
            } else if let Some(path) = &dropped_file.path {
                if let Err(err) = self.load_image_from_path(path) {
                    self.last_error =
                        Some(format_load_error(Some(path), filename.as_deref(), &err));
                }
                ctx.request_repaint();
            }
        }

        // Ctrl+W = Close Image
        if keyboard_shortcut {
            self.current = None;
            self.pending_image = None;
            self.current_path = None;
            self.current_dir_images.clear();
            self.current_dir_index = None;
            self.current_dir_parent = None;
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

        // Apply finished background decodes, then make sure a pending image
        // has its first texture level on the way.
        while let Ok(result) = self.decode_rx.try_recv() {
            self.handle_decode_result(ctx, result);
        }
        self.request_pending_image_level(ctx);

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
                if let Some(img) = self.current.as_mut() {
                    let base_image_size =
                        egui::vec2(img.native_size[0] as f32, img.native_size[1] as f32);

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
                    // Mip selection must use physical pixels, not egui points,
                    // or high-DPI displays get a mip that is too small (blurry).
                    let physical_scale = effective_scale * ctx.pixels_per_point();
                    let desired = pick_mip_level(physical_scale, img.textures.len());

                    // Ask the background worker for the missing level; until it
                    // arrives the closest already-decoded level is shown.
                    if img.textures[desired].is_none() && !img.requested[desired] {
                        img.requested[desired] = true;
                        let _ = self.decode_tx.send(DecodeRequest {
                            generation: img.generation,
                            level: desired,
                            target: level_size(img.native_size, desired),
                            bytes: img.bytes.clone(),
                        });
                    }

                    // Once the right level is on screen, evict textures that are
                    // 2+ levels sharper than needed — they are the memory hogs
                    // and can be re-decoded from `img.bytes` when zooming in.
                    if img.textures[desired].is_some() {
                        for i in 0..desired.saturating_sub(1) {
                            if img.textures[i].is_some() {
                                img.textures[i] = None;
                                img.requested[i] = false;
                            }
                        }
                    }

                    if let Some(level) = best_available_level(&img.textures, desired) {
                        let texture = img.textures[level].as_ref().unwrap();

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
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
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
    fn refresh_current_dir_cache(&mut self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| "Current image has no parent directory".to_string())?;

        let mut files: Vec<DirImage> = std::fs::read_dir(parent)
            .map_err(|e| e.to_string())?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                // file_type/metadata come from the directory enumeration on
                // Windows, so neither costs an extra stat per file.
                if !entry.file_type().ok()?.is_file() {
                    return None;
                }
                let path = entry.path();
                if !is_supported_image(&path) {
                    return None;
                }
                let name_w: Vec<u16> = path
                    .file_name()?
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let metadata = entry.metadata().ok();
                Some(DirImage {
                    path,
                    name_w,
                    metadata,
                })
            })
            .collect();

        let sort = explorer::query_sort_for_folder(parent).unwrap_or(explorer::DEFAULT_SORT);
        files.sort_by(|a, b| compare_dir_images(a, b, sort));

        let files: Vec<PathBuf> = files.into_iter().map(|f| f.path).collect();
        self.current_dir_index = files.iter().position(|p| p == path);
        self.current_dir_parent = Some(parent.to_path_buf());
        self.current_dir_images = files;
        Ok(())
    }

    fn wic_context(&mut self) -> Result<&wic::WicContext, String> {
        if self.wic.is_none() {
            self.wic = Some(wic::WicContext::new()?);
        }
        self.wic
            .as_ref()
            .ok_or_else(|| "Failed to initialize image decoder".to_string())
    }

    /// Probes the image size (header only, fast) and schedules the actual
    /// pixel decoding on the background worker. The previous image stays on
    /// screen until the first texture level of this one is ready.
    fn load_image(&mut self, bytes: Arc<[u8]>, filename: Option<String>) -> Result<(), String> {
        let (width, height) = self.wic_context()?.image_size(&bytes)?;
        let level_count = mip_level_count(width, height);
        self.generation_counter += 1;
        self.pending_image = Some(LoadedImage {
            generation: self.generation_counter,
            bytes,
            filename,
            native_size: [width, height],
            textures: (0..level_count).map(|_| None).collect(),
            requested: vec![false; level_count],
        });
        self.last_error = None;
        Ok(())
    }

    fn load_image_from_path_impl(
        &mut self,
        path: &Path,
        refresh_cache: bool,
    ) -> Result<(), String> {
        let filename = path.file_name().map(|n| n.to_string_lossy().to_string());
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        self.load_image(Arc::from(bytes), filename)?;
        self.current_path = Some(path.to_path_buf());
        if refresh_cache {
            self.refresh_current_dir_cache(path)?;
        }
        Ok(())
    }

    fn handle_decode_result(&mut self, ctx: &egui::Context, result: DecodeResult) {
        if let Some(pending) = self.pending_image.as_mut()
            && pending.generation == result.generation
        {
            match result.outcome {
                Ok(decoded) => {
                    let texture = upload_texture(ctx, result.generation, result.level, decoded);
                    pending.textures[result.level] = Some(texture);
                    self.current = self.pending_image.take();
                    self.reset_view();
                }
                Err(err) => {
                    let filename = pending.filename.clone();
                    self.last_error = Some(format_load_error(None, filename.as_deref(), &err));
                    self.pending_image = None;
                }
            }
            ctx.request_repaint();
            return;
        }

        if let Some(img) = self.current.as_mut()
            && img.generation == result.generation
        {
            match result.outcome {
                Ok(decoded) => {
                    let texture = upload_texture(ctx, result.generation, result.level, decoded);
                    img.textures[result.level] = Some(texture);
                    ctx.request_repaint();
                }
                Err(err) => {
                    let filename = img.filename.clone();
                    self.last_error = Some(format_load_error(None, filename.as_deref(), &err));
                }
            }
        }
        // Results for images that were replaced in the meantime are dropped.
    }

    /// Kicks off decoding of the level a pending image will need once shown
    /// (it always starts at zoom 1, fit to window).
    fn request_pending_image_level(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_image.as_mut() else {
            return;
        };
        let avail = ctx.screen_rect().size() * ctx.pixels_per_point();
        let fit_scale = (avail.x / pending.native_size[0] as f32)
            .min(avail.y / pending.native_size[1] as f32)
            .min(1.0);
        let desired = pick_mip_level(fit_scale, pending.textures.len());
        if pending.textures[desired].is_none() && !pending.requested[desired] {
            pending.requested[desired] = true;
            let _ = self.decode_tx.send(DecodeRequest {
                generation: pending.generation,
                level: desired,
                target: level_size(pending.native_size, desired),
                bytes: pending.bytes.clone(),
            });
        }
    }

    fn reset_view(&mut self) {
        self.zoom = 1.0;
        self.pan_offset = egui::Vec2::ZERO;
        self.is_dragging = false;
        self.last_mouse_pos = None;
        self.last_window_size = None;
        self.is_animating_to_center = false;
        self.animation_start_time = None;
    }

    fn load_image_from_path(&mut self, path: &Path) -> Result<(), String> {
        self.load_image_from_path_impl(path, true)?;
        Ok(())
    }

    fn load_adjacent_image(&mut self, direction: isize) -> Result<(), String> {
        let Some(current_path) = self.current_path.as_ref().cloned() else {
            return Ok(());
        };
        if direction == 0 {
            return Ok(());
        }

        let expected_parent = current_path.parent().map(Path::to_path_buf);
        if self.current_dir_images.is_empty()
            || self.current_dir_parent != expected_parent
            || self.current_dir_index.is_none()
        {
            self.refresh_current_dir_cache(&current_path)?;
        }

        let Some(current_idx) = self.current_dir_index else {
            return Ok(());
        };

        let next_idx = if direction > 0 {
            current_idx + 1
        } else if current_idx == 0 {
            return Ok(());
        } else {
            current_idx - 1
        };

        let Some(next_path) = self.current_dir_images.get(next_idx).cloned() else {
            return Ok(());
        };
        self.current_dir_index = Some(next_idx);

        self.load_image_from_path_impl(&next_path, false)
            .map_err(|e| {
                let filename = next_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string());
                format_load_error(Some(&next_path), filename.as_deref(), &e)
            })
    }
}

struct DirImage {
    path: PathBuf,
    /// Null-terminated UTF-16 file name for StrCmpLogicalW.
    name_w: Vec<u16>,
    metadata: Option<std::fs::Metadata>,
}

fn compare_dir_images(a: &DirImage, b: &DirImage, sort: explorer::SortSpec) -> Ordering {
    let ordering = match sort.key {
        explorer::SortKey::Name => natural_name_cmp(&a.name_w, &b.name_w),
        explorer::SortKey::DateModified => file_time_cmp(a, b, |m| m.modified().ok())
            .then_with(|| natural_name_cmp(&a.name_w, &b.name_w)),
        explorer::SortKey::DateCreated => file_time_cmp(a, b, |m| m.created().ok())
            .then_with(|| natural_name_cmp(&a.name_w, &b.name_w)),
        explorer::SortKey::Size => {
            let size = |d: &DirImage| d.metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            size(a)
                .cmp(&size(b))
                .then_with(|| natural_name_cmp(&a.name_w, &b.name_w))
        }
    };
    if sort.ascending {
        ordering
    } else {
        ordering.reverse()
    }
}

fn file_time_cmp(
    a: &DirImage,
    b: &DirImage,
    time: impl Fn(&std::fs::Metadata) -> Option<std::time::SystemTime>,
) -> Ordering {
    let a_time = a.metadata.as_ref().and_then(&time);
    let b_time = b.metadata.as_ref().and_then(&time);
    a_time.cmp(&b_time)
}

/// Explorer-style natural sort ("img2" before "img10").
fn natural_name_cmp(a: &[u16], b: &[u16]) -> Ordering {
    let r = unsafe { StrCmpLogicalW(PCWSTR(a.as_ptr()), PCWSTR(b.as_ptr())) };
    r.cmp(&0)
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

/// An opened image: the compressed source bytes plus lazily decoded texture
/// levels. Level k is the image at native size / 2^k; levels are decoded by
/// the background worker only when the view needs them.
struct LoadedImage {
    generation: u64,
    bytes: Arc<[u8]>,
    filename: Option<String>,
    native_size: [u32; 2],
    textures: Vec<Option<egui::TextureHandle>>,
    requested: Vec<bool>,
}

struct DecodeRequest {
    generation: u64,
    level: usize,
    target: [u32; 2],
    bytes: Arc<[u8]>,
}

struct DecodeResult {
    generation: u64,
    level: usize,
    outcome: Result<wic::DecodedImage, String>,
}

fn spawn_decode_worker(
    ctx: egui::Context,
) -> (mpsc::Sender<DecodeRequest>, mpsc::Receiver<DecodeResult>) {
    let (request_tx, request_rx) = mpsc::channel::<DecodeRequest>();
    let (result_tx, result_rx) = mpsc::channel::<DecodeResult>();
    std::thread::spawn(move || {
        // The WIC factory must be created and used on this thread only.
        let wic = wic::WicContext::new();
        while let Ok(request) = request_rx.recv() {
            let outcome = match &wic {
                Ok(w) => w.decode_scaled(&request.bytes, request.target[0], request.target[1]),
                Err(e) => Err(e.clone()),
            };
            let send_failed = result_tx
                .send(DecodeResult {
                    generation: request.generation,
                    level: request.level,
                    outcome,
                })
                .is_err();
            if send_failed {
                break;
            }
            // Wake the UI thread so it picks up the result.
            ctx.request_repaint();
        }
    });
    (request_tx, result_rx)
}

const TEXTURE_OPTIONS: egui::TextureOptions = egui::TextureOptions {
    magnification: egui::TextureFilter::Linear,
    minification: egui::TextureFilter::Linear,
    wrap_mode: egui::TextureWrapMode::ClampToEdge,
    mipmap_mode: None,
};

fn upload_texture(
    ctx: &egui::Context,
    generation: u64,
    level: usize,
    decoded: wic::DecodedImage,
) -> egui::TextureHandle {
    let image = egui::ColorImage {
        size: [decoded.width as usize, decoded.height as usize],
        pixels: decoded.pixels,
        source_size: egui::vec2(decoded.width as f32, decoded.height as f32),
    };
    ctx.load_texture(format!("img_{generation}_mip_{level}"), image, TEXTURE_OPTIONS)
}

fn mip_level_count(width: u32, height: u32) -> usize {
    let max_dim = width.max(height).max(1);
    (32 - max_dim.leading_zeros()) as usize
}

fn level_size(native: [u32; 2], level: usize) -> [u32; 2] {
    [(native[0] >> level).max(1), (native[1] >> level).max(1)]
}

fn best_available_level(
    textures: &[Option<egui::TextureHandle>],
    desired: usize,
) -> Option<usize> {
    if textures.get(desired)?.is_some() {
        return Some(desired);
    }
    // Prefer the nearest sharper level (downscaling it looks fine) over an
    // upscaled blurry one.
    (0..desired)
        .rev()
        .find(|&i| textures[i].is_some())
        .or_else(|| ((desired + 1)..textures.len()).find(|&i| textures[i].is_some()))
}
