use std::{collections::{HashMap, HashSet}, path::Path};

use lazy_static::lazy_static;
use serde::{Serialize, Deserialize};

use crate::character::PlayerCharacter;

lazy_static! {
    pub static ref PROF_CODE_MAP: ProficiencyCodeMap = {
        let mut map = ProficiencyCodeMap {
            on_added: HashMap::new(),
            on_removed: HashMap::new(),
        };
        map.on_add("divine_blessing", |sheet, _| sheet.combat_stats.modifiers.add_all_saves("divine_blessing", 2));
        map.on_remove("divine_blessing", |sheet, _| sheet.combat_stats.modifiers.remove_all_saves("divine_blessing"));
        map
    };
}


pub struct ProficiencyCodeMap {
    on_added: HashMap<String, fn(&mut PlayerCharacter, &ProficiencyInstance)>,
    on_removed: HashMap<String, fn(&mut PlayerCharacter, &ProficiencyInstance)>,
}

impl ProficiencyCodeMap {
    fn on_add(&mut self, prof: impl Into<String>, func: fn(&mut PlayerCharacter, &ProficiencyInstance)) {
        self.on_added.insert(prof.into(), func);
    }
    fn on_remove(&mut self, prof: impl Into<String>, func: fn(&mut PlayerCharacter, &ProficiencyInstance)) {
        self.on_removed.insert(prof.into(), func);
    }

    pub fn trigger_add(&self, id: impl Into<String>, sheet: &mut PlayerCharacter, prof: &ProficiencyInstance) {
        if let Some(func) = self.on_added.get(&id.into()) {
            func(sheet, prof);
        }
    }
    pub fn trigger_remove(&self, id: impl Into<String>, sheet: &mut PlayerCharacter, prof: &ProficiencyInstance) {
        if let Some(func) = self.on_removed.get(&id.into()) {
            func(sheet, prof);
        }
    }
}

/// Data representing a proficiency.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Proficiency {
    pub name: String,
    pub description: String,
    pub is_general: bool,
    pub max_level: u8,
    pub requires_specification: bool,
    pub valid_specifications: Option<HashSet<String>>,
    pub starting_throw: Option<i32>,
}

impl Proficiency {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            is_general: true,
            max_level: 0,
            requires_specification: false,
            valid_specifications: None,
            starting_throw: None,
        }
    }

    pub fn save(&self, file: &str) -> Result<(), ()> {
        if let Ok(s) = ron::to_string(self) {
            let file = format!("proficiencies/{}.ron", file);
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
}

/// A *specific* proficiency for a character. This includes the specification (e.g. Craft(Jeweler)),
/// if present. It also stores the proficiency level, in the case that it was taken multiple times.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProficiencyInstance {
    pub prof: Proficiency,
    pub prof_level: u8,
    pub specification: Option<String>,
    pub throw: Option<i32>,
}

impl ProficiencyInstance {
    pub fn from_prof(prof: Proficiency) -> Self {
        Self {
            specification: if prof.requires_specification {Some("".to_owned())} else {None},
            prof_level: 0,
            throw: prof.starting_throw.clone(),
            prof,
        }
    }

    pub fn display(&self) -> String {
        format!("{}{}{}", self.prof.name, self.specification.as_ref().map_or("".to_owned(), |s| format!(" ({})", s)), if self.prof_level > 0 {roman_numeral(self.prof_level + 1)} else {""})
    }
}

fn roman_numeral(n: u8) -> &'static str {
    // yes, this is dumb. I'm not trying to make a good roman numeral function here, I've done that
    // for leetcode already. This is never gonna be higher than like 3 so shut up
    match n {
        1 => " I",
        2 => " II",
        3 => " III",
        4 => " IV",
        5 => " V",
        6 => " VI",
        7 => " VII",
        8 => " VIII",
        9 => " IX",
        10 => " X",
        _ => "",
    }
}

/// An object for holding proficiencies. 
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Proficiencies {
    pub general_slots: u8,
    pub class_slots: u8,
    pub profs: HashMap<(String, Option<String>), ProficiencyInstance>,
}

impl Proficiencies {
    pub fn new() -> Self {
        Self {
            general_slots: 0,
            class_slots: 0,
            profs: HashMap::new(),
        }
    }
}