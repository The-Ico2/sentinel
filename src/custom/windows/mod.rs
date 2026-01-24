// ~/backend/sentinelc/src/IPC/windows/mod.rs
pub mod taskbar;
pub mod wallpaper;
pub mod theme;
pub mod transparency;
pub mod registry;

pub struct WindowsCManager {
    pub taskbar: taskbar::Taskbar,
    pub wallpaper: wallpaper::Wallpaper,
    pub theme: theme::Theme,
    pub transparency: transparency::Transparency,
}

impl WindowsCManager {
    pub fn new() -> Self {
        Self {
            taskbar: taskbar::Taskbar::new(),
            wallpaper: wallpaper::Wallpaper::new(),
            theme: theme::Theme::new(),
            transparency: transparency::Transparency::new(),
        }
    }
}