use std::collections::HashSet;

use egui::Color32;
use serde::{Serialize, Deserialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Party {
    pub temporary_xp: u32,
    pub members: HashSet<(String, String)>,
    pub color: Color32,
}

impl Party {
    pub fn new() -> Self {
        Self {
            temporary_xp: 0,
            members: HashSet::new(),
            color: Color32::WHITE,
        }
    }
}