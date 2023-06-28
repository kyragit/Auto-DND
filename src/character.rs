use std::collections::HashSet;

use crate::{dice::{roll, DiceRoll}, class::{Class, SavingThrowProgressionType, HitDie, DivineValue, ArcaneValue}, race::{Race, RaceTable}, combat::{CombatantStats, DamageRoll, StatModifiers, StatusEffects}, item::{Item, ItemType, Encumbrance}, enemy::AttackRoutine, proficiency::{Proficiencies, ProficiencyInstance, PROF_CODE_MAP}};
use array_macro::array;
use serde::{Deserialize, Serialize};
use simple_enum_macro::simple_enum;

/// All the data for a player character, aside from their name (this is for technical reasons and 
/// may change in the future). Essentially, this is everything that would go on a character sheet.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerCharacter {
    pub combat_stats: CombatantStats,
    pub race: Race,
    pub class: Class,
    pub title: String,
    pub level: u8,
    pub xp: u32,
    pub xp_to_level: u32,
    pub inventory: PlayerInventory,
    pub proficiencies: Proficiencies,
    pub divine_spells: Option<DivineSpellcaster>,
    pub arcane_spells: Option<ArcaneSpellcaster>,
    pub notes: String,
}

impl PlayerCharacter {
    pub fn random() -> Self {
        let race = RaceTable::StandardFantasy.random_race();
        let class = Class::default();
        Self {
            combat_stats: CombatantStats { 
                attributes: race.roll_attrs(), 
                health: Health::new(), 
                attack_throw: 10, 
                armor_class: 0, 
                damage: AttackRoutine::One(DamageRoll::default()),
                attack_index: 0,
                saving_throws: SavingThrows::new(),
                status_effects: StatusEffects::new(),
                modifiers: StatModifiers::new(),
            },
            race,
            xp_to_level: 0,
            class,
            title: String::new(),
            level: 0,
            xp: 0,
            inventory: PlayerInventory::empty(),
            proficiencies: Proficiencies::new(),
            divine_spells: None,
            arcane_spells: None,
            notes: String::new(),
        }
    }

    pub fn initialize(&mut self) {
        self.combat_stats.saving_throws = SavingThrows::calculate_simple(self.class.saving_throw_progression_type, self.level);
        self.combat_stats.modifiers.melee_attack.add("strength", self.combat_stats.attributes.modifier(Attr::STR));
        self.combat_stats.modifiers.melee_damage.add("strength", self.combat_stats.attributes.modifier(Attr::STR));
        self.combat_stats.modifiers.missile_attack.add("dexterity", self.combat_stats.attributes.modifier(Attr::DEX));
        self.combat_stats.modifiers.armor_class.add("dexterity", self.combat_stats.attributes.modifier(Attr::DEX));
        self.combat_stats.modifiers.initiative.add("dexterity", self.combat_stats.attributes.modifier(Attr::DEX));
        self.combat_stats.modifiers.add_all_saves("wisdom", self.combat_stats.attributes.modifier(Attr::WIS));
        let xp_gain = self.class.prime_reqs.iter().map(|attr| self.combat_stats.attributes.modifier(*attr)).min();
        self.combat_stats.modifiers.xp_gain.add("prime_reqs", xp_gain.unwrap_or(0) as f64 * 0.05);
        self.proficiencies.class_slots = 1;
        self.proficiencies.general_slots = 1;
        if self.combat_stats.attributes.modifier(Attr::INT) > 0 {
            self.proficiencies.general_slots += self.combat_stats.attributes.modifier(Attr::INT) as u8;
        }
        if self.class.divine_value != DivineValue::None {
            self.divine_spells = Some(DivineSpellcaster {
                spell_slots: array![(0, 0); 5],
                spell_repertoire: array![HashSet::new(); 5],
            });
        }
        if self.class.arcane_value != ArcaneValue::None {
            self.arcane_spells = Some(ArcaneSpellcaster {
                spell_slots: array![(0, 0); 6],
                spell_repertoire: array![(HashSet::new(), 0); 6],
            });
        }
        self.level_up();
    }

