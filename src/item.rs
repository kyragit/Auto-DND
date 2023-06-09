use std::{collections::HashSet, path::Path};

use displaydoc::Display;
use serde::{Serialize, Deserialize};
use simple_enum_macro::simple_enum;

use crate::combat::DamageRoll;


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ItemType {
    pub name: String,
    pub description: String,
    pub value: Value,
    pub encumbrance: Encumbrance,
    pub tags: HashSet<String>,
    pub weapon_stats: Option<WeaponStats>,
    pub armor_stats: Option<ArmorValue>,
    pub shield_stats: Option<ArmorValue>,
    pub container_stats: Option<ContainerStats>,
}

impl ItemType {
    pub fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            value: Value(0.0),
            encumbrance: Encumbrance::OneSixth,
            tags: HashSet::new(),
            weapon_stats: None,
            armor_stats: None,
            shield_stats: None,
            container_stats: None,
        }
    }
    pub fn save(&self, file: &str) -> Result<(), ()> {
        if let Ok(s) = ron::to_string(self) {
            let file = format!("items/{}.ron", file);
            let path = Path::new(&file);
            if let Some(parent) = path.parent() {
                if std::fs::create_dir_all(parent).is_err() {
                    return Err(());
                }
            }
            if let Ok(_) = std::fs::write(path, s.as_bytes()) {
                return Ok(());
            }
        }
        Err(())
    }
    pub fn platinum() -> Self {
        Self {
            name: "Platinum Piece".to_owned(),
            description: "A platinum coin.".to_owned(),
            value: Value::from_platinum(1.0),
            encumbrance: Encumbrance::Treasure,
            tags: HashSet::from(["pp".to_owned()]),
            weapon_stats: None,
            armor_stats: None,
            shield_stats: None,
            container_stats: None,
        }
    }
    pub fn gold() -> Self {
        Self {
            name: "Gold Piece".to_owned(),
            description: "A gold coin.".to_owned(),
            value: Value::from_gold(1.0),
            encumbrance: Encumbrance::Treasure,
            tags: HashSet::from(["gp".to_owned()]),
            weapon_stats: None,
            armor_stats: None,
            shield_stats: None,
            container_stats: None,
        }
    }
    pub fn electrum() -> Self {
        Self {
            name: "Electrum Piece".to_owned(),
            description: "An electrum coin.".to_owned(),
            value: Value::from_electrum(1.0),
            encumbrance: Encumbrance::Treasure,
            tags: HashSet::from(["ep".to_owned()]),
            weapon_stats: None,
            armor_stats: None,
            shield_stats: None,
            container_stats: None,
        }
    }
    pub fn silver() -> Self {
        Self {
            name: "Silver Piece".to_owned(),
            description: "A silver coin.".to_owned(),
            value: Value::from_silver(1.0),
            encumbrance: Encumbrance::Treasure,
            tags: HashSet::from(["sp".to_owned()]),
            weapon_stats: None,
            armor_stats: None,
            shield_stats: None,
            container_stats: None,
        }
    }
    pub fn copper() -> Self {
        Self {
            name: "Copper Piece".to_owned(),
            description: "A copper coin.".to_owned(),
            value: Value::from_copper(1.0),
            encumbrance: Encumbrance::Treasure,
            tags: HashSet::from(["cp".to_owned()]),
            weapon_stats: None,
            armor_stats: None,
            shield_stats: None,
            container_stats: None,
        }
    }
}

#[simple_enum(display)]
pub enum Encumbrance {
    /// Negligible
    Negligible,
    /// Treasure (1/1,000 of a stone)
    Treasure,
    /// 1/6 of a stone
    OneSixth,
    /// 1/2 of a stone
    OneHalf,
    /// One stone
    OneStone,
    /// Very Heavy
    VeryHeavy(u32),
}

impl Encumbrance {
    pub fn as_float(&self) -> f64 {
        match self {
            Encumbrance::Negligible => 0.0,
            Encumbrance::Treasure => 0.001,
            Encumbrance::OneSixth => 1.0 / 6.0,
            Encumbrance::OneHalf => 0.5,
            Encumbrance::OneStone => 1.0,
            Encumbrance::VeryHeavy(n) => *n as f64,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Self::Negligible => "0 Stone".to_owned(),
            Self::Treasure => "1 stone per 1,000".to_owned(),
            Self::OneSixth => "1/6 Stone".to_owned(),
            Self::OneHalf => "1/2 Stone".to_owned(),
            Self::OneStone => "1 Stone".to_owned(),
            Self::VeryHeavy(s) => format!("{} Stone", s),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Value(pub f64);

impl Value {
    pub fn as_copper(&self) -> f64 {
        self.0 * 10.0
    }
    pub fn as_silver(&self) -> f64 {
        self.0
    }
    pub fn as_electrum(&self) -> f64 {
        self.0 / 5.0
    }
    pub fn as_gold(&self) -> f64 {
        self.0 / 10.0
    }
    pub fn as_platinum(&self) -> f64 {
        self.0 / 50.0
    }
    pub fn from_copper(amount: f64) -> Self {
        Self(amount / 10.0)
    }
    pub fn from_silver(amount: f64) -> Self {
        Self(amount)
    }
    pub fn from_electrum(amount: f64) -> Self {
        Self(amount * 5.0)
    }
    pub fn from_gold(amount: f64) -> Self {
        Self(amount * 10.0)
    }
    pub fn from_platinum(amount: f64) -> Self {
        Self(amount * 50.0)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WeaponStats {
    pub damage: WeaponDamage,
}

impl WeaponStats {
    pub fn default() -> Self {
        Self {
            damage: WeaponDamage::Melee(MeleeDamage::OneHanded(DamageRoll::default())),
        }
    }
}

pub type AmmoType = String;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum WeaponDamage {
    Melee(MeleeDamage),
    Missile(DamageRoll, AmmoType),
}

impl WeaponDamage {
    pub fn display(&self) -> String {
        match self {
            Self::Melee(_) => "Melee".to_owned(),
            Self::Missile(_, _) => "Missile".to_owned(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum MeleeDamage {
    OneHanded(DamageRoll),
    Versatile(DamageRoll, DamageRoll),
    TwoHanded(DamageRoll),
}

impl MeleeDamage {
    pub fn display(&self) -> String {
        match self {
            Self::OneHanded(_) => "One-Handed".to_owned(),
            Self::Versatile(_, _) => "Versatile".to_owned(),
            Self::TwoHanded(_) => "Two-Handed".to_owned(),
        }
    }
}

pub type ArmorValue = u32;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Display, PartialEq)]
pub enum ContainerStats {
    /// Items
    Items(u32),
    /// Stone
    Stone(u32),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Item {
    pub item_type: ItemType,
    pub count: u32,
}

impl Item {
    pub fn from_type(item_type: ItemType, count: u32) -> Self {
        Self { 
            item_type, 
            count, 
        }
    }
}