#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self};
use argh::FromArgs;
use std::path::PathBuf;

fn main() -> eframe::Result {
    let cli: Cli = argh::from_env();
    let mut options = eframe::NativeOptions::default();
    let initial_path = cli.file.map(|s| PathBuf::from(s));

    options.persist_window = true;

    eframe::run_native(
        "imeji",
        options,
        Box::new(move |_| {
            let mut app = Imeji::default();
            if let Some(p) = initial_path {
                if let Ok(bytes) = std::fs::read(p) {
                    app.load_image(&bytes);
                }
            }
            Ok(Box::new(app))
        })
    )
}

#[derive(Default)]
struct Imeji {
    image: Option<egui::ColorImage>,
    texture: Option<egui::TextureHandle>,
    //path: Option<PathBuf>,
}

#[derive(FromArgs)]
/// load img from file explorer
struct Cli {
    #[argh(positional)]
    file: Option<String>,
}

impl eframe::App for Imeji {
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}
    
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
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(image) = &self.image {
                let texture = self.texture.get_or_insert_with(|| {
                    ctx.load_texture("loaded_image", image.clone(), egui::TextureOptions::default())
                });
                let image_size = texture.size_vec2();
                let available_size = ui.available_size();

                let scale = (available_size.x / image_size.x)
                    .min(available_size.y / image_size.y)
                    .min(1.0);

                let display_size = image_size * scale;

                ui.centered_and_justified(|ui| {
                    ui.image(egui::load::SizedTexture::new(texture, display_size));
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Drop img here buds");
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
                self.image = Some(egui::ColorImage::from_rgba_unmultiplied(size, &rgba_image));
                self.texture = None;
            }
            Err(e) => println!("ImageError : {e}"),
        }
    }
}