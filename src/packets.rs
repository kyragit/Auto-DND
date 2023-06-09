use std::collections::HashMap;
use std::net::SocketAddr;

use serde::{Serialize, Deserialize};
use simple_enum_macro::simple_enum;
use crate::character::{PlayerCharacter, PlayerEquipSlot};
use crate::class::Class;
use crate::combat::{CombatantType, DamageRoll, SavingThrowType};
use crate::dm_app::{DMAppData, UserData, Registry};
use crate::enemy::AttackRoutine;
use crate::item::{WeaponDamage, MeleeDamage};
use crate::player_app::PlayerAppData;
use crate::proficiency::{Proficiency, ProficiencyInstance};

/// A packet sent from the server to a client.
#[derive(Debug, Serialize, Deserialize)]
pub enum ClientBoundPacket {
    /// Logs a message in the chat. Note that this may be called when a user sends a chat message,
    /// the server sends a chat message, or any time a message is sent to a client.
    ChatMessage(String),
    /// The result of a log in attempt.
    LogInResult(bool),
    /// The result of an attempt to create an account.
    CreateAccountResult(bool, String, String),
    /// The result of creating a new character.
    CreateNewCharacterResult(Result<(), ClientFacingError>, String),
    /// Sent when a client's character is updated for whatever reason.
    UpdateCharacter(String, PlayerCharacter),
    /// Sent when the player recieves their personal notes from the server upon login.
    UpdatePlayerNotes(String),
    /// Sent to notify a player that they must make a combat action.
    DecideCombatAction(CombatantType, Vec<CombatantType>),
    UpdateClassRegistry(Registry<Class>),
    UpdateProfRegistry(HashMap<String, Proficiency>),
}

impl ClientBoundPacket {
    pub fn handle(self, data: &mut PlayerAppData) {
        match self {
            Self::ChatMessage(msg) => {
                data.logs.insert(0, msg);
            },
            Self::LogInResult(success) => {
                data.logged_in = success;
            },
            Self::CreateAccountResult(success, username, password) => {
                if success {
                    data.send_to_server(ServerBoundPacket::AttemptLogIn(username, password));
                }
            },
            Self::CreateNewCharacterResult(success, name) => {
                if let Err(e) = success {
                    data.new_char_name_error = Some(e);
                } else {
                    if let Some(i) = data.picked_character {
                        if i < data.new_characters.len() {
                            data.new_characters.remove(i);
                        }
                    }
                    data.picked_character = None;
                    data.new_char_name = None;
                    data.new_char_name_error = None;
                    data.send_to_server(ServerBoundPacket::RequestCharacterUpdate(name, None));
                }
            },
            Self::UpdateCharacter(name, mut character) => {
                if let Some(old_char) = data.characters.get(&name) {
                    character.notes = old_char.notes.clone();
                }
                data.characters.insert(name, character);
            },
            Self::UpdatePlayerNotes(notes) => {
                data.notes = notes;
            },
            Self::DecideCombatAction(this, combatants) => {
                data.character_awaiting_action = Some(this);
                data.combatant_list = combatants;
                data.window_states.insert("combat_action".to_owned(), true);
            },
            Self::UpdateClassRegistry(registry) => {
                data.class_registry = registry;
            },
            Self::UpdateProfRegistry(profs) => {
                for (id, prof) in  &profs {
                    data.sorted_prof_list.push((id.clone(), prof.name.clone()));
                }
                data.sorted_prof_list.sort();
                data.proficiency_registry = profs;
            },
        }
    }
}

