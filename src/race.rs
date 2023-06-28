use simple_enum_macro::simple_enum;

use crate::{dice::{roll, DiceRoll}, character::Attributes};

#[simple_enum(display)]
pub enum Race {
    /// Human
    Human,
    /// Dwarf
    Dwarf,
    /// Elf
    Elf,
    /// Halfling
    Halfling,
    /// Gnome
    Gnome,
    /// Zaharan
    Zaharan,
    /// Thrassian
    Thrassian,
    /// Nobiran
    Nobiran,
}

impl Race {
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
            Self::Gnome => {
                Attributes {
                    strength: DiceRoll::simple_drop_highest(4, 6).roll() as u8,
                    dexterity: DiceRoll::simple(3, 6).roll() as u8,
                    constitution: DiceRoll::simple_drop_lowest(4, 6).roll() as u8,
                    intelligence: DiceRoll::simple_drop_lowest(4, 6).roll() as u8,
                    wisdom: DiceRoll::simple(3, 6).roll() as u8,
                    charisma: DiceRoll::simple(3, 6).roll() as u8,
                }
            },
            Self::Nobiran => {
                let r = DiceRoll::simple_drop_lowest(4, 6);
                Attributes {
                    strength: r.roll() as u8,
                    dexterity: r.roll() as u8,
                    constitution: r.roll() as u8,
                    intelligence: r.roll() as u8,
                    wisdom: r.roll() as u8,
                    charisma: r.roll() as u8,
                }
            },
            Self::Thrassian => {
                Attributes {
                    strength: DiceRoll::simple_drop_lowest(4, 6).roll() as u8,
                    dexterity: DiceRoll::simple_drop_lowest(4, 6).roll() as u8,
                    constitution: DiceRoll::simple_drop_lowest(4, 6).roll() as u8,
                    intelligence: DiceRoll::simple(3, 6).roll() as u8,
                    wisdom: DiceRoll::simple_drop_highest(4, 6).roll() as u8,
                    charisma: DiceRoll::simple_drop_highest(4, 6).roll() as u8,
                }
            },
            Self::Zaharan => {
                Attributes {
                    strength: DiceRoll::simple(3, 6).roll() as u8,
                    dexterity: DiceRoll::simple(3, 6).roll() as u8,
                    constitution: DiceRoll::simple(3, 6).roll() as u8,
                    intelligence: DiceRoll::simple_drop_lowest(4, 6).roll() as u8,
                    wisdom: DiceRoll::simple_drop_lowest(4, 6).roll() as u8,
                    charisma: DiceRoll::simple_drop_lowest(4, 6).roll() as u8,
                }
            },
            Self::Halfling => {
                Attributes {
                    strength: DiceRoll::simple(3, 6).roll() as u8,
                    dexterity: DiceRoll::simple_drop_lowest(4, 6).roll() as u8,
                    constitution: DiceRoll::simple(3, 6).roll() as u8,
                    intelligence: DiceRoll::simple(3, 6).roll() as u8,
                    wisdom: DiceRoll::simple(3, 6).roll() as u8,
                    charisma: DiceRoll::simple(3, 6).roll() as u8,
                }
            }
        }
    }
}

#[simple_enum(display)]
pub enum RaceTable {
    /// Standard Fantasy
    StandardFantasy,
}

impl RaceTable {
    pub fn random_race(&self) -> Race {
        match self {
            Self::StandardFantasy => {
                match roll(DiceRoll::simple(1, 100)) {
                    ..=60 => Race::Human,
                    61..=70 => Race::Elf,
                    71..=85 => Race::Dwarf,
                    86..=90 => Race::Halfling,
                    91..=95 => Race::Gnome,
                    96..=97 => Race::Zaharan,
                    98..=99 => Race::Thrassian,
                    100.. => Race::Nobiran,
                }
            },
        }
    }
}