    pub fn level_up(&mut self) {
        if self.level >= self.class.maximum_level {
            return;
        }
        self.level += 1;
        let hp = self.roll_hit_die();
        self.combat_stats.health.max_hp += hp;
        self.combat_stats.health.current_hp = self.combat_stats.health.max_hp as i32;
        self.combat_stats.saving_throws = SavingThrows::calculate_simple(self.class.saving_throw_progression_type, self.level);
        self.combat_stats.attack_throw = self.class.attack_throw_progression.calculate(self.level);
        self.xp_to_level = self.class.calculate_next_level_cost(self.level);
        self.title = self.class.titles.get(self.level);
        match self.level {
            5 | 9 | 13 => {
                self.proficiencies.general_slots += 1;
            },
            _ => {},
        }
        match self.class.saving_throw_progression_type {
            SavingThrowProgressionType::Fighter => {
                match self.level {
                    3 | 6 | 9 | 12 => {
                        self.proficiencies.class_slots += 1;
                    },
                    _ => {},
                }
            },
            SavingThrowProgressionType::Cleric | SavingThrowProgressionType::Thief => {
                match self.level {
                    4 | 8 | 12 => {
                        self.proficiencies.class_slots += 1;
                    },
                    _ => {},
                }
            },
            SavingThrowProgressionType::Mage => {
                match self.level {
                    6 | 12 => {
                        self.proficiencies.class_slots += 1;
                    },
                    _ => {},
                }
            },
        }
        if let Some(divine) = &mut self.divine_spells {
            let slots = self.class.divine_value.get_max_spell_slots(self.level);
            divine.spell_slots = [(slots[0], slots[0]), (slots[1], slots[1]), (slots[2], slots[2]), (slots[3], slots[3]), (slots[4], slots[4])];
        }
        if let Some(arcane) = &mut self.arcane_spells {
            let slots = self.class.arcane_value.get_max_spell_slots(self.level);
            let rep = self.class.arcane_value.get_repertoire_size(self.level, self.combat_stats.attributes.modifier(Attr::INT));
            arcane.spell_slots = [(slots[0], slots[0]), (slots[1], slots[1]), (slots[2], slots[2]), (slots[3], slots[3]), (slots[4], slots[4]), (slots[5], slots[5])];
            for i in 0..=5 {
                arcane.spell_repertoire[i].1 = rep[i];
            }
        }
    }

    pub fn roll_hit_die(&self) -> u32 {
        if self.level > 9 {
            match self.class.saving_throw_progression_type {
                SavingThrowProgressionType::Cleric |
                SavingThrowProgressionType::Mage => 1,
                SavingThrowProgressionType::Fighter |
                SavingThrowProgressionType::Thief => 2,
            }
        } else {
            DiceRoll::simple_modifier(
                1, 
                match self.class.hit_die {
                    HitDie::D4 => 4,
                    HitDie::D6 => 6,
                    HitDie::D8 => 8,
                    HitDie::D10 => 10,
                    HitDie::D12 => 12,
                }, 
                self.combat_stats.attributes.modifier(Attr::CON),
            ).roll() as u32
        }
    }

    pub fn add_xp(&mut self, mut amount: u32) {
        amount = (amount as i32 + (amount as f64 * self.combat_stats.modifiers.xp_gain.total()).round() as i32) as u32;
        self.xp += amount;
        while self.xp >= self.xp_to_level {
            self.level_up();
        }
    }

    pub fn add_prof(&mut self, id: &str, prof: ProficiencyInstance) {
        PROF_CODE_MAP.trigger_add(id, self, &prof);
        self.proficiencies.profs.insert((id.to_owned(), prof.specification.clone()), prof);
    }

    pub fn remove_prof(&mut self, id: &(String, Option<String>)) {
        if let Some(prof) = self.proficiencies.profs.remove(id) {
            PROF_CODE_MAP.trigger_remove(&id.0, self, &prof);
        }
    }

    pub fn has_prof(&self, id: impl Into<String>, spec: Option<impl Into<String>>) -> bool {
        self.proficiencies.profs.contains_key(&(id.into(), spec.map(|s| s.into())))
    }
}

/// The six basic attributes (ability scores).
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Attributes {
    pub strength: u8,
    pub dexterity: u8,
    pub constitution: u8,
    pub intelligence: u8,
    pub wisdom: u8,
    pub charisma: u8,
}

