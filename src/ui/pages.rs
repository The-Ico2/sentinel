use std::path::PathBuf;

/// Directory where page HTML files are stored.
fn pages_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pages")
}

pub fn base_page() -> PathBuf {
    pages_dir().join("base.html")
}

#[allow(dead_code)]
pub fn home_page() -> PathBuf {
    pages_dir().join("home.html")
}

#[allow(dead_code)]
pub fn addons_page() -> PathBuf {
    pages_dir().join("addons.html")
}

#[allow(dead_code)]
pub fn data_page() -> PathBuf {
    pages_dir().join("data.html")
}

#[allow(dead_code)]
pub fn settings_page() -> PathBuf {
    pages_dir().join("settings.html")
}

#[allow(dead_code)]
pub fn store_page() -> PathBuf {
    pages_dir().join("store.html")
}
