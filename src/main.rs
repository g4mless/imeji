#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use argh::FromArgs;
use eframe::egui;
use image::GenericImageView;
use std::path::PathBuf;

fn main() -> eframe::Result {
    let cli: Cli = argh::from_env();
    let mut options = eframe::NativeOptions::default();
    let initial_path = cli.file.map(|s| PathBuf::from(s));

    options.persist_window = true;

    let mut viewport_builder = egui::ViewportBuilder::default()
        .with_min_inner_size([480.0, 480.0]);

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
                if let Ok(bytes) = std::fs::read(p) {
                    app.load_image(&bytes);
                }
            }
            Ok(Box::new(app))
        }),
    )
}

fn load_icon() -> Result<egui::IconData, Box<dyn std::error::Error>> {
    let image = image::load_from_memory(include_bytes!("../icon.ico"))?;
    let (width, height) = image.dimensions(); // Get dimensions before conversion
    let image_buffer = image.to_rgba8();
    Ok(egui::IconData {
        rgba: image_buffer.into_raw(),
        width,
        height,
    })
}

#[derive(Default)]
struct Imeji {
    image: Option<egui::ColorImage>,
    texture: Option<egui::TextureHandle>,
    zoom: f32,
    pan_offset: egui::Vec2,
    is_dragging: bool,
    last_mouse_pos: Option<egui::Pos2>,
    last_window_size: Option<egui::Vec2>,
}

impl Imeji {
    fn new() -> Self {
        Self {
            image: None,
            texture: None,
            zoom: 1.0,
            pan_offset: egui::Vec2::ZERO,
            is_dragging: false,
            last_mouse_pos: None,
            last_window_size: None,
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
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                if let Some(bytes) = &i.raw.dropped_files[0].bytes {
                    self.load_image(bytes);
                } else if let Some(path) = &i.raw.dropped_files[0].path {
                    if let Ok(bytes) = std::fs::read(path) {
                        self.load_image(&bytes);
                    }
                }
            }
        });

        // Ctrl+W = Close Image
        ctx.input_mut(|i| {
            if i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::W,
            )) {
                self.image = None;
                self.texture = None;
                self.zoom = 1.0;
                self.pan_offset = egui::Vec2::ZERO;
            }
        });

        let current_window_size = ctx.screen_rect().size();
        if let Some(last_size) = self.last_window_size {
            let size_increase = current_window_size - last_size;
            if size_increase.x > 100.0 || size_increase.y > 100.0 {
                self.zoom = 1.0;
                self.pan_offset = egui::Vec2::ZERO;
            }
        }
        self.last_window_size = Some(current_window_size);

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(image) = &self.image {
                let texture = self.texture.get_or_insert_with(|| {
                    let texture_options = egui::TextureOptions {
                        magnification: egui::TextureFilter::Linear,
                        minification: egui::TextureFilter::Linear,
                        wrap_mode: egui::TextureWrapMode::ClampToEdge,
                        mipmap_mode: Some(egui::TextureFilter::Linear),
                    };
                    ctx.load_texture("loaded_image", image.clone(), texture_options)
                });

                let image_size = texture.size_vec2();
                let available_size = ui.available_size();

                let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll_delta != 0.0 {
                    let zoom_factor = 1.0 + scroll_delta * 0.001;
                    let old_zoom = self.zoom;
                    self.zoom = (self.zoom * zoom_factor).clamp(1.0, 10.0);
                    
                    // Reset pan offset to center when zoom is 1.0
                    if self.zoom == 1.0 {
                        self.pan_offset = egui::Vec2::ZERO;
                    } else if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
                        let center = ui.available_rect_before_wrap().center();
                        let mouse_offset = mouse_pos - center;
                        let zoom_change = self.zoom / old_zoom - 1.0;
                        self.pan_offset -= mouse_offset * zoom_change;
                    }
                }

                let mouse_pos = ui.input(|i| i.pointer.hover_pos());
                let is_primary_down = ui.input(|i| i.pointer.primary_down());

                if is_primary_down && mouse_pos.is_some() {
                    let current_pos = mouse_pos.unwrap();
                    
                    if !self.is_dragging {
                        self.is_dragging = true;
                        self.last_mouse_pos = Some(current_pos);
                    } else if let Some(last_pos) = self.last_mouse_pos {
                        let delta = current_pos - last_pos;
                        // Only allow panning when zoom is greater than 1.0
                        if self.zoom > 1.0 {
                            self.pan_offset += delta;
                        }
                        self.last_mouse_pos = Some(current_pos);
                    }
                } else {
                    self.is_dragging = false;
                    self.last_mouse_pos = None;
                }

                let base_scale = (available_size.x / image_size.x)
                    .min(available_size.y / image_size.y)
                    .min(1.0);
                
                let display_size = image_size * base_scale * self.zoom;
                let center = ui.available_rect_before_wrap().center();
                let image_pos = center - display_size * 0.5 + self.pan_offset;

                let image_rect = egui::Rect::from_min_size(image_pos, display_size);
                let _response = ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::click_and_drag());
                
                if ui.is_rect_visible(image_rect) {
                    ui.painter().image(
                        texture.id(),
                        image_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }

            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Drop img here");
                });
            }
        });
    }
}

impl Imeji {
    fn load_image(&mut self, bytes: &[u8]) {
        match image::load_from_memory(bytes) {
            Ok(dynamic_image) => {
                let rgba_image = dynamic_image.to_rgba8();
                let size = [rgba_image.width() as usize, rgba_image.height() as usize];
                self.image = Some(egui::ColorImage::from_rgba_unmultiplied(
                    size,
                    &rgba_image,
                ));
                self.texture = None;
                // Reset zoom and pan when loading new image
                self.zoom = 1.0;
                self.pan_offset = egui::Vec2::ZERO;
                self.is_dragging = false;
                self.last_mouse_pos = None;
                self.last_window_size = None;
            }
            Err(e) => println!("ImageError : {e}"),
        }
    }
}