impl Attributes {
    pub fn random() -> Self {
        let r = DiceRoll::simple(3, 6);
        Self {
            strength: roll(r) as u8,
            dexterity: roll(r) as u8,
            constitution: roll(r) as u8,
            intelligence: roll(r) as u8,
            wisdom: roll(r) as u8,
            charisma: roll(r) as u8,
        }
    }
    /// Gets the attribute modifier for the specified attribute.
    pub fn modifier(&self, attr: Attr) -> i32 {
        let a = match attr {
            Attr::STR => self.strength,
            Attr::DEX => self.dexterity,
            Attr::CON => self.constitution,
            Attr::INT => self.intelligence,
            Attr::WIS => self.wisdom,
            Attr::CHA => self.charisma,
        };
        match a {
            18.. => 3,
            16..=17 => 2,
            13..=15 => 1,
            9..=12 => 0,
            6..=8 => -1,
            4..=5 => -2,
            ..=3 => -3,
        }
    }
}

#[simple_enum(display)]
pub enum Attr {
    /// STR
    STR,
    /// DEX
    DEX,
    /// CON
    CON,
    /// INT
    INT,
    /// WIS
    WIS,
    /// CHA
    CHA,
}

/// All the information relating to an entity's health, i.e. its current and max hp.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Health {
    pub max_hp: u32,
    pub current_hp: i32,
}

impl Health {
    pub fn new() -> Self {
        Self {
            max_hp: 0,
            current_hp: 0,
        }
    }
}

/// The saving throw modifiers. Note this is target 20 rather than the way ACKS normally does it
/// (normally, saving throw of 15+ means roll 1d20 + modifiers, 15 or more is success. In target
/// 20, roll 1d20 + saving throw + modifiers, 20 or more is success. To convert, simply do 
/// 20 - old target = new modifier).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SavingThrows {
    pub petrification_paralysis: i32,
    pub poison_death: i32,
    pub blast_breath: i32,
    pub staffs_wands: i32,
    pub spells: i32,
}

impl SavingThrows {
    pub fn new() -> Self {
        Self { 
            petrification_paralysis: 0, 
            poison_death: 0, 
            blast_breath: 0, 
            staffs_wands: 0, 
            spells: 0
        }
    }

    pub fn from(petrification_paralysis: i32, poison_death: i32, blast_breath: i32, staffs_wands: i32, spells: i32) -> Self {
        Self {
            petrification_paralysis,
            poison_death,
            blast_breath,
            staffs_wands,
            spells,
        }
    }

    pub fn apply_mod(mut self, modifier: i32) -> Self {
        self.blast_breath += modifier;
        self.petrification_paralysis += modifier;
        self.poison_death += modifier;
        self.spells += modifier;
        self.staffs_wands += modifier;
        self
    }

    pub fn calculate(save_type: SavingThrowProgressionType, level: u8, attrs: Attributes) -> Self {
        if level == 0 {
            return Self::from(4, 5, 3, 3, 2).apply_mod(attrs.modifier(Attr::WIS));
        }
        let base = match save_type {
            SavingThrowProgressionType::Fighter => Self::from(5, 6, 4, 4, 3),
            SavingThrowProgressionType::Cleric => Self::from(7, 10, 4, 7, 5),
            SavingThrowProgressionType::Mage => Self::from(7, 7, 5, 9, 8),
            SavingThrowProgressionType::Thief => Self::from(7, 7, 4, 6, 5),
        };
        let level_mod: i32 = match save_type {
            SavingThrowProgressionType::Fighter => {
                // i am positive there is a procedural way to do this but i just cannot figure it
                // out. now this one has to be different
                match level {
                    ..=1 => 0,
                    2..=3 => 1,
                    4 => 2,
                    5..=6 => 3,
                    7 => 4,
                    8..=9 => 5,
                    10 => 6,
                    11..=12 => 7,
                    13 => 8,
                    14.. => 9,
                }
            },
            SavingThrowProgressionType::Cleric | SavingThrowProgressionType::Thief => (level as i32 - 1) / 2,
            SavingThrowProgressionType::Mage => (level as i32 - 1) / 3,
        };
        base.apply_mod(level_mod).apply_mod(attrs.modifier(Attr::WIS))
    }

