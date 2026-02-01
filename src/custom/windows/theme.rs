use crate::{info};

pub struct Theme;

impl Theme {
    pub fn new() -> Self {
        info!("[Theme] Theme manager initialized");
        Self
    }
}