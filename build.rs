// build.rs
fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        // Menunjuk ke file resource script yang akan kita buat
        res.set_resource_file("app.rc");
        // Mengkompilasi resource ke dalam aplikasi
        if let Err(e) = res.compile() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}