/// A packet sent to the server.
#[derive(Debug, Serialize, Deserialize)]
pub enum ServerBoundPacket {
    /// Sent when a user sends a message in chat.
    ChatMessage(String),
    /// Sent when a user tries to log in.
    AttemptLogIn(String, String),
    /// Sent when a user creates a new account.
    CreateAccount(String, String),
    /// Sent when a user generates a new character.
    CreateNewCharacter(String, PlayerCharacter),
    /// Sent to update a user's character sheet with data from the server.
    RequestCharacterUpdate(String, Option<PlayerCharacter>),
    /// Sent when the player's personal notes change.
    UpdatePlayerNotes(String),
    /// Sent when a player has decided on a combat action.
    DecideCombatAction(CombatAction),
    /// Sent when a player tries to rearrange their inventory.
    MoveInventoryItem(String, usize, bool),
    /// Sent when a player tries to equip an item.
    EquipInventoryItem(String, PlayerEquipSlot, usize),
    /// Sent when a player tries to unequip an item.
    UnequipInventoryItem(String, PlayerEquipSlot),
    SavingThrow(String, SavingThrowType),
    PickNewProficiency(String, bool, String, Option<String>),
}

impl ServerBoundPacket {
    pub fn handle(self, data: &mut DMAppData, user: SocketAddr) {
        match self {
            Self::ChatMessage(mut msg) => {
                let mut name = String::from("error");
                for (username, addr) in &data.connected_users {
                    if *addr == user {
                        name = username.clone();
                        break;
                    }
                }
                msg.insert_str(0, format!("[{}]: ", name).as_str());
                data.log_public(msg);
            },
            Self::AttemptLogIn(username, password) => {
                if let Some(pw) = data.known_users.get(&username) {
                    if pw == &password {
                        data.log_public(format!("User \"{}\" has logged in!", &username));
                        data.connected_users.insert(username.clone(), user);
                        if !data.user_data.contains_key(&username) {
                            data.user_data.insert(username.clone(), UserData::new());
                        }
                        data.send_to_user_by_addr(ClientBoundPacket::LogInResult(true), user);
                        if let Some(user_data) = data.user_data.get(&username) {
                            data.send_to_user_by_addr(ClientBoundPacket::UpdatePlayerNotes(user_data.notes.clone()), user);
                        }
                        if let Some(user_data) = data.user_data.get(&username) {
                            for (name, character) in user_data.characters.clone() {
                                data.send_to_user_by_addr(ClientBoundPacket::UpdateCharacter(name, character), user);
                            }
                        }
                        data.send_to_user_by_addr(ClientBoundPacket::UpdateClassRegistry(data.class_registry.clone()), user);
                        data.send_to_user_by_addr(ClientBoundPacket::UpdateProfRegistry(data.proficiency_registry.clone()), user);
                        return;
                    }
                }
                data.send_to_user_by_addr(ClientBoundPacket::LogInResult(false), user);
            },
            Self::CreateAccount(username, password) => {
                if data.known_users.contains_key(&username) || username == "server" || username.trim().is_empty() {
                    data.send_to_user_by_addr(ClientBoundPacket::CreateAccountResult(false, username, password), user);
                } else {
                    data.log_private(format!("User \"{}\" has been created.", &username));
                    data.known_users.insert(username.clone(), password.clone());
                    data.send_to_user_by_addr(ClientBoundPacket::CreateAccountResult(true, username, password), user);
                }
            },
            Self::CreateNewCharacter(name, mut character) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some(user_data) = data.user_data.get_mut(&username) {
                        if user_data.characters.contains_key(&name) {
                            data.send_to_user(ClientBoundPacket::CreateNewCharacterResult(Err(ClientFacingError::CharacterNameTaken), name), username);
                            return;
                        }
                        if name.trim().is_empty() {
                            data.send_to_user(ClientBoundPacket::CreateNewCharacterResult(Err(ClientFacingError::CharacterNameInvalid), name), username);
                            return;
                        }
                        if name.len() > 40 {
                            data.send_to_user(ClientBoundPacket::CreateNewCharacterResult(Err(ClientFacingError::CharacterNameTooLong), name), username);
                            return;
                        }
                        character.initialize();
                        if let Some(prof) = data.proficiency_registry.get("adventuring") {
                            character.add_prof("adventuring", ProficiencyInstance::from_prof(prof.clone()));
                        }
                        user_data.characters.insert(name.clone(), character);
                        data.send_to_user(ClientBoundPacket::CreateNewCharacterResult(Ok(()), name), username);
                    }
                    
                }
            },
            Self::RequestCharacterUpdate(name, maybe_char) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some(user_data) = data.user_data.get_mut(&username) {
                        if let Some(sheet) = user_data.characters.get_mut(&name) {
                            if let Some(user_sheet) = maybe_char {
                                sheet.notes = user_sheet.notes;
                                let sheet = sheet.clone();
                                data.send_to_user(ClientBoundPacket::UpdateCharacter(name, sheet), username);
                            } else {
                                let sheet = sheet.clone();
                                data.send_to_user(ClientBoundPacket::UpdateCharacter(name, sheet), username);
                            } 
                        }
                    }
                }
            },
            Self::UpdatePlayerNotes(notes) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some(user_data) = data.user_data.get_mut(&username) {
                        user_data.notes = notes;
                    }
                }
            },
            Self::DecideCombatAction(action) => {
                data.fight = data.fight.take().map(|mut f| {f.resolve_action(data, action); f});
            },
            Self::MoveInventoryItem(name, index, up) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some(user_data) = data.user_data.get_mut(&username) {
                        if let Some(sheet) = user_data.characters.get_mut(&name) {
                            if up {
                                sheet.inventory.move_up(index);
                            } else {
                                sheet.inventory.move_down(index);
                            }
                            let sheet = sheet.clone();
                            data.send_to_user(ClientBoundPacket::UpdateCharacter(name, sheet), username);
                        }
                    }
                }
            },
            Self::EquipInventoryItem(name, slot, index) => {
                if slot == PlayerEquipSlot::LeftHand || slot == PlayerEquipSlot::BothHands {
                    ServerBoundPacket::UnequipInventoryItem(name.clone(), PlayerEquipSlot::LeftHand).handle(data, user);
                }
                if slot == PlayerEquipSlot::RightHand || slot == PlayerEquipSlot::BothHands {
                    ServerBoundPacket::UnequipInventoryItem(name.clone(), PlayerEquipSlot::RightHand).handle(data, user);
                }
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some(user_data) = data.user_data.get_mut(&username) {
                        if let Some(sheet) = user_data.characters.get_mut(&name) {
                            sheet.inventory.equip(slot, index);
                            if let Some(item) = sheet.inventory.get_equip_slot(slot) {
                                if let Some(armor) = item.item_type.armor_stats {
                                    if slot == PlayerEquipSlot::Armor {
                                        sheet.combat_stats.modifiers.armor_class.add("armor", armor as i32);
                                    }
                                }
                                if let Some(shield) = item.item_type.shield_stats {
                                    if slot == PlayerEquipSlot::LeftHand {
                                        sheet.combat_stats.modifiers.armor_class.add("shield", shield as i32);
                                    }
                                }
                                if let Some(weapon) = &item.item_type.weapon_stats {
                                    match &weapon.damage {
                                        WeaponDamage::Melee(melee) => {
                                            match melee {
                                                MeleeDamage::OneHanded(dmg) => {
                                                    match slot {
                                                        PlayerEquipSlot::LeftHand => {
                                                            sheet.combat_stats.modifiers.melee_attack.add("dual_wielding", 1);
                                                        },
                                                        PlayerEquipSlot::RightHand => {
                                                            sheet.combat_stats.damage = AttackRoutine::One(dmg.clone());
                                                        },
                                                        _ => {},
                                                    }
                                                },
                                                MeleeDamage::Versatile(dmg1, dmg2) => {
                                                    match slot {
                                                        PlayerEquipSlot::LeftHand => {
                                                            sheet.combat_stats.modifiers.melee_attack.add("dual_wielding", 1);
                                                        },
                                                        PlayerEquipSlot::RightHand => {
                                                            sheet.combat_stats.damage = AttackRoutine::One(dmg1.clone());
                                                        },
                                                        PlayerEquipSlot::BothHands => {
                                                            sheet.combat_stats.damage = AttackRoutine::One(dmg2.clone());
                                                        },
                                                        _ => {},
                                                    }
                                                },
                                                MeleeDamage::TwoHanded(dmg) => {
                                                    match slot {
                                                        PlayerEquipSlot::BothHands => {
                                                            sheet.combat_stats.damage = AttackRoutine::One(dmg.clone());
                                                            sheet.combat_stats.modifiers.initiative.add("heavy_weapon", -1);
                                                        },
                                                        _ => {},
                                                    }
                                                },
                                            }
                                        },
                                        WeaponDamage::Missile(dmg, _) => {
                                            match slot {
                                                PlayerEquipSlot::BothHands => {
                                                    sheet.combat_stats.damage = AttackRoutine::One(dmg.clone());
                                                },
                                                _ => {},
                                            }
                                        },
                                    }
                                }
                            }
                            let sheet = sheet.clone();
                            data.send_to_user(ClientBoundPacket::UpdateCharacter(name, sheet), username);
                        }
                    }
                }
            },
            Self::UnequipInventoryItem(name, mut slot) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some(user_data) = data.user_data.get_mut(&username) {
                        if let Some(sheet) = user_data.characters.get_mut(&name) {
                            if slot == PlayerEquipSlot::LeftHand || slot == PlayerEquipSlot::RightHand {
                                if let Some(left) = sheet.inventory.left_hand {
                                    if let Some(right) = sheet.inventory.right_hand {
                                        if left == right {
                                            slot = PlayerEquipSlot::BothHands;
                                        }
                                    }
                                }
                            }
                            if let Some(item) = sheet.inventory.get_equip_slot(slot) {
                                if let Some(_) = item.item_type.armor_stats {
                                    if slot == PlayerEquipSlot::Armor {
                                        sheet.combat_stats.modifiers.armor_class.remove("armor");
                                    }
                                }
                                if let Some(_) = item.item_type.shield_stats {
                                    if slot == PlayerEquipSlot::LeftHand {
                                        sheet.combat_stats.modifiers.armor_class.remove("shield");
                                    }
                                }
                                if let Some(weapon) = &item.item_type.weapon_stats {
                                    match &weapon.damage {
                                        WeaponDamage::Melee(melee) => {
                                            match melee {
                                                MeleeDamage::OneHanded(_) => {
                                                    match slot {
                                                        PlayerEquipSlot::LeftHand => {
                                                            sheet.combat_stats.modifiers.melee_attack.remove("dual_wielding");
                                                        },
                                                        PlayerEquipSlot::RightHand => {
                                                            sheet.combat_stats.damage = AttackRoutine::One(DamageRoll::default());
                                                        },
                                                        _ => {},
                                                    }
                                                },
                                                MeleeDamage::Versatile(_, _) => {
                                                    match slot {
                                                        PlayerEquipSlot::LeftHand => {
                                                            sheet.combat_stats.modifiers.melee_attack.remove("dual_wielding");
                                                        },
                                                        PlayerEquipSlot::RightHand | PlayerEquipSlot::BothHands => {
                                                            sheet.combat_stats.damage = AttackRoutine::One(DamageRoll::default());
                                                        },
                                                        _ => {},
                                                    }
                                                },
                                                MeleeDamage::TwoHanded(_) => {
                                                    if slot == PlayerEquipSlot::BothHands {
                                                        sheet.combat_stats.damage = AttackRoutine::One(DamageRoll::default());
                                                        sheet.combat_stats.modifiers.initiative.remove("heavy_weapon");
                                                    }
                                                },
                                            }
                                        },
                                        WeaponDamage::Missile(_, _) => {
                                            if slot == PlayerEquipSlot::BothHands {
                                                sheet.combat_stats.damage = AttackRoutine::One(DamageRoll::default());
                                            }
                                        },
                                    }
                                }
                                sheet.inventory.unequip(slot);
                                let sheet = sheet.clone();
                                data.send_to_user(ClientBoundPacket::UpdateCharacter(name, sheet), username);
                            }
                        }
                    }
                }
            },
            Self::SavingThrow(name, save) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some(user_data) = data.user_data.get_mut(&username) {
                        if let Some(sheet) = user_data.characters.get_mut(&name) {
                            if sheet.combat_stats.saving_throw(save) {
                                data.log_public(format!("{} successfully made a saving throw against {}!", name, save));
                            } else {
                                data.log_public(format!("{} failed a saving throw against {}!", name, save));
                            }
                        }
                    }
                }
            },
            Self::PickNewProficiency(name, general, id, spec) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some(user_data) = data.user_data.get_mut(&username) {
                        if let Some(sheet) = user_data.characters.get_mut(&name) {
                            if let Some(prof) = data.proficiency_registry.get(&id) {
                                if general && prof.is_general && sheet.proficiencies.general_slots > 0 {
                                    if let Some(p) = sheet.proficiencies.profs.get(&(id.clone(), spec.clone())) {
                                        if p.prof_level < prof.max_level {
                                            let mut p = p.clone();
                                            p.prof_level += 1;
                                            sheet.remove_prof(&(id.clone(), spec));
                                            sheet.add_prof(&id, p);
                                            sheet.proficiencies.general_slots -= 1;
                                        }
                                    } else {
                                        let mut p = ProficiencyInstance::from_prof(prof.clone());
                                        if prof.requires_specification {
                                            if let Some(valid) = &prof.valid_specifications {
                                                if valid.contains(spec.as_ref().unwrap_or(&"@&%:".to_owned())) {
                                                    p.specification = spec;
                                                } else {
                                                    return;
                                                }
                                            } else if spec.is_some() {
                                                p.specification = spec;
                                            } else {
                                                return;
                                            }
                                        }
                                        sheet.add_prof(&id, p);
                                        sheet.proficiencies.general_slots -= 1;
                                    }
                                } else if !general && sheet.class.class_proficiencies.contains_key(&id) && sheet.proficiencies.class_slots > 0 {
                                    if let Some(p) = sheet.proficiencies.profs.get(&(id.clone(), spec.clone())) {
                                        if p.prof_level < prof.max_level {
                                            let mut p = p.clone();
                                            p.prof_level += 1;
                                            sheet.remove_prof(&(id.clone(), spec.clone()));
                                            sheet.add_prof(&id, p);
                                            sheet.proficiencies.class_slots -= 1;
                                        }
                                    } else {
                                        let mut p = ProficiencyInstance::from_prof(prof.clone());
                                        if prof.requires_specification {
                                            if let Some(valid) = &prof.valid_specifications {
                                                if valid.contains(spec.as_ref().unwrap_or(&"@&%:".to_owned())) {
                                                    if let Some(class_valid) = sheet.class.class_proficiencies.get(&id).unwrap() {
                                                        if class_valid.contains(spec.as_ref().unwrap_or(&"@&%:".to_owned())) {
                                                            p.specification = spec;
                                                        } else {
                                                            return;
                                                        }
                                                    } else {
                                                        p.specification = spec;
                                                    }
                                                } else {
                                                    return;
                                                }
                                            } else if spec.is_some() {
                                                p.specification = spec;
                                            } else {
                                                return;
                                            }
                                        }
                                        sheet.add_prof(&id, p);
                                        sheet.proficiencies.class_slots -= 1;
                                    }
                                }
                                let sheet = sheet.clone();
                                data.send_to_user(ClientBoundPacket::UpdateCharacter(name, sheet), username);
                            }
                        }
                    }
                }
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum CombatAction {
    Attack(CombatantType),
    RelinquishControl,
}

#[simple_enum]
pub enum ClientFacingError {
    UsernameTaken,
    UsernameTooLong,
    UsernameInvalid,
    CharacterNameTaken,
    CharacterNameTooLong,
    CharacterNameInvalid,
    Generic,
}

impl std::fmt::Display for ClientFacingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Generic => "Something went wrong.",
            Self::UsernameTaken => "There is already an account with that username.",
            Self::UsernameTooLong => "That username is too long. Pick something shorter.",
            Self::UsernameInvalid => "That username is disallowed.",
            Self::CharacterNameTaken => "You already have a character with that name. Pick something else.",
            Self::CharacterNameTooLong => "That name is too long. Pick something shorter.",
            Self::CharacterNameInvalid => "That name is disallowed.",
        })
    }
}