fn main() {
    // Embed a Windows icon into the final .exe when building on Windows
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/app.ico"); // multi-size .ico recommended
        res.set("ProductName", "GlpiNotifier");
        res.set("FileDescription", "GLPI notifier for Windows");
        res.set("CompanyName", "Cardan");
        res.set("OriginalFilename", "glpi-notifier-rs.exe");
        res.compile().expect("Failed to embed icon");
    }
}
