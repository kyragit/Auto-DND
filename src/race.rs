use serde::{Serialize, Deserialize};

use crate::{dice::{roll, DiceRoll}, character::Attributes};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Race {
    Human,
    Dwarf,
    Elf,
}

impl std::fmt::Display for Race {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Human => "Human",
            Self::Dwarf => "Dwarf",
            Self::Elf => "Elf",
        };
        write!(f, "{}", s)
    }
}

impl Race {
    pub fn random() -> Self {
        match roll(DiceRoll::simple(1, 100)) {
            ..=80 => Self::Human,
            81..=90 => Self::Dwarf,
            91.. => Self::Elf,
        }
    }

    pub fn roll_attrs(&self) -> Attributes {
        match self {
            Self::Human => {
                Attributes::random()
            },
            Self::Dwarf => {
                let simple = DiceRoll::simple(3, 6);
                Attributes {
                    strength: roll(simple) as u8,
                    dexterity: roll(simple) as u8,
                    constitution: roll(DiceRoll::simple_drop_lowest(4, 6)) as u8,
                    intelligence: roll(simple) as u8,
                    wisdom: roll(simple) as u8,
                    charisma: roll(DiceRoll::simple_drop_highest(4, 6)) as u8,
                }
            },
            Self::Elf => {
                let simple = DiceRoll::simple(3, 6);
                Attributes {
                    strength: roll(simple) as u8,
                    dexterity: roll(simple) as u8,
                    constitution: roll(DiceRoll::simple_drop_highest(4, 6)) as u8,
                    intelligence: roll(DiceRoll::simple_drop_lowest(4, 6)) as u8,
                    wisdom: roll(simple) as u8,
                    charisma: roll(simple) as u8,
                }
            },
        }
    }
}

