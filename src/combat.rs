use std::{collections::{HashMap, hash_map::Iter, HashSet}, ops::{AddAssign, SubAssign}, cmp::Ordering};

use serde::{Serialize, Deserialize};

use crate::{character::{Attributes, Health, SavingThrows}, dm_app::DMAppData, packets::{ClientBoundPacket, CombatAction}, dice::{self, DiceRoll}};


pub trait Combatant<'s> {
    fn get_combat_stats(&'s self) -> &'s CombatantStats;
    fn get_combat_stats_mut(&'s mut self) -> &'s mut CombatantStats;
}

/// All the stats required for something to engage in combat. All of these are *base* stats, before
/// any modifiers! This means `armor_class` will be zero for most characters, unless they have 
/// innate armor! All modifiers are stored in `modifiers`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CombatantStats {
    pub attributes: Attributes,
    pub health: Health,
    pub attack_throw: i32,
    pub armor_class: i32,
    pub damage: Damage,
    pub saving_throws: SavingThrows,
    pub status_effects: StatusEffects,
    pub modifiers: StatModifiers,
}

impl CombatantStats {
    pub fn empty() -> Self {
        Self {
            attributes: Attributes {
                strength: 0,
                dexterity: 0,
                constitution: 0,
                intelligence: 0,
                wisdom: 0,
                charisma: 0,
            },
            health: Health::new(),
            attack_throw: 0,
            armor_class: 0,
            damage: Damage::default(),
            saving_throws: SavingThrows::new(),
            status_effects: StatusEffects::new(),
            modifiers: StatModifiers::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatusEffects {
    pub effects: HashSet<StatusEffect>,
}

impl StatusEffects {
    pub fn new() -> Self {
        Self {
            effects: HashSet::new(),
        }
    }

    pub fn is(&self, effect: StatusEffect) -> bool {
        self.effects.contains(&effect)
    }

    pub fn is_helpless(&self) -> bool {
        for effect in &self.effects {
            match effect {
                StatusEffect::Sleeping |
                StatusEffect::Paralyzed => {
                    return true;
                },
                _ => {},
            }
        }
        false
    }

    pub fn is_incapacitated(&self) -> bool {
        for effect in &self.effects {
            match effect {
                StatusEffect::Sleeping |
                StatusEffect::Dying |
                StatusEffect::Dead |
                StatusEffect::Paralyzed => {
                    return true;
                },
                _ => {},
            }
        }
        false
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusEffect {
    Dead,
    Dying,
    Sleeping,
    Paralyzed,
    Concentrating,
}

/// Represents the base damage roll for something.
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Damage {
    pub amount: u32,
    pub sides: u32,
    pub modifier: i32,
}

impl Damage {
    pub fn default() -> Self {
        Self {
            amount: 1,
            sides: 2,
            modifier: 0,
        }
    }
}

/// Stores ALL active modifiers for every stat, including permanent and temporary modifiers. Each
/// modifier needs a unique key that specifies where it cam from (proficiencies, class bonuses, etc).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatModifiers {
    pub melee_attack: StatMod<i32>,
    pub missile_attack: StatMod<i32>,
    pub melee_damage: StatMod<i32>,
    pub missile_damage: StatMod<i32>,
    pub initiative: StatMod<i32>,
    pub surprise: StatMod<i32>,
    pub armor_class: StatMod<i32>,
    pub xp_gain: StatMod<f32>,
    pub save_petrification_paralysis: StatMod<i32>,
    pub save_poison_death: StatMod<i32>,
    pub save_blast_breath: StatMod<i32>,
    pub save_staffs_wands: StatMod<i32>,
    pub save_spells: StatMod<i32>,
}

impl StatModifiers {
    pub fn new() -> Self {
        Self {
            melee_attack: StatMod::new(0),
            missile_attack: StatMod::new(0),
            melee_damage: StatMod::new(0),
            missile_damage: StatMod::new(0),
            initiative: StatMod::new(0),
            surprise: StatMod::new(0),
            armor_class: StatMod::new(0),
            xp_gain: StatMod::new(0.0),
            save_petrification_paralysis: StatMod::new(0),
            save_poison_death: StatMod::new(0),
            save_blast_breath: StatMod::new(0),
            save_staffs_wands: StatMod::new(0),
            save_spells: StatMod::new(0),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatMod<T: AddAssign + SubAssign + Clone + Copy> {
    total: T,
    map: HashMap<String, T>,
}

impl<T: AddAssign + SubAssign + Clone + Copy> StatMod<T> {
    pub fn new(initial: T) -> Self {
        Self {
            total: initial,
            map: HashMap::new(),
        }
    }

    /// Returns the sum of all current modifiers.
    pub fn total(&self) -> T {
        self.total
    }

    /// Adds a modifier with the specified key, and returns the previous modifier at that key.
    pub fn add(&mut self, key: impl Into<String> + Clone, value: T) -> Option<T> {
        let previous = self.remove(key.clone());
        self.map.insert(key.into(), value);
        self.total += value;
        previous
    } 

    /// Removes a modifier with the given key, and returns the removed value.
    pub fn remove(&mut self, key: impl Into<String>) -> Option<T> {
        if let Some(value) = self.map.remove(&key.into()) {
            self.total -= value;
            return Some(value);
        }
        None
    }

    /// Returns true if there is a modifier at the given key.
    pub fn has_modifier(&self, key: impl Into<String>) -> bool {
        self.map.contains_key(&key.into())
    }

    pub fn view_all(&self) -> Iter<String, T> {
        self.map.iter()
    }
}

/// An active fight between combatants.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Fight {
    pub combatants: Vec<(Owner, CombatantType)>,
    pub current_turn: usize,
    pub ongoing_round: bool,
    pub awaiting_response: Option<Owner>,
}

impl Fight {
    pub fn new(combatants: Vec<(Owner, CombatantType)>) -> Self {
        Self {
            combatants,
            current_turn: 0,
            ongoing_round: false,
            awaiting_response: None,
        }
    }

    pub fn get_current_actor(&self) -> CombatantType {
        self.combatants.get(self.current_turn).map_or(CombatantType::not_found(), |(_, t)| t.clone())
    }

    pub fn start_round(&mut self, data: &mut DMAppData) {
        let mut drained = self.combatants.drain(..).map(|(owner, ctype)| {
            let mut r = dice::roll(DiceRoll::simple(1, 6)) as i32;
            r += data.get_combatant_stats_alt(&ctype, |s| s.modifiers.initiative.total()).unwrap_or(0);
            (owner, ctype, r)
        }).collect::<Vec<(Owner, CombatantType, i32)>>();
        drained.sort_unstable_by(|a, b| {
            if a.2 == b.2 {
                match a.0 {
                    Owner::DM => {
                        match b.0 {
                            Owner::DM => {
                                Ordering::Equal
                            },
                            Owner::Player(_) => {
                                Ordering::Less
                            }
                        }
                    },
                    Owner::Player(_) => {
                        match b.0 {
                            Owner::DM => {
                                Ordering::Greater
                            },
                            Owner::Player(_) => {
                                Ordering::Equal
                            }
                        }
                    },
                }
            } else {
                a.2.cmp(&b.2)
            }
        });
        self.combatants = drained.into_iter().map(|(o, c, _)| (o, c)).collect();
        self.combatants.reverse();
        data.log_public("Round started!");
        self.ongoing_round = true;
    }

    pub fn next_turn(&mut self, data: &mut DMAppData) {
        if let Some((owner, ctype)) = self.combatants.get(self.current_turn) {
            if data.get_combatant_stats_alt(ctype, |s| s.status_effects.is_incapacitated()).unwrap_or(false) {
                data.log_public(format!("{} is unable to act!", ctype.name()));
                self.current_turn += 1;
                return;
            }
            data.log_public(format!("It is {}'s turn!", ctype.name()));
            let mut list = vec![];
            for (_, comb) in &self.combatants {
                if comb.id() == ctype.id() {
                    continue;
                }
                list.push(comb.clone());
            }
            match owner {
                Owner::DM => {
                    data.temp_state.combatant_list = list;
                },
                Owner::Player(player) => {
                    data.send_to_user(ClientBoundPacket::DecideCombatAction(ctype.clone(), list), player.clone());
                },
            }
            self.awaiting_response = Some(owner.clone());
        } else {
            self.current_turn = 0;
            data.log_public("Round concluded!");
            self.ongoing_round = false;
        }
    }

    pub fn resolve_action(&mut self, data: &mut DMAppData, action: CombatAction) {
        match &action {
            CombatAction::Attack(target) => {
                let current_actor = self.get_current_actor();
                match attack_roll(data, &current_actor, target) {
                    AttackResult::CriticalFail => {
                        let msg = match dice::roll(DiceRoll::simple(1, 6)) {
                            ..=1 => format!("{} failed miserably when attacking {}!", current_actor.name(), target.name()),
                            2 => format!("{} critically missed {}!", current_actor.name(), target.name()),
                            3 => format!("{} utterly whiffed an attempt to hit {}!", current_actor.name(), target.name()),
                            4 => format!("{} absolutely annihilated the air nearby to {}.", current_actor.name(), target.name()),
                            5 => format!("Whatever {} tried to do to {}, it didn\'t work very well.", current_actor.name(), target.name()),
                            6.. => format!("{} lands a devastating warning blow toward {}! It did absolutely nothing.", current_actor.name(), target.name()),
                        };
                        data.log_public(msg);
                    },
                    AttackResult::Fail => {
                        data.log_public(format!("{} missed {}!", current_actor.name(), target.name()));
                    },
                    AttackResult::Success => {
                        let damage = damage_roll(data, &current_actor, false);
                        let mut killed = false;
                        data.get_combatant_stats(target, |stats| {
                            if let Some(stats) = stats {
                                stats.health.current_hp -= damage;
                                if stats.health.current_hp <= 0 {
                                    stats.status_effects.effects.insert(StatusEffect::Dying);
                                    killed = true;
                                }
                            }
                        });
                        data.log_public(format!("{} hit {} for {} damage!", current_actor.name(), target.name(), damage));
                        if killed {
                            data.log_public(format!("{} was killed!", target.name()));
                        }
                    },
                    AttackResult::CriticalSuccess => {
                        let damage = damage_roll(data, &current_actor, true);
                        let mut killed = false;
                        data.get_combatant_stats(target, |stats| {
                            if let Some(stats) = stats {
                                stats.health.current_hp -= damage;
                                if stats.health.current_hp <= 0 {
                                    stats.status_effects.effects.insert(StatusEffect::Dying);
                                    killed = true;
                                }
                            }
                        });
                        let msg = match dice::roll(DiceRoll::simple(1, 6)) {
                            ..=1 => format!("{} critically hit {} for a whopping {} damage!", current_actor.name(), target.name(), damage),
                            2 => format!("{} absolutely devastated {} for {} damage!", current_actor.name(), target.name(), damage),
                            3 => format!("{} expertly struck {} for {} damage!", current_actor.name(), target.name(), damage),
                            4 => format!("{} showed {} who\'s boss. It did {} damage!", current_actor.name(), target.name(), damage),
                            5 => format!("{} obliterated {} for a staggering {} damage!", current_actor.name(), target.name(), damage),
                            6.. => format!("{} asked nicely for {} to go away. With force. It did {} damage!", current_actor.name(), target.name(), damage),
                        };
                        data.log_public(msg);
                        if killed {
                            data.log_public(format!("{} was killed!", target.name()));
                        }
                    },
                }
                data.update_combatant(target);
                data.update_combatant(&current_actor);
            },
            CombatAction::RelinquishControl => {
                self.awaiting_response = Some(Owner::DM);
                return;
            },
        }
        self.current_turn += 1;
        self.awaiting_response = None;
    }
}

pub fn damage_roll(data: &mut DMAppData, attacker: &CombatantType, critical: bool) -> i32 {
    data.get_combatant_stats_alt(attacker, |stats| {
        let mut nat = dice::roll(DiceRoll::simple(stats.damage.amount, stats.damage.sides));
        if critical {
            nat *= 2;
        }
        match nat as i32 + stats.modifiers.melee_damage.total() {
            i if i < 1 => 1,
            i => i,
        }
    }).unwrap_or(1)
}

pub fn attack_roll(data: &mut DMAppData, attacker: &CombatantType, target: &CombatantType) -> AttackResult {
    let attack_throw = data.get_combatant_stats_alt(attacker, |s| s.attack_throw + s.modifiers.melee_attack.total()).unwrap_or(10);
    let armor_class = data.get_combatant_stats_alt(target, |s| s.armor_class + s.modifiers.armor_class.total()).unwrap_or(0);

    let r = d20_exploding();
    if r <= 1 {
        return AttackResult::CriticalFail;
    }
    match r + attack_throw - armor_class {
        ..=19 => AttackResult::Fail,
        20..=29 => AttackResult::Success,
        30.. => AttackResult::CriticalSuccess,
    }
}

fn d20_exploding() -> i32 {
    match dice::roll(DiceRoll::simple(1, 20)) {
        20 => 20 + d20_exploding(),
        i => i as i32,
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum AttackResult {
    CriticalFail,
    Fail,
    Success,
    CriticalSuccess,
}

/// Represents who actually gets to make the decisions for this combatant; i.e. who is currently
/// in control of them (PC's are not always controlled by players, for example if they are charmed).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Owner {
    DM,
    Player(String),
}

/// What type of combatant this is. If it's an enemy, stores the enemy name and its ID. If it's a
/// PC, stores the user's username and the character's name.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum CombatantType {
    Enemy(String, u32, String),
    PC(String, String),
}

impl CombatantType {
    /// Gets the formatted display name for this combatant.
    pub fn name(&self) -> String {
        match self {
            Self::Enemy(_, id, name) => {
                if *id == 0 {
                    name.clone()
                } else {
                    format!("{} {}", name, id + 1)
                }
            },
            Self::PC(_, character) => {
                character.clone()
            },
        }
    }

    /// Gets the internal identifier for this combatant.
    pub fn id(&self) -> String {
        match self {
            Self::Enemy(type_id, id, _) => {
                format!("{} {}", type_id, id)
            },
            Self::PC(_, character) => {
                character.clone()
            },
        }
    }

    pub fn not_found() -> Self {
        Self::PC("server".to_owned(), "Nonexistent combatant. If you see this, something has gone wrong.".to_owned())
    }
}

