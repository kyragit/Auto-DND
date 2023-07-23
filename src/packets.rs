use std::collections::HashMap;
use std::net::SocketAddr;

use egui::Color32;
use serde::{Serialize, Deserialize};
use simple_enum_macro::simple_enum;
use crate::character::{PlayerCharacter, PlayerEquipSlot};
use crate::class::Class;
use crate::combat::{Combatant, SavingThrowType, PreRoundAction, MovementAction, AttackAction, Owner, TurnType};
use crate::common_ui::ChatMessage;
use crate::dm_app::{DMAppData, UserData, Registry};
use crate::party::Party;
use crate::player_app::{PlayerAppData, CombatState};
use crate::proficiency::{Proficiency, ProficiencyInstance};
use crate::spell::SpellRegistry;

/// A packet sent from the server to a client.
#[derive(Debug, Serialize, Deserialize)]
pub enum ClientBoundPacket {
    /// Logs a message in the chat. Note that this may be called when a user sends a chat message,
    /// the server sends a chat message, or any time a message is sent to a client.
    ChatMessage(ChatMessage),
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
    /// Sent to give the client a clone of the class registry.
    UpdateClassRegistry(Registry<Class>),
    /// Sent to give the client a clone of the proficiency registry.
    UpdateProfRegistry(HashMap<String, Proficiency>),
    /// Sent to give the client a clone of the spell registry.
    UpdateSpellRegistry(SpellRegistry),
    RespondToRequest(Request, bool),
    UpdateCombatState(Option<CombatState>),
    UpdateParties(HashMap<String, Party>),
}

