use crate::character::Attr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Class {
    pub name: String,
    pub description: String,
    pub prime_reqs: Vec<Attr>,
    pub hit_die: HitDie,
    pub base_xp_cost: u32,
    pub saving_throw_progression_type: SavingThrowProgressionType,
}

impl Class {
    pub fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            prime_reqs: vec![],
            hit_die: HitDie::D4,
            base_xp_cost: 2000,
            saving_throw_progression_type: SavingThrowProgressionType::Fighter,
        }
    }
    pub fn calculate_next_level_cost(&self, current_level: u8) -> u32 {
        match current_level {
            0 => 100,
            1..=5 => self.base_xp_cost * (2u32.pow(current_level as u32 - 1)),
            6 => {
                let mut unrounded = self.base_xp_cost * (2u32.pow(5));
                let modulo = unrounded % 5000;
                if modulo <= 2500 {
                    unrounded -= modulo
                } else {
                    unrounded += 5000 - modulo;
                }
                unrounded
            },
            7 => self.calculate_next_level_cost(6) * 2,
            8.. => self.calculate_next_level_cost(7) + (self.saving_throw_progression_type.get_max_xp_cost() * (current_level as u32 - 7)),
        }
    }
    pub fn from_file(path: String) -> Option<Self> {
        if let Ok(file) = std::fs::read_to_string(format!("classes/{}", path)) {
            if let Ok(class) = ron::from_str::<Self>(&file) {
                return Some(class);
            }
        }
        None
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum HitDie {
    D4,
    D6,
    D8,
    D10,
    D12,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SavingThrowProgressionType {
    Fighter,
    Thief,
    Cleric,
    Mage,
}

impl std::fmt::Display for SavingThrowProgressionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Fighter => "Fighter",
            Self::Thief => "Thief",
            Self::Cleric => "Cleric",
            Self::Mage => "Mage",
        })
    }
}

impl SavingThrowProgressionType {
    pub fn get_max_xp_cost(&self) -> u32 {
        match self {
            Self::Fighter => 120000,
            Self::Cleric | Self::Thief => 100000,
            Self::Mage => 150000,
        }
    }
}