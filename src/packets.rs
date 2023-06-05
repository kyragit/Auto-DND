use std::net::SocketAddr;

use serde::{Serialize, Deserialize};
use crate::character::PlayerCharacter;
use crate::class::Class;
use crate::combat::CombatantType;
use crate::dm_app::{DMAppData, UserData};
use crate::player_app::PlayerAppData;

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
    UpdatePlayerNotes(String),
    DecideCombatAction(CombatantType, Vec<CombatantType>),
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
                    data.new_char_name.clear();
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
    UpdatePlayerNotes(String),
    DecideCombatAction(CombatAction),
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
                        if let Ok(s) = std::fs::read_to_string("classes/fighter.ron") {
                            if let Ok(class) = ron::from_str::<Class>(&s) {
                                character.class = class;
                            }
                        }
                        character.initialize();
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
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum CombatAction {
    Attack(CombatantType),
    RelinquishControl,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
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