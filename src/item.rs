use std::collections::HashSet;

use serde::{Serialize, Deserialize};

use crate::combat::Damage;


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

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encumbrance {
    Negligible,
    Treasure,
    OneSixth,
    OneHalf,
    OneStone,
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
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Value(f64);

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

pub type AmmoType = String;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum WeaponDamage {
    Melee(MeleeDamage),
    Missile(Damage, AmmoType),
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum MeleeDamage {
    OneHanded(Damage),
    Versatile(Damage, Damage),
    TwoHanded(Damage),
}

pub type ArmorValue = u32;

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ContainerStats {
    Items(u32),
    Stone(u32),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Item {
    pub item_type: ItemType,
    pub count: u32,
}