impl ClientBoundPacket {
    pub fn handle(self, data: &mut PlayerAppData) {
        match self {
            Self::ChatMessage(msg) => {
                data.logs.insert(0, msg.to_layout_job());
                if data.unread_messages == 0 {
                    data.unread_msg_buffer = true;
                }
                data.unread_messages += 1;
            },
            Self::LogInResult(success) => {
                data.logged_in = success;
            },
            Self::CreateAccountResult(success, username, password) => {
                if success {
                    data.username = username.clone();
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
            Self::UpdateSpellRegistry(spells) => {
                data.spell_registry = spells;
            },
            Self::RespondToRequest(request, approved) => {
                data.requests.set_approval(request, approved);
            },
            Self::UpdateCombatState(state) => {
                if state.is_some() && data.combat_state.is_none() {
                    data.combat_just_started = true;
                }
                data.combat_state = state;
            },
            Self::UpdateParties(parties) => {
                data.parties = parties;
            },
        }
    }
}

/// A packet sent to the server.
#[derive(Debug, Serialize, Deserialize)]
pub enum ServerBoundPacket {
    /// Sent when a user sends a message in chat.
    ChatMessage(ChatMessage),
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
    /// Sent when a player tries to rearrange their inventory.
    MoveInventoryItem(String, usize, bool),
    /// Sent when a player tries to equip an item.
    EquipInventoryItem(String, PlayerEquipSlot, usize),
    /// Sent when a player tries to unequip an item.
    UnequipInventoryItem(String, PlayerEquipSlot),
    /// Sent when a saving throw is made by a PC.
    SavingThrow(String, SavingThrowType),
    /// Sent when a player selects a new proficiency.
    PickNewProficiency(String, bool, String, Option<String>),
    MakeRequest(Request),
    MakePreRoundDeclaration(Combatant, PreRoundAction),
    DecideMovementAction(MovementAction),
    DecideAttackAction(AttackAction),
}

impl ServerBoundPacket {
    pub fn handle(self, data: &mut DMAppData, user: SocketAddr) {
        match self {
            Self::ChatMessage(msg) => {
                data.log(msg);
                if data.temp_state.unread_messages == 0 {
                    data.temp_state.unread_msg_buffer = true;
                }
                data.temp_state.unread_messages += 1;
            },
            Self::AttemptLogIn(username, password) => {
                if let Some(pw) = data.known_users.get(&username) {
                    if pw == &password {
                        data.log(ChatMessage::no_sender(format!("User \"{}\" has logged in!", &username)).blue());
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
                        data.send_to_user_by_addr(ClientBoundPacket::UpdateSpellRegistry(data.spell_registry.clone()), user);
                        data.send_to_user_by_addr(ClientBoundPacket::UpdateParties(data.parties.clone()), user);
                        return;
                    }
                }
                data.send_to_user_by_addr(ClientBoundPacket::LogInResult(false), user);
            },
            Self::CreateAccount(mut username, password) => {
                username = username.trim().to_owned();
                if data.known_users.contains_key(&username) || username == "server" || username.is_empty() {
                    data.send_to_user_by_addr(ClientBoundPacket::CreateAccountResult(false, username, password), user);
                } else {
                    data.log(ChatMessage::no_sender(format!("User \"{}\" has been created.", &username)).private().blue());
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
                            character.add_prof("adventuring", ProficiencyInstance::from_prof(prof.clone(), None));
                        }
                        if let Some(arcane) = &mut character.arcane_spells {
                            if let Some(spell) = data.spell_registry.random_arcane(0) {
                                arcane.spell_repertoire[0].0.insert(spell);
                            }
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
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some(user_data) = data.user_data.get_mut(&username) {
                        if let Some(sheet) = user_data.characters.get_mut(&name) {
                            sheet.equip_item(slot, index);
                            let sheet = sheet.clone();
                            data.send_to_user(ClientBoundPacket::UpdateCharacter(name, sheet), username);
                        }
                    }
                }
            },
            Self::UnequipInventoryItem(name, slot) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some(user_data) = data.user_data.get_mut(&username) {
                        if let Some(sheet) = user_data.characters.get_mut(&name) {
                            sheet.unequip_item(slot);
                            let sheet = sheet.clone();
                            data.send_to_user(ClientBoundPacket::UpdateCharacter(name, sheet), username);
                        }
                    }
                }
            },
            Self::SavingThrow(name, save) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some(user_data) = data.user_data.get_mut(&username) {
                        if let Some(sheet) = user_data.characters.get_mut(&name) {
                            if sheet.combat_stats.saving_throw(save) {
                                data.log(ChatMessage::no_sender(format!("{} successfully made a saving throw against {}!", name, save)).dice_roll().color(Color32::LIGHT_GREEN));
                            } else {
                                data.log(ChatMessage::no_sender(format!("{} failed a saving throw against {}!", name, save)).dice_roll().color(Color32::LIGHT_RED));
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
                                        let mut p = ProficiencyInstance::from_prof(prof.clone(), None);
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
                                        let mut p = ProficiencyInstance::from_prof(prof.clone(), None);
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
            Self::MakeRequest(request) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    data.temp_state.requests.push((username, request));
                }
            },
            Self::MakePreRoundDeclaration(combatant, action) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    if let Some((_, map)) = &mut data.loaded_map {
                        if let Some(fight) = &mut map.fight {
                            if fight.started && fight.current_turn.is_none() {
                                if fight.combatants.contains(&(Owner::Player(username.clone()), combatant.clone())) {
                                    if action == PreRoundAction::None {
                                        fight.declarations.remove(&combatant);
                                    } else {
                                        fight.declarations.insert(combatant.clone(), action);
                                    }
                                    // this is annoying
                                    fight.clone().update_specific_client(data, username);
                                }
                            }
                        }
                    }
                }
            },
            Self::DecideMovementAction(action) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    data.get_fight(|fight| {
                        if let Some((turn, turn_type)) = &mut fight.current_turn {
                            if let Some((owner, _, _)) = fight.turn_order.get(*turn) {
                                if *owner == Owner::Player(username) {
                                    if let TurnType::Movement {player_action, ..} = turn_type {
                                        *player_action = Some(action);
                                    }
                                }
                            }
                        }
                    });
                }
            },
            Self::DecideAttackAction(action) => {
                if let Some(username) = data.get_username_by_addr(user) {
                    data.get_fight(|fight| {
                        if let Some((turn, turn_type)) = &mut fight.current_turn {
                            if let Some((owner, _, _)) = fight.turn_order.get(*turn) {
                                if *owner == Owner::Player(username) {
                                    if let TurnType::Attack {player_action, ..} = turn_type {
                                        *player_action = Some(action);
                                    }
                                }
                            }
                        }
                    });
                }
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum CombatAction {
    Attack(Combatant),
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

#[simple_enum(display)]
pub enum Request {
    /// Generate a new set of characters.
    GenerateCharacters,
}