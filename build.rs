fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let icon_path = if std::path::Path::new("icon.ico").exists() {
            "icon.ico"
        } else if std::path::Path::new("pages/icons/icon.ico").exists() {
            "pages/icons/icon.ico"
        } else {
            return;
        };
        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon_path);
        res.compile().expect("Failed to compile Windows resources");
    }
}
