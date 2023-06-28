use std::{collections::{HashSet, HashMap}};

use crate::{character::Attr, race::Race};
use displaydoc::Display;
use serde::{Deserialize, Serialize};
use simple_enum_macro::simple_enum;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Class {
    pub race: Race,
    pub name: String,
    pub description: String,
    pub prime_reqs: HashSet<Attr>,
    pub maximum_level: u8,
    pub titles: Titles,
    pub hit_die: HitDie,
    pub base_xp_cost: u32,
    pub class_proficiencies: HashMap<String, Option<HashSet<String>>>,
    pub saving_throw_progression_type: SavingThrowProgressionType,
    pub attack_throw_progression: AttackThrowProgression,
    pub weapon_selection: WeaponSelection,
    pub armor_selection: ArmorSelection,
    pub fighting_styles: FightingStyles,
    pub class_damage_bonus: ClassDamageBonus,
    pub cleaves: Cleaves,
    pub thief_skills: ThiefSkills,
    pub divine_value: DivineValue,
    pub arcane_value: ArcaneValue,
}

impl Class {
    pub fn default() -> Self {
        Self {
            race: Race::Human,
            name: String::new(),
            description: String::new(),
            prime_reqs: HashSet::new(),
            maximum_level: 14,
            titles: Titles::new(),
            hit_die: HitDie::D4,
            base_xp_cost: 2000,
            class_proficiencies: HashMap::new(),
            saving_throw_progression_type: SavingThrowProgressionType::Fighter,
            attack_throw_progression: AttackThrowProgression::OnePerThree,
            weapon_selection: WeaponSelection::Unrestricted,
            armor_selection: ArmorSelection::Unrestricted,
            fighting_styles: FightingStyles::all(),
            class_damage_bonus: ClassDamageBonus::None,
            cleaves: Cleaves::None,
            thief_skills: ThiefSkills(HashSet::new()),
            divine_value: DivineValue::None,
            arcane_value: ArcaneValue::None,
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
}

#[simple_enum(display)]
pub enum HitDie {
    /// D4
    D4,
    /// D6
    D6,
    /// D8
    D8,
    /// D10
    D10,
    /// D12
    D12,
}

#[simple_enum(display)]
pub enum SavingThrowProgressionType {
    /// Fighter
    Fighter,
    /// Thief
    Thief,
    /// Cleric
    Cleric,
    /// Mage
    Mage,
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

#[simple_enum(display)]
pub enum AttackThrowProgression {
    /// Once per three levels
    OnePerThree,
    /// Once per two levels
    OnePerTwo,
    /// Twice per three levels
    TwoPerThree,
    /// Once every level
    OnePerOne,
    /// Three times every two levels
    ThreePerTwo,
}

impl AttackThrowProgression {
    pub fn calculate(&self, current_level: u8) -> i32 {
        if current_level == 0 {
            return 9;
        }
        10 + match self {
            Self::OnePerThree => (current_level as i32 - 1) / 3,
            Self::OnePerTwo => (current_level as i32 - 1) / 2,
            Self::TwoPerThree => ((current_level as f64 - 1.0) * (2.0 / 3.0)).round() as i32,
            Self::OnePerOne => current_level as i32 - 1,
            Self::ThreePerTwo => ((current_level as f64 - 1.0) * (3.0 / 2.0)).floor() as i32,
        }
    }
}

#[simple_enum(display)]
pub enum Cleaves {
    /// None
    None,
    /// Half
    Half,
    /// Full
    Full,
}

#[simple_enum(display)]
pub enum ClassDamageBonus {
    /// None
    None,
    /// Only Melee
    MeleeOnly,
    /// Only Missile
    MissileOnly,
    /// Both
    Both,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct FightingStyles {
    pub two_weapons: bool,
    pub weapon_and_shield: bool,
    pub two_handed: bool,
}

impl FightingStyles {
    pub fn new(two_weapons: bool, weapon_and_shield: bool, two_handed: bool) -> Self {
        Self {
            two_weapons,
            weapon_and_shield,
            two_handed,
        }
    }

    pub fn all() -> Self {
        Self {
            two_weapons: true,
            weapon_and_shield: true,
            two_handed: true,
        }
    }

    pub fn display(&self, class_name: &str) -> String {
        let mut list = vec![(self.two_weapons, "two weapons"), (self.weapon_and_shield, "a weapon and shield"), (self.two_handed, "two-handed weapons")];
        list.retain(|(b, _)| *b);
        if list.is_empty() {
            return format!("The {} class may not fight in any style.", class_name);
        }
        let mut s = format!("The {} class may fight with ", class_name);
        for (i, &(_, d)) in list.iter().enumerate() {
            let m = match list.len() {
                1 => format!("{}.", d),
                2 => if i == 0 {format!("{} and ", d)} else {format!("{}.", d)},
                3 => {
                    match i {
                        0 => format!("{}, ", d),
                        1 => format!("{}, and ", d),
                        2 => format!("{}.", d),
                        _ => unreachable!(),
                    }
                },
                _ => unreachable!(),
            };
            s.push_str(&m);
        }
        s
    }
}

#[simple_enum(display)]
pub enum ArmorSelection {
    /// Forbidden
    Forbidden,
    /// Restricted (hide or less)
    Restricted,
    /// Narrow (leather or less)
    Narrow,
    /// Broad (chain or less)
    Broad,
    /// Unrestricted
    Unrestricted,
}

impl ArmorSelection {
    pub fn display(&self) -> String {
        match self {
            Self::Forbidden => "They may not wear armor.",
            Self::Restricted => "They can only wear hide armor or lighter.",
            Self::Narrow => "They can only wear leather armor or lighter.",
            Self::Broad => "They can wear chain armor or lighter.",
            Self::Unrestricted => "They may wear any armor.",
        }.to_owned()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Display)]
pub enum WeaponSelection {
    /// Restricted
    Restricted([RestrictedWeapons; 4]),
    /// Narrow
    Narrow([NarrowWeapons; 2]),
    /// Broad
    Broad([BroadWeapons; 2]),
    /// Unrestricted
    Unrestricted,
}

impl WeaponSelection {
    pub fn display(&self) -> String {
        match self {
            Self::Unrestricted => "They may use any type of melee or missile weapon.".to_owned(),
            Self::Broad(b) => format!("They may use {} and {}.", b[0], b[1]),
            Self::Narrow(n) => format!("They can only use {} and {}.", n[0], n[1]),
            Self::Restricted(r) => format!("They can only use {}, {}, {}, and {}.", r[0], r[1], r[2], r[3]),
        }
    }
}

#[simple_enum(display)]
pub enum RestrictedWeapons {
    /// clubs
    Club,
    /// daggers
    Dagger,
    /// bolas
    Bola,
    /// darts
    Dart,
    /// slings
    Sling,
    /// saps
    Sap,
    /// staves
    Staff,
    /// whips
    Whip,
}

impl RestrictedWeapons {
    pub fn display(&self) -> String {
        match self {
            RestrictedWeapons::Club => "Clubs",
            RestrictedWeapons::Dagger => "Daggers",
            RestrictedWeapons::Bola => "Bolas",
            RestrictedWeapons::Dart => "Darts",
            RestrictedWeapons::Sling => "Slings",
            RestrictedWeapons::Sap => "Saps",
            RestrictedWeapons::Staff => "Staves",
            RestrictedWeapons::Whip => "Whips",
        }.to_owned()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Display)]
pub enum NarrowWeapons {
    /// axes
    Axes,
    /// bows/crossbows
    BowsCrossbows,
    /// flails, hammers, maces
    FlailsHammersMaces,
    /// swords, daggers
    SwordsDaggers,
    /// spears, polearms
    SpearsPolearms,
    /// bolas, darts, nets, slings, saps, staves
    Special,
    /// {0}s, {1}s, {2}s
    AnyThree(String, String, String),
}

impl NarrowWeapons {
    pub fn display(&self) -> String {
        match self {
            NarrowWeapons::Axes => "Axes",
            NarrowWeapons::BowsCrossbows => "Bows and Crossbows",
            NarrowWeapons::FlailsHammersMaces => "Flails, Hammers, and Maces",
            NarrowWeapons::SwordsDaggers => "Swords and Daggers",
            NarrowWeapons::SpearsPolearms => "Spears and Polearms",
            NarrowWeapons::Special => "Bolas, Darts, Nets, Slings, Saps, and Staves",
            NarrowWeapons::AnyThree(_, _, _) => "Any Three Weapons",
        }.to_owned()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Display)]
pub enum BroadWeapons {
    /// one-handed weapons
    OneHanded,
    /// two-handed weapons
    TwoHanded,
    /// axes, flails, hammers, maces
    AxesFlailsHammersMaces,
    /// swords, daggers, spears, polearms
    SwordsDaggersSpearsPolearms,
    /// missile weapons
    Missile,
    /// {0}s, {1}s, {2}s, {3}s, {4}s
    AnyFive(String, String, String, String, String),
}

impl BroadWeapons {
    pub fn display(&self) -> String {
        match self {
            Self::OneHanded => "Any One-Handed Weapon",
            Self::TwoHanded => "Any Two-Handed Weapon",
            Self::AxesFlailsHammersMaces => "Axes, Flails, Hammers, and Maces",
            Self::SwordsDaggersSpearsPolearms => "Swords, Daggers, Spears, and Polearms",
            Self::Missile => "Any Missile Weapon",
            Self::AnyFive(_, _, _, _, _) => "Any Five Weapons",
        }.to_owned()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Titles(pub Vec<String>);

impl Titles {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    pub fn get(&self, current_level: u8) -> String {
        if current_level == 0 {
            return "Citizen".to_owned();
        }
        self.0.get(current_level as usize - 1).map_or("ERROR: Something has gone wrong! There is no defined title for this level!".to_owned(), |t| t.to_owned())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThiefSkills(pub HashSet<ThiefSkill>);

impl ThiefSkills {
    pub fn display(&self, class_name: &str) -> String {
        let mut s = format!("The {} class can ", class_name);
        for (i, skill) in self.0.iter().enumerate() {
            if i == self.0.len() - 1 {
                s.push_str(&format!(", and {}.", skill));
            } else if i == 0 {
                s.push_str(&format!("{}", skill));
            } else {
                s.push_str(&format!(", {}", skill));
            }
        }
        s
    }
}

#[simple_enum(display)]
pub enum ThiefSkill {
    /// Open Locks
    OpenLocks,
    /// Find Traps
    FindTraps,
    /// Remove Traps
    RemoveTraps,
    /// Pick Pockets
    PickPockets,
    /// Move Silently
    MoveSilently,
    /// Climb Walls
    ClimbWalls,
    /// Hide in Shadows
    HideInShadows,
    /// Hear Noise
    HearNoise,
    /// Backstab
    Backstab,
    /// Read Languages
    ReadLanguages,
    /// Use Scrolls
    UseScrolls,
}

pub const THIEF_SKILLS: [ThiefSkill; 11] = [
    ThiefSkill::OpenLocks,
    ThiefSkill::FindTraps,
    ThiefSkill::RemoveTraps,
    ThiefSkill::PickPockets,
    ThiefSkill::MoveSilently,
    ThiefSkill::ClimbWalls,
    ThiefSkill::HideInShadows,
    ThiefSkill::HearNoise,
    ThiefSkill::Backstab,
    ThiefSkill::ReadLanguages,
    ThiefSkill::UseScrolls,
];

#[simple_enum]
pub enum DivineValue {
    None,
    One(bool),
    Two(bool),
    Three(bool),
    Four(bool),
}

impl DivineValue {
    pub fn get_max_spell_slots(&self, mut level: u8) -> [u32; 5] {
        if let Self::One(_) = self {
            level = (level as f64 / 2.0).ceil() as u8;
        }
        let mut cleric: [u32; 5] = match level {
            ..=1 => [0, 0, 0, 0, 0],
            2 => [1, 0, 0, 0, 0],
            3 => [2, 0, 0, 0, 0],
            4 => [2, 1, 0, 0, 0],
            5 => [2, 2, 0, 0, 0],
            6 => [2, 2, 1, 1, 0],
            7 => [2, 2, 2, 1, 1],
            8 => [3, 3, 2, 2, 1],
            9 => [3, 3, 3, 2, 2],
            10 => [4, 4, 3, 3, 2],
            11 => [4, 4, 4, 3, 3],
            12 => [5, 5, 4, 4, 3],
            13 => [5, 5, 5, 4, 3],
            14.. => [6, 5, 5, 5, 4],
        };
        match self {
            Self::None => [0, 0, 0, 0, 0],
            Self::One(_) => cleric,
            Self::Two(_) => cleric,
            Self::Three(_) => {
                for i in &mut cleric {
                    *i += (*i as f64 / 3.0).round() as u32;
                }
                cleric
            },
            Self::Four(_) => {
                for i in &mut cleric {
                    *i += (*i as f64 / 2.0).round() as u32;
                }
                if cleric[0] == 0 {
                    cleric[0] = 1;
                }
                cleric 
            },
        }
    }
}

#[simple_enum]
pub enum ArcaneValue {
    None,
    One(bool),
    Two(bool),
    Three(bool),
    Four,
}

impl ArcaneValue {
    pub fn get_max_spell_slots(&self, mut level: u8) -> [u32; 6] {
        match self {
            Self::None => {
                return [0, 0, 0, 0, 0, 0];
            },
            Self::One(delayed) => {
                if *delayed {
                    if level > 7 {
                        level -= 7;
                    } else {
                        level = 0;
                    }
                } else {
                    level = (level as f64 / 3.0).floor() as u8;
                }
            },
            Self::Two(delayed) => {
                if *delayed {
                    if level > 5 {
                        level -= 5;
                    } else {
                        level = 0;
                    }
                } else {
                    if level == 1 {
                        level = 0;
                    } else {
                        level = (level as f64 / 2.0).round() as u8;
                    }
                }
            },
            Self::Three(delayed) => {
                if *delayed {
                    if level > 3 {
                        level -= 3;
                    } else {
                        level = 0;
                    }
                } else {
                    level = (level as f64 * (2.0 / 3.0)).round() as u8;
                }
            },
            Self::Four => {},
        }
        match level {
            0 => [0, 0, 0, 0, 0, 0],
            1 => [1, 0, 0, 0, 0, 0],
            2 => [2, 0, 0, 0, 0, 0],
            3 => [2, 1, 0, 0, 0, 0],
            4 => [2, 2, 0, 0, 0, 0],
            5 => [2, 2, 1, 0, 0, 0],
            6 => [2, 2, 2, 0, 0, 0],
            7 => [3, 2, 2, 1, 0, 0],
            8 => [3, 3, 2, 2, 0, 0],
            9 => [3, 3, 3, 2, 1, 0],
            10 => [3, 3, 3, 3, 2, 0],
            11 => [4, 3, 3, 3, 2, 1],
            12 => [4, 4, 3, 3, 3, 2],
            13 => [4, 4, 4, 3, 3, 2],
            14.. => [4, 4, 4, 4, 3, 3],
        }
    }

    pub fn get_repertoire_size(&self, level: u8, int_mod: i32) -> [u32; 6] {
        let mut rep = self.get_max_spell_slots(level);
        if int_mod > 0 {
            for i in &mut rep {
                if *i > 0 {
                    *i += int_mod as u32;
                }
            }
        }
        rep
    }
}