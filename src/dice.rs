use rand::prelude::*;
use serde::{Serialize, Deserialize};

/// Represents a roll of one or more dice.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DiceRoll {
    /// How many dice to roll. Should be greater than zero.
    pub amount: u32,
    /// How many sides each die has. Should be greater than zero.
    pub sides: u32,
    /// Modifier to apply. Effect varies based on `modifier_type`.
    pub modifier: i32,
    /// What operation the modifier applies.
    pub modifier_type: ModifierType,
    /// Whether to apply the modifier to every dice individually or to the total.
    pub apply_modifier_to_all: bool,
    /// What, and how many, dice to 'drop' (ignore).
    pub drop: Drop,
    /// The minimum value that this roll will evaluate to.
    pub min_value: i32,
}

impl DiceRoll {
    pub fn new(amount: u32, sides: u32, modifier: i32, modifier_type: ModifierType, apply_modifier_to_all: bool, drop: Drop, min_value: i32) -> Self {
        Self {
            amount,
            sides,
            modifier,
            modifier_type,
            apply_modifier_to_all,
            drop,
            min_value,
        }
    }

    pub fn to_notation(&self) -> String {
        format!("{}d{}{}", self.amount, self.sides, match self.modifier_type {
            ModifierType::Add => format!("{:+}", self.modifier),
            ModifierType::DivideCeil |
            ModifierType::DivideFloor |
            ModifierType::DivideRound => format!("÷{}", self.modifier),
            ModifierType::Multiply => format!("x{}", self.modifier),
        })
    }

    pub fn roll(&self) -> i32 {
        roll(*self)
    }

    pub fn simple(amount: u32, sides: u32) -> Self {
        Self {
            amount,
            sides,
            modifier: 0,
            modifier_type: ModifierType::Add,
            apply_modifier_to_all: false,
            drop: Drop::None,
            min_value: 1,
        }
    }

    pub fn simple_modifier(amount: u32, sides: u32, modifier: i32) -> Self {
        Self {
            amount,
            sides,
            modifier,
            modifier_type: ModifierType::Add,
            apply_modifier_to_all: false,
            drop: Drop::None,
            min_value: 1,
        }
    }

    pub fn simple_drop_highest(amount: u32, sides: u32) -> Self {
        Self {
            amount,
            sides,
            modifier: 0,
            modifier_type: ModifierType::Add,
            apply_modifier_to_all: false,
            drop: Drop::DropHighest(1),
            min_value: 1,
        }
    }

    pub fn simple_drop_lowest(amount: u32, sides: u32) -> Self {
        Self {
            amount,
            sides,
            modifier: 0,
            modifier_type: ModifierType::Add,
            apply_modifier_to_all: false,
            drop: Drop::DropLowest(1),
            min_value: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Eq, PartialOrd, Hash)]
pub enum ModifierType {
    Add,
    Multiply,
    DivideFloor,
    DivideCeil,
    DivideRound,
}

impl ModifierType {
    pub fn to_string(&self) -> String {
        match self {
            ModifierType::Add => "Addition".to_owned(),
            ModifierType::Multiply => "Multiplication".to_owned(),
            ModifierType::DivideFloor => "Division (Round down)".to_owned(),
            ModifierType::DivideCeil => "Division (Round up)".to_owned(),
            ModifierType::DivideRound => "Division (Round)".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Eq, PartialOrd, Hash)]
pub enum Drop {
    None,
    DropHighest(u32),
    DropLowest(u32),
}

impl Drop {
    pub fn to_string(&self) -> String {
        match self {
            Drop::None => "None".to_owned(),
            Drop::DropHighest(_) => "Drop highest".to_owned(),
            Drop::DropLowest(_) => "Drop lowest".to_owned(),
        }
    }
}

/// Rolls a `DiceRoll` and returns the result.
pub fn roll(roll: DiceRoll) -> i32 {
    if roll.amount == 0 || roll.sides == 0 {
        return 0;
    }
    let mut rng = thread_rng();
    let mut result: i32 = 0;
    let mut raw_rolls: Vec<i32> = Vec::new();
    for _ in 0..roll.amount {
        raw_rolls.push(rng.gen_range(1..=roll.sides) as i32);
    }
    if let Drop::DropHighest(i) = roll.drop {
        if i >= roll.amount {
            return 0;
        }
        raw_rolls.sort();
        for _ in 0..i {
            raw_rolls.pop();
        }
    }
    if let Drop::DropLowest(i) = roll.drop {
        if i >= roll.amount {
            return 0;
        }
        raw_rolls.sort();
        raw_rolls.reverse();
        for _ in 0..i {
            raw_rolls.pop();
        }
    }
    for r in raw_rolls {
        if roll.apply_modifier_to_all {
            result += apply_modifier(r, roll.modifier, roll.modifier_type);
        } else {
            result += r;
        }
    }
    if !roll.apply_modifier_to_all {
        result = apply_modifier(result, roll.modifier, roll.modifier_type);
    }
    if result < roll.min_value {
        result = roll.min_value;
    }
    result
}

fn apply_modifier(initial: i32, modifier: i32, modifier_type: ModifierType) -> i32 {
    match modifier_type {
        ModifierType::Add => initial + modifier,
        ModifierType::Multiply => initial * modifier,
        ModifierType::DivideFloor => {
            if modifier == 0 {
                initial
            } else {
                (initial as f32 / modifier as f32).floor() as i32
            }
        },
        ModifierType::DivideCeil => {
            if modifier == 0 {
                initial
            } else {
                (initial as f32 / modifier as f32).ceil() as i32
            }
        },
        ModifierType::DivideRound => {
            if modifier == 0 {
                initial
            } else {
                (initial as f32 / modifier as f32).round() as i32
            }
        },
    }
}