use crate::{info};

pub struct Transparency;

impl Transparency {
    pub fn new() -> Self {
        info!("[Transparency] Transparency manager initialized");
        Self
    }
}