use std::{collections::HashSet, path::Path};

use serde::{Serialize, Deserialize};
use simple_enum_macro::simple_enum;

use crate::{combat::{CombatantStats, DamageRoll}, dice::{DiceRoll, self, ModifierType, Drop}, character::{SavingThrows, Attributes}};


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Enemy {
    pub combat_stats: CombatantStats,
}

impl Enemy {
    pub fn from_type(typ: &EnemyType) -> Self {
        let mut s = Self {
            combat_stats: CombatantStats::empty(),
        };
        s.combat_stats.attributes = Attributes::random();
        let hp = match typ.hit_dice {
            EnemyHitDice::Standard(amount) => {
                dice::roll(DiceRoll::new(amount, 8, 0, ModifierType::Add, false, Drop::None, 1))
            },
            EnemyHitDice::WithModifier(amount, modifier) => {
                dice::roll(DiceRoll::new(amount, 8, modifier, ModifierType::Add, false, Drop::None, 1))
            },
            EnemyHitDice::Special(mut roll) => {
                roll.min_value = 1;
                dice::roll(roll)
            },
        };
        s.combat_stats.health.max_hp = hp as u32;
        s.combat_stats.health.current_hp = hp;
        s.combat_stats.armor_class = typ.base_armor_class;
        s.combat_stats.attack_throw = typ.base_attack_throw;
        s.combat_stats.saving_throws = typ.saves;
        s.combat_stats.damage = typ.base_damage;
        s
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EnemyType {
    pub name: String,
    pub description: String,
    pub hit_dice: EnemyHitDice,
    pub base_armor_class: i32,
    pub base_attack_throw: i32,
    pub base_damage: AttackRoutine,
    pub xp: u32,
    pub morale: i32,
    pub categories: HashSet<EnemyCategory>,
    pub alignment: Alignment,
    pub saves: SavingThrows,
}

impl EnemyType {
    pub fn save(&self, file: &str) -> Result<(), ()> {
        if let Ok(s) = ron::to_string(self) {
            let file = format!("enemies/{}.ron", file);
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

    pub fn default() -> Self {
        Self {
            name: "".to_owned(),
            description: "".to_owned(),
            hit_dice: EnemyHitDice::Standard(1),
            base_armor_class: 0,
            base_attack_throw: 10,
            base_damage: AttackRoutine::One(DamageRoll::default()),
            xp: 0,
            morale: 0,
            categories: HashSet::new(),
            alignment: Alignment::Neutral,
            saves: SavingThrows::new(),
        }
    }
}

#[simple_enum(display)]
pub enum EnemyCategory {
    /// Animal
    Animal,
    /// Beastman
    Beastman,
    /// Construct
    Construct,
    /// Enchanted Creature
    Enchanted,
    /// Fantastic Creature
    Fantastic,
    /// Giant Humanoid
    GiantHumanoid,
    /// Humanoid
    Humanoid,
    /// Ooze
    Ooze,
    /// Summoned Creature
    Summoned,
    /// Undead
    Undead,
    /// Vermin
    Vermin,
}

#[simple_enum(display)]
pub enum Alignment {
    /// Lawful
    Lawful,
    /// Neutral
    Neutral,
    /// Chaotic
    Chaotic,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum EnemyHitDice {
    Standard(u32),
    WithModifier(u32, i32),
    Special(DiceRoll),
}

impl std::fmt::Display for EnemyHitDice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Standard(_) => "Standard",
            Self::WithModifier(_, _) => "With Modifier",
            Self::Special(_) => "Custom",
        })
    }
}

impl EnemyHitDice {
    pub fn display(&self) -> String {
        match self {
            Self::Standard(n) => format!("{}", n),
            Self::WithModifier(n, m) => format!("{}{:+}", n, m),
            Self::Special(roll) => roll.to_notation(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttackRoutine {
    One(DamageRoll),
    Two(DamageRoll, DamageRoll),
    Three(DamageRoll, DamageRoll, DamageRoll),
}

impl std::fmt::Display for AttackRoutine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::One(_) => "One per round",
            Self::Two(_, _) => "Two per round",
            Self::Three(_, _, _) => "Three per round",
        })
    }
}

impl AttackRoutine {
    pub fn display(&self) -> String {
        match self {
            Self::One(r1) => r1.to_notation(),
            Self::Two(r1, r2) => format!("{}/{}", r1.to_notation(), r2.to_notation()),
            Self::Three(r1, r2, r3) => format!("{}/{}/{}", r1.to_notation(), r2.to_notation(), r3.to_notation()),
        }
    }
}