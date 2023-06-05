use crate::dice::{self, DiceRoll};



pub struct MortalWoundsResult {
    pub modified_roll: i32,
    pub condition: ConditionAndRecovery,
    pub modifiers: MortalWoundsModifiers,
}

impl MortalWoundsResult {
    pub fn roll(modifiers: MortalWoundsModifiers) -> Self {
        let mut total = dice::roll(DiceRoll::simple(1, 20)) as i32;
        total += modifiers.from_con;
        total += match modifiers.from_hit_dice {
            HitDiceValue::D4 => 0,
            HitDiceValue::D6 => 2,
            HitDiceValue::D8 => 4,
            HitDiceValue::D10 => 6,
            HitDiceValue::D12 => 8,
        };
        total += match modifiers.remaining_hp as f32 / modifiers.max_hp as f32 {
            x if x >= -0.25 => 5,
            x if x >= -0.5 && x < -0.25 => -2,
            x if x >= -1.0 && x < -0.5 => -5,
            x if x >= -2.0 && x < -1.0 => -10,
            _ => -20,
        };
        total += modifiers.from_healing_magic;
        total += modifiers.from_healing_prof;
        if modifiers.applied_horsetail {
            total += 2;
        }
        total += match modifiers.from_treatment_timing {
            TreatmentTiming::OneRound => 2,
            TreatmentTiming::OneTurn => -3,
            TreatmentTiming::OneHour => -5,
            TreatmentTiming::OneDay => -8,
            TreatmentTiming::OverOneDay => -10,
        };
        total += modifiers.other;

        let condition = match total {
            26.. => {ConditionAndRecovery::Dazed},
            21..=25 => {ConditionAndRecovery::KnockedOut},
            16..=20 => {ConditionAndRecovery::InShock},
            11..=15 => {ConditionAndRecovery::CriticallyWounded},
            6..=10 => {ConditionAndRecovery::GrievouslyWounded},
            1..=5 => {ConditionAndRecovery::MortallyWounded},
           -5..=0 => {ConditionAndRecovery::InstantDeath},
            ..=-6 => {ConditionAndRecovery::EvenMoreInstantDeath},
        };

        Self {
            modified_roll: total,
            condition,
            modifiers,
        }
    }
}

pub struct MortalWoundsModifiers {
    pub from_con: i32,
    pub from_hit_dice: HitDiceValue,
    pub remaining_hp: i32,
    pub max_hp: i32,
    pub from_healing_magic: i32,
    pub from_healing_prof: i32,
    pub applied_horsetail: bool,
    pub from_treatment_timing: TreatmentTiming,
    pub other: i32,
}

impl MortalWoundsModifiers {
    pub fn new(from_con: i32, from_hit_dice: HitDiceValue, remaining_hp: i32, max_hp: i32, from_healing_magic: i32, from_healing_prof: i32, applied_horsetail: bool, from_treatment_timing: TreatmentTiming, other: i32) -> Self { 
        Self { 
            from_con, 
            from_hit_dice, 
            remaining_hp, 
            max_hp,
            from_healing_magic, 
            from_healing_prof, 
            applied_horsetail, 
            from_treatment_timing,
            other,
        } 
    }
}

#[derive(Debug, Clone, Copy)]
pub enum HitDiceValue {
    D4,
    D6,
    D8,
    D10,
    D12,
}

#[derive(Debug, Clone, Copy)]
pub enum TreatmentTiming {
    OneRound,
    OneTurn,
    OneHour,
    OneDay,
    OverOneDay,
}

#[derive(Debug, Clone, Copy)]
pub enum ConditionAndRecovery {
    Dazed,
    KnockedOut,
    InShock,
    CriticallyWounded,
    GrievouslyWounded,
    MortallyWounded,
    InstantDeath,
    EvenMoreInstantDeath,
}

impl ConditionAndRecovery {
    pub fn description(&self) -> String {
        match *self {
            ConditionAndRecovery::Dazed => "You were just dazed. You recover immediately with 1hp. You do not need any bed rest.".to_owned(),
            ConditionAndRecovery::KnockedOut => "You were knocked out. You recover with 1 hp. You need magical healing or one night of bed rest.".to_owned(),
            ConditionAndRecovery::InShock => "You are in shock. You recover with 1 hp. You need magical healing and one night of bed rest, or 1 week of bed rest.".to_owned(),
            ConditionAndRecovery::CriticallyWounded => "You are critically wounded. You die unless healed to 1 hp within 1 day. If you are healed, you need 1 week of bed rest.".to_owned(),
            ConditionAndRecovery::GrievouslyWounded => "You are grievously wounded. You die unless healed to 1 hp within 1 turn. If you are healed, you need 2 weeks of bed rest.".to_owned(),
            ConditionAndRecovery::MortallyWounded => "You are mortally wounded. You die unless healed to 1 hp within 1 round. If you are healed, you need 1 month of bed rest.".to_owned(),
            ConditionAndRecovery::InstantDeath => "You were instantly killed.".to_owned(),
            ConditionAndRecovery::EvenMoreInstantDeath => "You were instantly killed.".to_owned(),
        }
    }
}