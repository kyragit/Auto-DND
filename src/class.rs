use std::collections::{HashSet, HashMap};

use crate::character::Attr;
use displaydoc::Display;
use serde::{Deserialize, Serialize};
use simple_enum_macro::simple_enum;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Class {
    pub name: String,
    pub description: String,
    pub prime_reqs: Vec<Attr>,
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
}

impl Class {
    pub fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            prime_reqs: vec![],
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

#[simple_enum]
pub enum AttackThrowProgression {
    OnePerThree,
    OnePerTwo,
    TwoPerThree,
    OnePerOne,
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

#[simple_enum]
pub enum Cleaves {
    None,
    Half,
    Full,
}

#[simple_enum]
pub enum ClassDamageBonus {
    None,
    MeleeOnly,
    MissileOnly,
    Both,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct FightingStyles {
    two_weapons: bool,
    weapon_and_shield: bool,
    two_handed: bool,
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

#[simple_enum]
pub enum ArmorSelection {
    Forbidden,
    Restricted,
    Narrow,
    Broad,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum WeaponSelection {
    Restricted([RestrictedWeapons; 4]),
    Narrow([NarrowWeapons; 2]),
    Broad([BroadWeapons; 2]),
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

#[derive(Debug, Serialize, Deserialize, Clone, Display)]
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

#[derive(Debug, Serialize, Deserialize, Clone, Display)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Titles(Vec<String>);

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