// ~/backend/sentinelc/src/IPC/windows/mod.rs
pub mod taskbar;
pub mod wallpaper;
pub mod theme;
pub mod transparency;

pub struct WindowsCManager {
    pub _taskbar: taskbar::Taskbar,
    pub _wallpaper: wallpaper::Wallpaper,
    pub _theme: theme::Theme,
    pub _transparency: transparency::Transparency,
}

impl WindowsCManager {
    pub fn new() -> Self {
        Self {
            _taskbar: taskbar::Taskbar::new(),
            _wallpaper: wallpaper::Wallpaper::new(),
            _theme: theme::Theme::new(),
            _transparency: transparency::Transparency::new(),
        }
    }
}