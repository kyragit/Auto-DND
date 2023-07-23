use std::{collections::{HashMap, hash_map::Iter, HashSet, BTreeSet}, ops::{AddAssign, SubAssign}, cmp::Ordering};

use displaydoc::Display;
use serde::{Serialize, Deserialize};
use simple_enum_macro::simple_enum;

use crate::{character::{Attributes, Health, SavingThrows}, dm_app::DMAppData, packets::ClientBoundPacket, dice::{self, DiceRoll}, enemy::AttackRoutine, common_ui::ChatMessage, spell::MagicType, player_app::{CombatRoundState, CombatState}};

/// All the stats required for something to engage in combat. All of these are *base* stats, before
/// any modifiers! This means `armor_class` will be zero for most characters, unless they have 
/// innate armor! All modifiers are stored in `modifiers`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CombatantStats {
    /// The combatant's base attributes (STR, DEX, etc.)
    pub attributes: Attributes,
    /// The combatant's current and maximum health.
    pub health: Health,
    /// The combatant's base attack throw bonus.
    pub attack_throw: i32,
    /// The combatant's BASE armor class (this is often zero!)
    pub armor_class: i32,
    /// The combatant's current base damage. This will change depending on what weapon they are 
    /// holding.
    pub damage: AttackRoutine,
    pub attack_index: u32,
    /// The combatant's base saving throw modifiers.
    pub saving_throws: SavingThrows,
    /// Any ailments affecting this combatant.
    pub status_effects: StatusEffects,
    /// All of the combatant's stat modifiers, from every source.
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
            damage: AttackRoutine::One(DamageRoll::default()),
            attack_index: 0,
            saving_throws: SavingThrows::new(),
            status_effects: StatusEffects::new(),
            modifiers: StatModifiers::new(),
        }
    }

    pub fn current_damage(&self) -> Option<DamageRoll> {
        match self.damage {
            AttackRoutine::One(d1) => {
                match self.attack_index {
                    0 => Some(d1),
                    _ => None,
                }
            },
            AttackRoutine::Two(d1, d2) => {
                match self.attack_index {
                    0 => Some(d1),
                    1 => Some(d2),
                    _ => None,
                }
            },
            AttackRoutine::Three(d1, d2, d3) => {
                match self.attack_index {
                    0 => Some(d1),
                    1 => Some(d2),
                    2 => Some(d3),
                    _ => None,
                }
            },
        }
    }

    pub fn saving_throw(&self, save: SavingThrowType) -> bool {
        let modifier = match save {
            SavingThrowType::PetrificationParalysis => {
                self.saving_throws.petrification_paralysis + self.modifiers.save_petrification_paralysis.total()
            },
            SavingThrowType::PoisonDeath => {
                self.saving_throws.poison_death + self.modifiers.save_poison_death.total()
            },
            SavingThrowType::BlastBreath => {
                self.saving_throws.blast_breath + self.modifiers.save_blast_breath.total()
            },
            SavingThrowType::StaffsWands => {
                self.saving_throws.staffs_wands + self.modifiers.save_staffs_wands.total()
            },
            SavingThrowType::Spells => {
                self.saving_throws.spells + self.modifiers.save_spells.total()
            },
        };
        let nat = DiceRoll::simple(1, 20).roll();
        nat >= 20 || nat + modifier >= 20
    }

    pub fn hurt(&mut self, damage: u32) -> bool {
        let before = self.health.current_hp;
        self.health.current_hp -= damage as i32;
        let after = self.health.current_hp;
        if before > 0 && after <= 0 {
            self.status_effects.effects.insert(StatusEffect::Dying);
            true
        } else {
            false
        }
    }
}

#[simple_enum(display)]
pub enum SavingThrowType {
    /// Petrification & Paralysis
    PetrificationParalysis,
    /// Poison & Death
    PoisonDeath,
    /// Blast & Breath
    BlastBreath,
    /// Staffs & Wands
    StaffsWands,
    /// Spells
    Spells,
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

