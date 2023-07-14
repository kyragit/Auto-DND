use std::collections::HashMap;

use serde::{Serialize, Deserialize};
use simple_enum_macro::simple_enum;


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Spell {
    pub magic_type: MagicType,
    pub name: String,
    pub description: String,
    pub spell_level: u8,
    pub reversed: Option<String>,
    pub range: SpellRange,
    pub duration: SpellDuration,
}

impl Spell {
    pub fn default() -> Self {
        Self {
            magic_type: MagicType::Arcane,
            name: String::new(),
            description: String::new(),
            spell_level: 0,
            reversed: None,
            range: SpellRange::OnSelf,
            duration: SpellDuration::Instant,
        }
    }
}

#[simple_enum(display)]
pub enum MagicType {
    /// Arcane
    Arcane,
    /// Divine
    Divine,
}

#[simple_enum(display)]
pub enum SpellRange {
    /// self
    OnSelf,
    /// touch
    Touch,
    /// {0}'
    Feet(u32),
    /// {0}' radius
    RadiusFeet(u32),
    /// unlimited
    Unlimited,
    /// special
    Special,
}

impl SpellRange {
    pub fn display(&self) -> String {
        match self {
            Self::OnSelf => "Self",
            Self::Touch => "Touch",
            Self::Feet(_) => "Feet",
            Self::RadiusFeet(_) => "Feet Radius",
            Self::Unlimited => "Unlimited",
            Self::Special => "Special",
        }.to_owned()
    }
}

#[simple_enum(display)]
pub enum SpellDuration {
    /// instantaneous
    Instant,
    /// {0} rounds
    Rounds(u32),
    /// {0} rounds per level
    RoundsPerLevel(u32),
    /// {0} turns
    Turns(u32),
    /// {0} turns per level
    TurnsPerLevel(u32),
    /// {0} days
    Days(u32),
    /// {0} days per level
    DaysPerLevel(u32),
    /// concentration
    Concentration,
    /// permanent
    Permanent,
    /// special
    Special,
}

impl SpellDuration {
    pub fn display(&self) -> String {
        match self {
            SpellDuration::Instant => "Instantaneous",
            SpellDuration::Rounds(_) => "Rounds",
            SpellDuration::RoundsPerLevel(_) => "Rounds per Level",
            SpellDuration::Turns(_) => "Turns",
            SpellDuration::TurnsPerLevel(_) => "Turns per Level",
            SpellDuration::Days(_) => "Days",
            SpellDuration::DaysPerLevel(_) => "Days per Level",
            SpellDuration::Concentration => "Concentration",
            SpellDuration::Permanent => "Permanent",
            SpellDuration::Special => "Special",
        }.to_owned()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellRegistry {
    pub divine: [HashMap<String, Spell>; 7],
    pub arcane: [HashMap<String, Spell>; 9],
}

impl SpellRegistry {
    pub fn new() -> Self {
        Self {
            divine: array_macro::array![HashMap::new(); 7],
            arcane: array_macro::array![HashMap::new(); 9],
        }
    }
    pub fn random_arcane(&self, level: u8) -> Option<String> {
        if level < 9 {
            if self.arcane[level as usize].is_empty() {
                None
            } else {
                let spells: Vec<&String> = self.arcane[level as usize].keys().collect();
                Some(spells[rand::random::<usize>() % spells.len()].clone())
            }
        } else {
            None
        }
    }
    pub fn get_spell_name_or<'a>(&'a self, id: impl Into<String>, lvl: u8, magic_type: MagicType, default: &'a str) -> &'a str {
        match magic_type {
            MagicType::Divine => self.divine.get(lvl as usize),
            MagicType::Arcane => self.arcane.get(lvl as usize),
        }.and_then(|map| map.get(&id.into()).map(|s| s.name.as_str())).unwrap_or(default)
    }
    pub fn get_spell_name_or_default<'a>(&'a self, id: impl Into<String>, lvl: u8, magic_type: MagicType) -> &'a str {
        self.get_spell_name_or(id, lvl, magic_type, "Nonexistent Spell")
    }
}