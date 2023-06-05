use std::collections::HashSet;

use crate::{dice::{roll, DiceRoll, self}, class::{Class, SavingThrowProgressionType}, race::Race, combat::{CombatantStats, Combatant, Damage, StatModifiers, StatusEffects}, item::{Item, ItemType}};
use serde::{Deserialize, Serialize};

/// All the data for a player character, aside from their name (this is for technical reasons and 
/// may change in the future). Essentially, this is everything that would go on a character sheet.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerCharacter {
    pub combat_stats: CombatantStats,
    pub race: Race,
    pub class: Class,
    pub level: u8,
    pub xp: u32,
    pub xp_to_level: u32,
    pub inventory: PlayerInventory,
    pub notes: String,
}

impl PlayerCharacter {
    pub fn random() -> Self {
        let race = Race::random();
        let class = Class::default();
        Self {
            combat_stats: CombatantStats { 
                attributes: race.roll_attrs(), 
                health: Health::new(), 
                attack_throw: 10, 
                armor_class: 0, 
                damage: Damage::default(),
                saving_throws: SavingThrows::new(),
                status_effects: StatusEffects::new(),
                modifiers: StatModifiers::new(),
            },
            race,
            xp_to_level: class.calculate_next_level_cost(1),
            class,
            level: 1,
            xp: 0,
            inventory: PlayerInventory::empty(),
            notes: String::new(),
        }
    }

    pub fn initialize(&mut self) {
        let hp = dice::roll(DiceRoll::new(
            1, 
            match self.class.hit_die {
                crate::class::HitDie::D4 => 4,
                crate::class::HitDie::D6 => 6,
                crate::class::HitDie::D8 => 8,
                crate::class::HitDie::D10 => 10,
                crate::class::HitDie::D12 => 12,
            }, 
            self.combat_stats.attributes.modifier(Attr::CON), 
            dice::ModifierType::Add, false, dice::Drop::None, 1)
        );
        self.combat_stats.health.max_hp = hp;
        self.combat_stats.health.current_hp = hp as i32;
        self.combat_stats.saving_throws = SavingThrows::calculate_simple(self.class.saving_throw_progression_type, self.level);
        self.combat_stats.damage = Damage { amount: 1, sides: 6, modifier: 0 };
        self.combat_stats.modifiers.melee_attack.add("strength", self.combat_stats.attributes.modifier(Attr::STR));
        self.combat_stats.modifiers.melee_damage.add("strength", self.combat_stats.attributes.modifier(Attr::STR));
        self.combat_stats.modifiers.missile_attack.add("dexterity", self.combat_stats.attributes.modifier(Attr::DEX));
        self.combat_stats.modifiers.armor_class.add("dexterity", self.combat_stats.attributes.modifier(Attr::DEX));
        self.combat_stats.modifiers.initiative.add("dexterity", self.combat_stats.attributes.modifier(Attr::DEX));
        self.combat_stats.modifiers.save_petrification_paralysis.add("wisdom", self.combat_stats.attributes.modifier(Attr::WIS));
        self.combat_stats.modifiers.save_poison_death.add("wisdom", self.combat_stats.attributes.modifier(Attr::WIS));
        self.combat_stats.modifiers.save_blast_breath.add("wisdom", self.combat_stats.attributes.modifier(Attr::WIS));
        self.combat_stats.modifiers.save_staffs_wands.add("wisdom", self.combat_stats.attributes.modifier(Attr::WIS));
        self.combat_stats.modifiers.save_spells.add("wisdom", self.combat_stats.attributes.modifier(Attr::WIS));
        let xp_gain = self.class.prime_reqs.iter().map(|attr| self.combat_stats.attributes.modifier(*attr)).min();
        self.combat_stats.modifiers.xp_gain.add("prime_reqs", xp_gain.unwrap_or(0) as f32 * 0.05);

        self.inventory.add(Item { item_type: ItemType::gold(), count: 100 });
        self.inventory.add(Item { item_type: ItemType::silver(), count: 250 });
        self.inventory.add(Item { item_type: ItemType::copper(), count: 325 });
    }
}

impl<'s> Combatant<'s> for PlayerCharacter {
    fn get_combat_stats(&'s self) -> &'s CombatantStats {
        &self.combat_stats
    }
    fn get_combat_stats_mut(&'s mut self) -> &'s mut CombatantStats {
        &mut self.combat_stats
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

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum Attr {
    STR,
    DEX,
    CON,
    INT,
    WIS,
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
            max_hp: 1,
            current_hp: 1,
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
    left_hand: i32,
    right_hand: i32,
    armor: i32,
    clothes: HashSet<u32>,
    cp: i32,
    sp: i32,
    ep: i32,
    gp: i32,
    pp: i32,
}

impl PlayerInventory {
    pub fn empty() -> Self {
        Self {
            total_weight: 0.0,
            items: Vec::new(),
            left_hand: -1,
            right_hand: -1,
            armor: -1,
            clothes: HashSet::new(),
            cp: -1,
            sp: -1,
            ep: -1,
            gp: -1,
            pp: -1,
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

    pub fn add(&mut self, item: Item) {
        self.total_weight += item.item_type.encumbrance.as_float() * item.count as f64;
        self.items.push(item);
    }

    pub fn remove(&mut self, index: u32) -> Option<Item> {
        if index < self.items.len() as u32 {
            let item = self.items.remove(index as usize);
            self.total_weight -= item.item_type.encumbrance.as_float() * item.count as f64;
            self.fix_indexes(index);
            return Some(item);
        }
        None
    }

    fn fix_indexes(&mut self, index: u32) {
        let i = index as i32;
        let fix = |n: &mut i32| {
            if i < *n {
                *n -= 1;
            } else if i == *n {
                *n = -1;
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

        self.clothes.retain(|n| *n as i32 != i);
        self.clothes = self.clothes.drain().map(|mut n| {
            if (n as i32) < i {
                n -= 1;
            }
            n
        }).collect();
    }
}