    pub fn is_untargetable(&self) -> bool {
        for effect in &self.effects {
            match effect {
                StatusEffect::Dead |
                StatusEffect::Dying => {
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

#[simple_enum(display)]
pub enum StatusEffect {
    /// Dead
    Dead,
    /// Dying
    Dying,
    /// Sleeping
    Sleeping,
    /// Paralyzed
    Paralyzed,
    /// Concentrating
    Concentrating,
}

impl StatusEffect {
    pub fn iterate() -> Vec<StatusEffect> {
        vec![
            Self::Dead,
            Self::Dying,
            Self::Sleeping,
            Self::Paralyzed,
            Self::Concentrating,
        ]
    }
}

#[simple_enum(display)]
pub enum AttackType {
    /// Melee
    Melee,
    /// Missile
    Missile,
}

/// Represents the base damage roll for something.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DamageRoll {
    pub amount: u32,
    pub sides: u32,
    pub modifier: i32,
    pub attack_type: AttackType,
}

impl DamageRoll {
    pub fn new(amount: u32, sides: u32, modifier: i32, attack_type: AttackType) -> Self {
        Self {
            amount,
            sides,
            modifier,
            attack_type,
        }
    }

    pub fn default() -> Self {
        Self {
            amount: 1,
            sides: 2,
            modifier: 0,
            attack_type: AttackType::Melee,
        }
    }

    pub fn melee() -> Self {
        Self {
            amount: 1,
            sides: 2,
            modifier: 0,
            attack_type: AttackType::Melee,
        } 
    }

    pub fn missile() -> Self {
        Self {
            amount: 1,
            sides: 2,
            modifier: 0,
            attack_type: AttackType::Missile,
        } 
    }

    pub fn as_diceroll(&self) -> DiceRoll {
        DiceRoll::simple_modifier(self.amount, self.sides, self.modifier)
    }

    pub fn to_notation(&self) -> String {
        if self.modifier == 0 {
            format!("{}d{}", self.amount, self.sides)
        } else {
            format!("{}d{}{:+}", self.amount, self.sides, self.modifier)
        }
    }

    pub fn roll(&self) -> i32 {
        self.as_diceroll().roll()
    }
}

#[simple_enum(display)]
pub enum StatModType {
    /// Melee Attack
    MeleeAttack,
    /// Missile Attack
    MissileAttack,
    /// Melee Damage
    MeleeDamage,
    /// Missile Damage
    MissileDamage,
    /// Initiative
    Initiative,
    /// Surprise
    Surprise,
    /// Armor Class
    ArmorClass,
    /// Save (Petrification & Paralysis)
    SavePP,
    /// Save (Poison & Death)
    SavePD,
    /// Save (Blast & Breath)
    SaveBB,
    /// Save (Staffs & Wands)
    SaveSW,
    /// Save (Spells)
    SaveSpells,
}

/// Stores ALL active modifiers for every stat, including permanent and temporary modifiers. Each
/// modifier needs a unique key that specifies where it came from (proficiencies, class bonuses, etc).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatModifiers {
    pub melee_attack: StatMod<i32>,
    pub missile_attack: StatMod<i32>,
    pub melee_damage: StatMod<i32>,
    pub missile_damage: StatMod<i32>,
    pub initiative: StatMod<i32>,
    pub surprise: StatMod<i32>,
    pub armor_class: StatMod<i32>,
    pub xp_gain: StatMod<f64>,
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

    pub fn add_all_saves(&mut self, key: impl Into<String> + Clone, value: i32) {
        self.save_petrification_paralysis.add(key.clone(), value);
        self.save_poison_death.add(key.clone(), value);
        self.save_blast_breath.add(key.clone(), value);
        self.save_staffs_wands.add(key.clone(), value);
        self.save_spells.add(key, value);
    }

    pub fn remove_all_saves(&mut self, key: impl Into<String> + Clone) {
        self.save_petrification_paralysis.remove(key.clone());
        self.save_poison_death.remove(key.clone());
        self.save_blast_breath.remove(key.clone());
        self.save_staffs_wands.remove(key.clone());
        self.save_spells.remove(key);
    }

    pub fn get_i32(&mut self, typ: StatModType) -> &mut StatMod<i32> {
        match typ {
            StatModType::MeleeAttack => &mut self.melee_attack,
            StatModType::MissileAttack => &mut self.missile_attack,
            StatModType::MeleeDamage => &mut self.melee_damage,
            StatModType::MissileDamage => &mut self.missile_damage,
            StatModType::Initiative => &mut self.initiative,
            StatModType::Surprise => &mut self.surprise,
            StatModType::ArmorClass => &mut self.armor_class,
            StatModType::SavePP => &mut self.save_petrification_paralysis,
            StatModType::SavePD => &mut self.save_poison_death,
            StatModType::SaveBB => &mut self.save_blast_breath,
            StatModType::SaveSW => &mut self.save_staffs_wands,
            StatModType::SaveSpells => &mut self.save_spells,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatMod<T: AddAssign + SubAssign + Clone + Copy> {
    total: T,
    map: HashMap<String, T>,
    #[serde(skip)]
    temp_id: String,
    #[serde(skip)]
    temp_amount: T,
}

impl<T: AddAssign + SubAssign + Clone + Copy> StatMod<T> {
    pub fn new(initial: T) -> Self {
        Self {
            total: initial,
            map: HashMap::new(),
            temp_id: String::new(),
            temp_amount: initial,
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

    pub fn new_mod_state(&mut self) -> (&mut String, &mut T) {
        (&mut self.temp_id, &mut self.temp_amount)
    }

    pub fn apply_new_mod(&mut self, reset_val: T) {
        self.add(self.temp_id.clone(), self.temp_amount);
        self.temp_id.clear();
        self.temp_amount = reset_val;
    }
}

#[simple_enum(no_copy)]
pub enum TurnType {
    Movement {
        action: MovementAction,
        player_action: Option<MovementAction>,
    },
    Attack {
        action: AttackAction,
        player_action: Option<AttackAction>,
    },
}

impl std::fmt::Display for TurnType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Movement {..} => egui_phosphor::SNEAKER_MOVE,
            Self::Attack {..} => egui_phosphor::SWORD,
        })
    }
}

/// An active fight between combatants.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Fight {
    pub started: bool,
    pub combatants: BTreeSet<(Owner, Combatant)>,
    pub declarations: HashMap<Combatant, PreRoundAction>,
    pub turn_order: Vec<(Owner, Combatant, PreRoundAction)>,
    pub current_turn: Option<(usize, TurnType)>,
}

impl Fight {
    pub fn new() -> Self {
        Self {
            started: false,
            combatants: BTreeSet::new(),
            declarations: HashMap::new(),
            turn_order: Vec::new(),
            current_turn: None,
        }
    }

    pub fn get_current_actor(&self) -> Combatant {
        if let Some((turn, _)) = self.current_turn {
            self.turn_order.get(turn).map_or(Combatant::not_found(), |(_, t, _)| t.clone())
        } else {
            Combatant::not_found()
        }
    }

    pub fn start_round(&mut self, data: &mut DMAppData) {
        let mut list = vec![];
        for (owner, ctype) in &self.combatants {
            if !data.get_combatant_stats_alt(ctype, |c| c.status_effects.is_incapacitated()).unwrap_or(true) {
                let mut r = dice::roll(DiceRoll::simple(1, 6)) as i32;
                r += data.get_combatant_stats_alt(ctype, |c| c.modifiers.initiative.total()).unwrap_or(0);

                list.push((owner.clone(), ctype.clone(), self.declarations.get(ctype).cloned().unwrap_or(PreRoundAction::None), r));
            }
        }
        list.sort_unstable_by(|a, b| {
            if a.3 == b.3 {
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
                a.3.cmp(&b.3)
            }
        });
        self.turn_order = list.into_iter().map(|(o, c, d, _)| (o, c, d)).collect();
        self.turn_order.reverse();
        self.current_turn = Some((0, TurnType::Movement {action: MovementAction::None, player_action: None}));
        self.update_clients(data);
    }

    pub fn update_specific_client(&self, data: &mut DMAppData, user: String) {
        let current_actor = self.get_current_actor();
        let iter: Vec<(&Owner, &Combatant, &PreRoundAction)> = if self.current_turn.is_none() {
            self.combatants.iter().map(|(o, c)| (o, c, self.declarations.get(c).unwrap_or(&PreRoundAction::None))).collect()
        } else {
            self.turn_order.iter().map(|(o, c, p)| (o, c, p)).collect()
        };
        let mut state = CombatState {
            your_combatants: HashMap::new(),
            valid_targets: HashSet::new(),
            round_state: CombatRoundState::NotYourTurn,
        };
        for (owner, combatant, pre_round) in iter {
            if let Owner::Player(player) = owner {
                if *player != user {
                    continue;
                }
                if data.get_combatant_stats_alt(combatant, |s| s.status_effects.is_incapacitated()).unwrap_or(true) {
                    continue;
                }
                state.your_combatants.insert(combatant.clone(), pre_round.clone());
                match &self.current_turn {
                    Some((_, turn_type)) => {
                        if *combatant == current_actor {
                            for (o, t, _) in &self.turn_order {
                                if o == owner {
                                    continue;
                                }
                                if data.get_combatant_stats_alt(t, |c| c.status_effects.is_untargetable()).unwrap_or(true) {
                                    continue;
                                }
                                state.valid_targets.insert(t.clone());
                            }
                            state.round_state = match turn_type {
                                TurnType::Movement {..} => CombatRoundState::MovementAction { combatant: current_actor.clone(), waiting_for_approval: false, temp_action: MovementAction::None },
                                TurnType::Attack {..} => CombatRoundState::AttackAction { combatant: current_actor.clone(), waiting_for_approval: false, temp_action: AttackAction::None },
                            };
                        }
                    },
                    None => {
                        state.round_state = CombatRoundState::PreRound;
                    },
                }
            }
        }
        data.send_to_user(ClientBoundPacket::UpdateCombatState(Some(state)), user);
    }

    pub fn update_clients(&self, data: &mut DMAppData) {
        if self.current_turn.is_none() {
            data.log(ChatMessage::no_sender("Round started!").combat());
        }
        let current_actor = self.get_current_actor();
        let mut map: HashMap<String, (HashMap<Combatant, PreRoundAction>, CombatRoundState, HashSet<Combatant>)> = HashMap::new();

        let iter: Vec<(&Owner, &Combatant, &PreRoundAction)> = if self.current_turn.is_none() {
            self.combatants.iter().map(|(o, c)| (o, c, self.declarations.get(c).unwrap_or(&PreRoundAction::None))).collect()
        } else {
            self.turn_order.iter().map(|(o, c, p)| (o, c, p)).collect()
        };
        for (owner, combatant, pre_round) in iter {
            if let Owner::Player(user) = owner {
                if data.get_combatant_stats_alt(combatant, |s| s.status_effects.is_incapacitated()).unwrap_or(true) {
                    continue;
                }
                let (involved, round_state, targets) = map.entry(user.clone()).or_insert((HashMap::new(), CombatRoundState::NotYourTurn, HashSet::new()));
                involved.insert(combatant.clone(), pre_round.clone());
                match &self.current_turn {
                    Some((_, turn_type)) => {
                        if *combatant == current_actor {
                            for (o, t, _) in &self.turn_order {
                                if o == owner {
                                    continue;
                                }
                                if data.get_combatant_stats_alt(t, |c| c.status_effects.is_untargetable()).unwrap_or(true) {
                                    continue;
                                }
                                targets.insert(t.clone());
                            }
                            *round_state = match turn_type {
                                TurnType::Movement {..} => CombatRoundState::MovementAction { combatant: current_actor.clone(), waiting_for_approval: false, temp_action: MovementAction::None },
                                TurnType::Attack {..} => CombatRoundState::AttackAction { combatant: current_actor.clone(), waiting_for_approval: false, temp_action: AttackAction::None },
                            };
                        }
                    },
                    None => {
                        *round_state = CombatRoundState::PreRound;
                    },
                }
            }
        }
        for (user, (your_combatants, round_state, valid_targets)) in map {
            data.send_to_user(
                ClientBoundPacket::UpdateCombatState(
                    Some(CombatState {
                        your_combatants,
                        valid_targets,
                        round_state,
                    })), 
                user
            );
        }
    }

    pub fn next_turn(&mut self, data: &mut DMAppData) {
        self.turn_order.retain(|(_, c, _)| data.get_combatant_stats_alt(c, |s| !s.status_effects.is_incapacitated()).unwrap_or(false));
        match &mut self.current_turn {
            Some((turn, turn_type)) => {
                match *turn_type {
                    TurnType::Movement {..} => {
                        *turn_type = TurnType::Attack {action: AttackAction::None, player_action: None};
                    },
                    TurnType::Attack {..} => {
                        *turn += 1;
                        if *turn >= self.turn_order.len() {
                            self.current_turn = None;
                            self.declarations.clear();
                        } else {
                            *turn_type = TurnType::Movement {action: MovementAction::None, player_action: None};
                        }
                    },
                }
            },
            None => {},
        }
        self.update_clients(data);
    }

    pub fn resolve_action(&mut self, data: &mut DMAppData) {
        if let Some((turn, turn_type)) = &mut self.current_turn {
            if let Some((_, actor, dec)) = self.turn_order.get(*turn) {
                match turn_type {
                    TurnType::Movement {action, ..} => {
                        match action {
                            MovementAction::None => {
                                self.next_turn(data);
                            },
                            MovementAction::Move => {
                                data.log(ChatMessage::no_sender(format!("{} moves.", actor)).combat());
                                self.next_turn(data);
                            },
                            MovementAction::Run => {
                                data.log(ChatMessage::no_sender(format!("{} runs.", actor)).combat());
                                *turn_type = TurnType::Attack {action: AttackAction::None, player_action: None};
                                self.next_turn(data);
                            },
                            MovementAction::Charge => {
                                data.log(ChatMessage::no_sender(format!("{} charges.", actor)).combat());
                                *turn_type = TurnType::Attack {action: AttackAction::None, player_action: None};
                                self.next_turn(data);
                            },
                            MovementAction::FightingWithdrawal => {
                                data.log(ChatMessage::no_sender(format!("{} makes a fighting withdrawal.", actor)).combat());
                                *turn_type = TurnType::Attack {action: AttackAction::None, player_action: None};
                                self.next_turn(data);
                            },
                            MovementAction::FullRetreat => {
                                data.log(ChatMessage::no_sender(format!("{} makes a full retreat.", actor)).combat());
                                *turn_type = TurnType::Attack {action: AttackAction::None, player_action: None};
                                self.next_turn(data);
                            },
                            MovementAction::SimpleAction => {
                                data.log(ChatMessage::no_sender(format!("{} performs a simple action.", actor)).combat());
                                self.next_turn(data);
                            },
                        }
                    },
                    TurnType::Attack {action, ..} => {
                        match action {
                            AttackAction::None => {
                                self.next_turn(data);
                            },
                            AttackAction::Attack(target, modifier) => {
                                let target = target.clone();
                                let actor = actor.clone();
                                let modifier = modifier.clone();
                                self.make_attack(data, &actor, &target, modifier);
                            },
                            AttackAction::SpecialManeuver(target, maneuver, _modifier) => {
                                data.log(ChatMessage::no_sender(format!("{} tries to {} {}!", actor, maneuver, target)).combat());
                                self.next_turn(data);
                            },
                            AttackAction::CastSpell => {
                                if let PreRoundAction::CastSpell(id, lvl, typ) = dec {
                                    if let Combatant::PC { user, name } = actor {
                                        data.apply_to_pc(user, name, |sheet| {
                                            match typ {
                                                MagicType::Arcane => {
                                                    if let Some(_spells) = &mut sheet.arcane_spells {
                                                        
                                                    }
                                                },
                                                MagicType::Divine => {

                                                },
                                            }
                                        });
                                    }
                                    data.log(ChatMessage::no_sender(format!("{} casts {}!", actor, data.spell_registry.get_spell_name_or_default(id, *lvl, *typ))).combat());
                                } else {
                                    data.log(ChatMessage::no_sender(format!("{} tries to cast a spell that they didn't declare.", actor)).combat().light_red());
                                }
                                self.next_turn(data);
                            },
                            AttackAction::OtherAction => {
                                data.log(ChatMessage::no_sender(format!("{} performs a simple action.", actor)).combat());
                                self.next_turn(data);
                            },
                        }
                    },
                }
            } else {
                self.next_turn(data);
            }
        }
    }

    pub fn make_attack(&mut self, data: &mut DMAppData, attacker: &Combatant, target: &Combatant, modifier: i32) {
        match attack_roll(data, attacker, target, modifier) {
            AttackResult::CriticalFail => {
                let msg = match dice::roll(DiceRoll::simple(1, 6)) {
                    ..=1 => format!("{} failed miserably when attacking {}!", attacker, target),
                    2 => format!("{} critically missed {}!", attacker, target),
                    3 => format!("{} utterly whiffed an attempt to hit {}!", attacker, target),
                    4 => format!("{} absolutely annihilated the air nearby to {}.", attacker, target),
                    5 => format!("Whatever {} tried to do to {}, it didn\'t work very well.", attacker, target),
                    6.. => format!("{} lands a devastating warning blow toward {}! It did absolutely nothing.", attacker, target),
                };
                data.log(ChatMessage::no_sender(msg).combat().dice_roll());
            },
            AttackResult::Fail => {
                data.log(ChatMessage::no_sender(format!("{} missed {}!", attacker, target)).combat().dice_roll());
            },
            AttackResult::Success => {
                let damage = damage_roll(data, attacker, false);
                let mut killed = false;
                data.get_combatant_stats(target, |stats| {
                    if let Some(stats) = stats {
                        killed = stats.hurt(damage as u32);
                    }
                });
                data.log(ChatMessage::no_sender(format!("{} hit {} for {} damage!", attacker, target, damage)).combat().dice_roll());
                if killed {
                    data.log(ChatMessage::no_sender(format!("{} was killed!", target)).combat().red());
                }
            },
            AttackResult::CriticalSuccess => {
                let damage = damage_roll(data, attacker, true);
                let mut killed = false;
                data.get_combatant_stats(target, |stats| {
                    if let Some(stats) = stats {
                        killed = stats.hurt(damage as u32);
                    }
                });
                let msg = match dice::roll(DiceRoll::simple(1, 6)) {
                    ..=1 => format!("{} critically hit {} for a whopping {} damage!", attacker, target, damage),
                    2 => format!("{} absolutely devastated {} for {} damage!", attacker, target, damage),
                    3 => format!("{} expertly struck {} for {} damage!", attacker, target, damage),
                    4 => format!("{} showed {} who\'s boss. It did {} damage!", attacker, target, damage),
                    5 => format!("{} obliterated {} for a staggering {} damage!", attacker, target, damage),
                    6.. => format!("{} asked nicely for {} to go away. With force. It did {} damage!", attacker, target, damage),
                };
                data.log(ChatMessage::no_sender(msg).combat().dice_roll());
                if killed {
                    data.log(ChatMessage::no_sender(format!("{} was killed!", target)).combat().red());
                }
            },
        }
        if data.get_combatant_stats(attacker, |stats| {
            if let Some(stats) = stats {
                stats.attack_index += 1;
                if stats.current_damage().is_none() {
                    stats.attack_index = 0;
                    true
                } else {
                    false
                }
            } else {
                true
            }
        }) {
            self.next_turn(data);
        }
        data.update_combatant(target);
        data.update_combatant(attacker);
    }
}

pub fn damage_roll(data: &mut DMAppData, attacker: &Combatant, critical: bool) -> i32 {
    data.get_combatant_stats_alt(attacker, |stats| {
        let damage = stats.current_damage().unwrap_or(DamageRoll::default());
        let mut nat = damage.roll();
        if critical {
            nat *= 2;
        }
        match damage.attack_type {
            AttackType::Melee => {
                match nat + stats.modifiers.melee_damage.total() {
                    i if i < 1 => 1,
                    i => i,
                }
            },
            AttackType::Missile => {
                match nat + stats.modifiers.missile_damage.total() {
                    i if i < 1 => 1,
                    i => i,
                }
            },
        }
    }).unwrap_or(1)
}

pub fn attack_roll(data: &mut DMAppData, attacker: &Combatant, target: &Combatant, modifiers: i32) -> AttackResult {
    let attack_throw = data.get_combatant_stats_alt(attacker, |s| {
        match s.current_damage().unwrap_or(DamageRoll::default()).attack_type {
            AttackType::Melee => {
                s.attack_throw + s.modifiers.melee_attack.total()
            },
            AttackType::Missile => {
                s.attack_throw + s.modifiers.missile_attack.total()
            },
        }
    }).unwrap_or(10);
    let armor_class = data.get_combatant_stats_alt(target, |s| s.armor_class + s.modifiers.armor_class.total()).unwrap_or(0);
    let r = d20_exploding();
    if r <= 1 {
        return AttackResult::CriticalFail;
    }
    match r + attack_throw - armor_class + modifiers {
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
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Owner {
    DM,
    Player(String),
}

/// A combatant, either a player character or enemy. 
/// 
/// ### Fields
/// - `Enemy.0`: The enemy type registry ID.
/// - `Enemy.1`: The numerical index of this enemy, for when there are more than one of the same type.
/// - `Enemy.2`: The enemy type name, so it doesn't have to be looked up constantly.
/// - `PC.0`: The player username.
/// - `PC.1`: The player character name.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Display, PartialOrd, Ord)]
pub enum Combatant {
    /// {display_name}
    Enemy {
        room: String,
        type_id: String,
        index: usize,
        display_name: String,
    },
    /// {name}
    PC {
        user: String,
        name: String,
    },
}

impl Combatant {
    /// A combatant that doesn't exist, in case of an error.
    pub fn not_found() -> Self {
        Self::pc("server".to_owned(), "Nonexistent combatant. If you see this, something has gone wrong.".to_owned())
    }

    pub fn enemy(room: String, type_id: String, index: usize, display_name: String) -> Self {
        Self::Enemy { room, type_id, index, display_name }
    }

    pub fn enemy_auto_name(room: String, type_id: String, index: usize, type_name: String) -> Self {
        Self::enemy(room, type_id, index, if index == 0 {type_name} else {format!("{} {}", type_name, index + 1)})
    }

    pub fn pc(user: String, name: String) -> Self {
        Self::PC { user, name }
    }
}

#[simple_enum(no_copy, display)]
pub enum PreRoundAction {
    /// None
    None,
    /// Fighting Withdrawal
    FightingWithdrawal,
    /// Full Retreat
    FullRetreat,
    /// Cast Spell
    CastSpell(String, u8, MagicType),
}

#[simple_enum(display)]
pub enum MovementAction {
    /// None
    None,
    /// Move
    Move,
    /// Run
    Run,
    /// Charge
    Charge,
    /// Fighting Withdrawal
    FightingWithdrawal,
    /// Full Retreat
    FullRetreat,
    /// Simple Action
    SimpleAction,
}

#[simple_enum(no_copy, display)]
pub enum AttackAction {
    /// None
    None,
    /// Attack
    Attack(Combatant, i32),
    /// Special Maneuver ({1})
    SpecialManeuver(Combatant, SpecialManeuver, i32),
    /// Cast Spell
    CastSpell,
    /// Other Action
    OtherAction,
}

impl AttackAction {
    pub fn display_alt(&self) -> String {
        match self {
            Self::Attack(target, _) => {
                format!("Attack {}", target)
            },
            Self::SpecialManeuver(target, maneuver, _) => {
                format!("{} {}", maneuver, target)
            },
            _ => format!("{}", self)
        }
    }
}

#[simple_enum(display)]
pub enum SpecialManeuver {
    /// Disarm
    Disarm,
    /// Force Back
    ForceBack,
    /// Incapacitate
    Incapacitate,
    /// Knock Down
    KnockDown,
    /// Sunder
    Sunder,
    /// Wrestle
    Wrestle,
}