    pub fn calculate_simple(save_type: SavingThrowProgressionType, level: u8) -> Self {
        Self::calculate(save_type, level, Attributes { strength: 9, dexterity: 9, constitution: 9, intelligence: 9, wisdom: 9, charisma: 9 })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInventory {
    total_weight: f64,
    items: Vec<Item>,
    pub left_hand: Option<usize>,
    pub right_hand: Option<usize>,
    pub armor: Option<usize>,
    pub clothes: HashSet<usize>,
    cp: Option<usize>,
    sp: Option<usize>,
    ep: Option<usize>,
    gp: Option<usize>,
    pp: Option<usize>,
}

impl PlayerInventory {
    pub fn empty() -> Self {
        Self {
            total_weight: 0.0,
            items: Vec::new(),
            left_hand: None,
            right_hand: None,
            armor: None,
            clothes: HashSet::new(),
            cp: None,
            sp: None,
            ep: None,
            gp: None,
            pp: None,
        }
    }

    pub fn get_equip_slot(&self, slot: PlayerEquipSlot) -> Option<&Item> {
        match slot {
            PlayerEquipSlot::LeftHand => self.left_hand,
            PlayerEquipSlot::RightHand | PlayerEquipSlot::BothHands => self.right_hand,
            PlayerEquipSlot::Armor => self.armor,
            PlayerEquipSlot::CP => self.cp,
            PlayerEquipSlot::SP => self.sp,
            PlayerEquipSlot::EP => self.ep,
            PlayerEquipSlot::GP => self.gp,
            PlayerEquipSlot::PP => self.pp,
        }.map_or(None, |i| self.items.get(i))
    }

    pub fn foreach_clothes<F: FnMut(&Item)>(&self, mut func: F) {
        for &i in &self.clothes {
            if let Some(item) = self.items.get(i as usize) {
                func(item);
            }
        }
    }

    pub fn total_weight(&self) -> f64 {
        self.total_weight
    }

    pub fn foreach<F: FnMut(&Item)>(&self, mut func: F) {
        for item in &self.items {
            func(item);
        }
    }

    pub fn foreach_enumerate<F: FnMut(usize, &Item)>(&self, mut func: F) {
        let mut i = 0usize;
        for item in &self.items {
            func(i, item);
            i += 1;
        }
    }

    pub fn add(&mut self, item: Item) {
        if item.item_type.tags.contains("pp") {
            self.add_currency(Currency::PP, item.count);
            return;
        }
        if item.item_type.tags.contains("gp") {
            self.add_currency(Currency::GP, item.count);
            return;
        }
        if item.item_type.tags.contains("ep") {
            self.add_currency(Currency::EP, item.count);
            return;
        }
        if item.item_type.tags.contains("sp") {
            self.add_currency(Currency::SP, item.count);
            return;
        }
        if item.item_type.tags.contains("cp") {
            self.add_currency(Currency::CP, item.count);
            return;
        }
        self.total_weight += item.item_type.encumbrance.as_float() * item.count as f64;
        self.items.push(item);
    }

    fn add_currency(&mut self, currency: Currency, amount: u32) {
        self.total_weight += Encumbrance::Treasure.as_float() * amount as f64;
        match currency {
            Currency::CP => {
                if let Some(i) = self.cp {
                    if let Some(item) = self.items.get_mut(i) {
                        item.count += amount;
                        return;
                    }
                }
                self.cp = Some(self.items.len());
                self.items.push(Item { item_type: ItemType::copper(), count: amount });
            },
            Currency::SP => {
                if let Some(i) = self.sp {
                    if let Some(item) = self.items.get_mut(i) {
                        item.count += amount;
                        return;
                    }
                }
                self.sp = Some(self.items.len());
                self.items.push(Item { item_type: ItemType::silver(), count: amount });
            },
            Currency::EP => {
                if let Some(i) = self.ep {
                    if let Some(item) = self.items.get_mut(i) {
                        item.count += amount;
                        return;
                    }
                }
                self.ep = Some(self.items.len());
                self.items.push(Item { item_type: ItemType::electrum(), count: amount });
            },
            Currency::GP => {
                if let Some(i) = self.gp {
                    if let Some(item) = self.items.get_mut(i) {
                        item.count += amount;
                        return;
                    }
                }
                self.gp = Some(self.items.len());
                self.items.push(Item { item_type: ItemType::gold(), count: amount });
            },
            Currency::PP => {
                if let Some(i) = self.pp {
                    if let Some(item) = self.items.get_mut(i) {
                        item.count += amount;
                        return;
                    }
                }
                self.pp = Some(self.items.len());
                self.items.push(Item { item_type: ItemType::platinum(), count: amount });
            },
        }
    }

    pub fn remove(&mut self, index: usize) -> Option<Item> {
        if index < self.items.len() {
            let item = self.items.remove(index);
            self.total_weight -= item.item_type.encumbrance.as_float() * item.count as f64;
            self.fix_indexes(index);
            return Some(item);
        }
        None
    }

    fn fix_indexes(&mut self, index: usize) {
        let fix = |maybe_n: &mut Option<usize>| {
            if let Some(n) = maybe_n {
                if index < *n {
                    *n -= 1;
                } else if index == *n {
                    *maybe_n = None;
                }
            }
        };
        fix(&mut self.left_hand);
        fix(&mut self.right_hand);
        fix(&mut self.armor);
        fix(&mut self.cp);
        fix(&mut self.sp);
        fix(&mut self.ep);
        fix(&mut self.gp);
        fix(&mut self.pp);

        self.clothes.retain(|n| *n != index);
        self.clothes = self.clothes.drain().map(|mut n| {
            if n < index {
                n -= 1;
            }
            n
        }).collect();
    }

    pub fn move_up(&mut self, index: usize) {
        if index == 0 {
            return;
        }
        if index < self.items.len() {
            self.items.swap(index, index - 1);
            self.swap_indexes(index, index - 1);
        }
    }

    pub fn move_down(&mut self, index: usize) {
        if index < self.items.len() - 1 {
            self.items.swap(index, index + 1);
            self.swap_indexes(index, index + 1);
        }
    }

    fn swap_indexes(&mut self, n: usize, m: usize) {
        let fix = |i: &mut Option<usize>| {
            if let Some(i) = i {
                if *i == n {
                    *i = m;
                    return;
                }
                if *i == m {
                    *i = n;
                    return;
                }
            }
        };
        fix(&mut self.left_hand);
        fix(&mut self.right_hand);
        fix(&mut self.armor);
        fix(&mut self.cp);
        fix(&mut self.sp);
        fix(&mut self.ep);
        fix(&mut self.gp);
        fix(&mut self.pp);
        let add_m = self.clothes.remove(&n);
        let add_n = self.clothes.remove(&m);
        if add_m {
            self.clothes.insert(m);
        }
        if add_n {
            self.clothes.insert(n);
        }
    }

    pub fn equip(&mut self, slot: PlayerEquipSlot, index: usize) {
        match slot {
            PlayerEquipSlot::LeftHand => {
                self.left_hand = Some(index);
            },
            PlayerEquipSlot::RightHand => {
                self.right_hand = Some(index);
            },
            PlayerEquipSlot::BothHands => {
                self.left_hand = Some(index);
                self.right_hand = Some(index); 
            },
            PlayerEquipSlot::Armor => {
                self.armor = Some(index);
            },
            _ => {},
        }
    }

    pub fn unequip(&mut self, slot: PlayerEquipSlot) {
        match slot {
            PlayerEquipSlot::LeftHand => {
                self.left_hand = None;
            },
            PlayerEquipSlot::RightHand => {
                self.right_hand = None;
            },
            PlayerEquipSlot::BothHands => {
                self.left_hand = None;
                self.right_hand = None;
            },
            PlayerEquipSlot::Armor => {
                self.armor = None;
            },
            _ => {},
        }
    }

    pub fn is_equipped(&self, index: usize) -> Option<PlayerEquipSlot> {
        let mut is_right = false;
        let mut is_left = false;
        if let Some(right) = self.right_hand {
            if right == index {
                is_right = true;
            }
        }
        if let Some(left) = self.left_hand {
            if left == index {
                is_left = true;
            }
        }
        if is_right && is_left {
            Some(PlayerEquipSlot::BothHands)
        } else if is_right {
            Some(PlayerEquipSlot::RightHand)
        } else if is_left {
            Some(PlayerEquipSlot::LeftHand)
        } else {
            None
        }
    }
}

#[simple_enum(display)]
pub enum PlayerEquipSlot {
    /// Left Hand
    LeftHand,
    /// Right Hand
    RightHand,
    /// Both Hands
    BothHands,
    /// Armor
    Armor,
    /// CP
    CP,
    /// SP
    SP,
    /// EP
    EP,
    /// GP
    GP,
    /// PP
    PP,
}

#[simple_enum]
enum Currency {
    CP,
    SP,
    EP,
    GP,
    PP,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DivineSpellcaster {
    /// form is [(current, maximum); spell_level]
    pub spell_slots: [(u32, u32); 5],
    pub spell_repertoire: [HashSet<String>; 5],
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArcaneSpellcaster {
    /// form is [(current, maximum); spell_level]
    pub spell_slots: [(u32, u32); 6],
    pub spell_repertoire: [(HashSet<String>, u32); 6],
}
