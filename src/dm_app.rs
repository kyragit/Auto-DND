use crate::character::{PlayerCharacter, SavingThrows, Attr};
use crate::class::{SavingThrowProgressionType, Class};
use crate::combat::{Fight, Owner, CombatantType, CombatantStats, DamageRoll};
use crate::common_ui::{CommonApp, self, CharacterSheetTab};
use crate::dice::{ModifierType, Drop, DiceRoll, roll};
use crate::enemy::{Enemy, EnemyType, EnemyHitDice, EnemyCategory, Alignment, AttackRoutine};
use crate::item::{ItemType, Encumbrance, WeaponStats, WeaponDamage, MeleeDamage, ContainerStats, Item};
use crate::proficiency::{Proficiency, ProficiencyInstance};
use eframe::egui::{self, Ui, RichText};
use eframe::epaint::Color32;
use crate::mortal_wounds::{MortalWoundsResult, MortalWoundsModifiers, HitDiceValue, TreatmentTiming};
use crate::packets::{ClientBoundPacket, ServerBoundPacket, CombatAction};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::io::prelude::*;
use std::path::Path;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

/// How often the server reads for packets, in milliseconds. Setting this too low may cause 
/// performance problems as it needs a lock on the app data.
pub const SERVER_UPDATE_CLOCK: u64 = 50;

/// Runs the DM (server) application.
pub fn run() -> Result<(), eframe::Error> {
    let mut app_data = DMAppData::new();
    app_data.load();
    let data = Arc::new(Mutex::new(app_data));

    let data_clone_1 = Arc::clone(&data);
    std::thread::Builder::new().name(String::from("handle_streams")).spawn(move || {
        handle_streams(data_clone_1);
    }).unwrap();

    let data_clone_2 = Arc::clone(&data);
    std::thread::Builder::new().name(String::from("handle_connections")).spawn(move || {
        handle_connections(data_clone_2);
    }).unwrap();

    return eframe::run_native(
        "DM Automation Tool", 
        eframe::NativeOptions {
            centered: true,
            initial_window_size: Some(egui::vec2(1280.0, 720.0)),
            ..Default::default()
        }, 
        Box::new(|ctx| {
            ctx.egui_ctx.set_visuals(egui::Visuals {
                ..Default::default()
            });
            Box::new(DMApp::new(data))
        })
    );
}

/// Responsible for reading and handling packets, as well as handling existing connections.
fn handle_streams(data: Arc<Mutex<DMAppData>>) {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(SERVER_UPDATE_CLOCK));
        let data = &mut *data.lock().unwrap();
        let mut closed: Vec<usize> = Vec::new();
        let mut packets: Vec<(ServerBoundPacket, SocketAddr)> = Vec::new();
        let mut client_packets: Vec<ClientBoundPacket> = Vec::new();
        for (i, mut stream) in data.streams.iter_mut().enumerate() {
            let mut reader = std::io::BufReader::new(&mut stream);
            let recieved: Vec<u8>;
            match reader.fill_buf() {
                Ok(buf) => {
                    if buf.is_empty() {
                        continue;
                    }
                    recieved = buf.to_vec();
                },
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::ConnectionReset {
                        closed.push(i);
                        if let Ok(addr) = stream.peer_addr() {
                            for (name, ip) in &data.connected_users {
                                if *ip == addr {
                                    let msg = format!("User \"{}\" has disconnected.", name);
                                    client_packets.push(
                                        ClientBoundPacket::ChatMessage(msg.clone())
                                    );
                                    data.logs.insert(0, msg);
                                }
                            }
                        }
                    }
                    continue;
                },
            }
            reader.consume(recieved.len());
            for split in recieved.split(|byte| *byte == 255) {
                let msg = String::from_utf8(split.to_vec()).unwrap_or(String::new());
                if let Ok(packet) = ron::from_str::<ServerBoundPacket>(msg.as_str()) {
                    if let Ok(addr) = stream.peer_addr() {
                        packets.push((packet, addr));
                    }
                }
            }
        }
        if !closed.is_empty() {
            closed.sort();
            closed.reverse();
            for i in closed {
                let stream = data.streams.remove(i);
                if let Ok(addr) = stream.peer_addr() {
                    data.connected_users.retain(|_, v| *v != addr);
                }
            }
        }
        for (packet, user) in packets {
            packet.handle(data, user);
        }
        for packet in client_packets {
            data.send_to_all_players(packet);
        }
    }
}

/// Responsible for handling new incoming connections.
fn handle_connections(data: Arc<Mutex<DMAppData>>) {
    let host_addr;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(SERVER_UPDATE_CLOCK));
        let data = &mut *data.lock().unwrap();
        if let Some(addr) = data.host_addr {
            host_addr = addr;
            break;
        }
    }
    let listener = TcpListener::bind(host_addr).unwrap();
    for stream in listener.incoming() {
        let data = &mut *data.lock().unwrap();
        match stream {
            Ok(s) => {
                s.set_nonblocking(true).unwrap();
                data.log_private(format!("Connection from user with ip: {:?}", s.peer_addr()));
                data.streams.push(s);
            },
            Err(e) => {
                data.log_private(format!("Connection error: {:?}", e));
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Registry<T> {
    pub tree: HashMap<String, RegistryNode<T>>,
}

impl<T> Registry<T> {
    pub fn new() -> Self {
        Self {
            tree: HashMap::new(),
        }
    }
    pub fn get(&self, path: &str) -> Option<&RegistryNode<T>> {
        let split: Vec<&str> = path.split("/").collect();
        let mut current = &self.tree;
        for (i, key) in split.iter().enumerate() {
            if let Some(node) = current.get(*key) {
                if i == split.len() - 1 {
                    return Some(node);
                }
                match node {
                    RegistryNode::Value(_) => {
                        return None;
                    },
                    RegistryNode::SubRegistry(map) => {
                        current = map;
                    },
                }
            } else {
                return None;
            }
        }
        None
    }
    pub fn register(&mut self, path: &str, value: T) -> Result<(), ()> {
        let mut split: Vec<&str> = path.split(|c| c == '/' || c == '\\').collect();
        let end = split.pop();
        if let Ok(reg) = split.iter().fold(Ok(&mut self.tree), |current, &key| {
            let current = current?;
            if !current.contains_key(key) {
                current.insert(key.to_owned(), RegistryNode::SubRegistry(HashMap::new()));
            }
            match current.get_mut(key).unwrap() {
                RegistryNode::Value(_) => {
                    Err(())
                },
                RegistryNode::SubRegistry(map) => {
                    Ok(map)
                },
            }
        }) {
            if let Some(end) = end {
                reg.insert(end.to_owned(), RegistryNode::Value(value));
                Ok(())
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum RegistryNode<T> {
    Value(T),
    SubRegistry(HashMap<String, RegistryNode<T>>),
}

/// The server's data that is saved to disk.
#[derive(Debug, Serialize, Deserialize)]
pub struct SaveData {
    pub known_users: HashMap<String, String>,
    pub user_data: HashMap<String, UserData>,
    pub fight: Option<Fight>,
    pub deployed_enemies: HashMap<String, (EnemyType, Vec<Enemy>)>,
}

/// Information associated with a user, like their characters.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserData {
    pub characters: HashMap<String, PlayerCharacter>,
    pub notes: String,
}

impl UserData {
    pub fn new() -> Self {
        Self {
            characters: HashMap::new(),
            notes: String::new(),
        }
    }
}

/// Superficial app state that is not saved.
pub struct AppTempState {
    pub exit_without_saving: bool,
    pub window_states: HashMap<String, bool>,
    pub user_charsheet_tab: CharacterSheetTab,
    pub chat: String,
    pub dice_roll: DiceRoll,
    pub dice_roll_advanced: bool,
    pub show_offline_users: bool,
    pub combatant_list: Vec<CombatantType>,
    pub selected_target: usize,
    pub temp_enemy_type: Option<EnemyType>,
    pub temp_enemy_saves_preset: Option<(SavingThrowProgressionType, u8)>,
    pub temp_enemy_filename: String,
    pub viewed_enemy: Option<String>,
    pub temp_item_type: Option<ItemType>,
    pub temp_item_tags: String,
    pub temp_item_filename: String,
    pub viewed_item: Option<String>,
    pub item_give_count: u32,
    pub viewed_prof: Option<String>,
    pub temp_prof: Option<Proficiency>,
    pub temp_prof_filename: String,
    pub temp_prof_valid: String,
}

impl AppTempState {
    pub fn new() -> Self {
        Self {
            exit_without_saving: false,
            window_states: HashMap::new(),
            user_charsheet_tab: CharacterSheetTab::Stats,
            chat: String::new(),
            dice_roll: DiceRoll::simple(1, 20),
            dice_roll_advanced: false,
            show_offline_users: false,
            combatant_list: Vec::new(),
            selected_target: 0,
            temp_enemy_type: None,
            temp_enemy_saves_preset: None,
            temp_enemy_filename: "enemy".to_owned(),
            viewed_enemy: None,
            temp_item_type: None,
            temp_item_tags: String::new(),
            temp_item_filename: "item".to_owned(),
            viewed_item: None,
            item_give_count: 1,
            viewed_prof: None,
            temp_prof: None,
            temp_prof_filename: "prof".to_owned(),
            temp_prof_valid: String::new(),
        }
    }
}

/// All the app's data.
pub struct DMAppData {
    pub host_port: u16,
    pub host_addr: Option<SocketAddr>,
    pub known_users: HashMap<String, String>,
    pub user_data: HashMap<String, UserData>,
    pub connected_users: HashMap<String, SocketAddr>,
    pub logs: Vec<String>,
    pub streams: Vec<TcpStream>,
    pub temp_state: AppTempState,
    pub fight: Option<Fight>,
    pub deployed_enemies: HashMap<String, (EnemyType, Vec<Enemy>)>,
    pub enemy_type_registry: Registry<EnemyType>,
    pub item_type_registry: Registry<ItemType>,
    pub class_registry: Registry<Class>,
    pub proficiency_registry: HashMap<String, Proficiency>,
    pub sorted_prof_list: Vec<(String, String)>,
}

impl DMAppData {
    pub fn new() -> Self {
        Self { 
            host_port: 8080,
            host_addr: None,
            known_users: HashMap::new(),
            user_data: HashMap::new(),
            connected_users: HashMap::new(),
            logs: Vec::new(),
            streams: Vec::new(),
            temp_state: AppTempState::new(),
            fight: None,
            deployed_enemies: HashMap::new(),
            enemy_type_registry: Registry::new(),
            item_type_registry: Registry::new(),
            class_registry: Registry::new(),
            proficiency_registry: HashMap::new(),
            sorted_prof_list: Vec::new(),
        }
    }

    /// Reads data stored on disk, if it exists.
    pub fn load(&mut self) {
        if let Ok(mut file) = std::fs::read_to_string("savedata.ron") {
            match ron::from_str::<SaveData>(&file) {
                Ok(data) => {
                    self.known_users = data.known_users;
                    self.user_data = data.user_data;
                    self.fight = data.fight;
                    self.deployed_enemies = data.deployed_enemies;
                },
                // backs up the existing save data if we couldn't deserialize it
                Err(e) => {
                    file.push_str(&format!("\n\n/* error parsing save data:\n{}\n*/", e));
                    let _ = std::fs::write(format!("backups/{}.ron", chrono::Local::now().format("%d-%m-%Y--%H-%M-%S")), file);
                },
            }
        }
        self.register_enemy_types();
        self.register_item_types();
        self.register_classes();
        self.register_profs();
    }

    fn register_enemy_types(&mut self) {
        Self::read_dir_recursive("enemies", |path, s| {
            if let Ok(enemy) = ron::from_str::<EnemyType>(&s) {
                let _ = self.enemy_type_registry.register(path.strip_prefix("enemies\\").unwrap(), enemy);
            }
        });
    }

    fn register_item_types(&mut self) {
        Self::read_dir_recursive("items", |path, s| {
            if let Ok(item) = ron::from_str::<ItemType>(&s) {
                let _ = self.item_type_registry.register(path.strip_prefix("items\\").unwrap(), item);
            }
        });
    }

    fn register_classes(&mut self) {
        Self::read_dir_recursive("classes", |path, s| {
            if let Ok(class) = ron::from_str::<Class>(&s) {
                let _ = self.class_registry.register(path.strip_prefix("classes\\").unwrap(), class);
            }
        });
    }

    fn register_profs(&mut self) {
        self.sorted_prof_list.clear();
        Self::read_dir_recursive("proficiencies", |path, s| {
            if let Ok(prof) = ron::from_str::<Proficiency>(&s) {
                let path = path.split(|c| c == '/' || c == '\\').last().unwrap_or("error").to_owned();
                self.sorted_prof_list.push((path.clone(), prof.name.clone()));
                self.proficiency_registry.insert(path, prof);
            }
        });
        self.sorted_prof_list.sort();
    }

    /// Reads through all files in a directory, as well as all sub-directories. If the files are 
    /// valid UTF-8, they are passed to the provided function.
    /// 
    /// ## Example
    /// ```rust
    /// Self::read_dir_recursive("some_directory", |path, file| {
    ///     // do something with `path` and `file`...
    /// });
    /// ```
    pub fn read_dir_recursive<F: FnMut(String, String)>(path: impl AsRef<Path>, mut func: F) {
        let mut files: Vec<(String, String)> = Vec::new();
        fn recurse(path: impl AsRef<Path>, files: &mut Vec<(String, String)>) {
            if let Ok(dir) = std::fs::read_dir(path) {
                for entry in dir {
                    if let Ok(entry) = entry {
                        if entry.path().is_dir() {
                            recurse(entry.path(), files);
                            continue;
                        }
                        if let Ok(s) = std::fs::read_to_string(entry.path()) {
                            files.push((entry.path().to_str().map_or("error", |s| s.strip_suffix(".ron").unwrap_or("error")).to_owned(), s));
                        }
                    }
                }
            }
        }
        recurse(path, &mut files);
        for (path, file) in files {
            func(path, file);
        }
    }

    /// Stores the app's data to disk.
    pub fn save(&mut self) {
        let mut file: File;
        if let Ok(f) = File::options().write(true).truncate(true).open("savedata.ron") {
            file = f;
        } else {
            file = File::create("savedata.ron").expect("Failed to create file");
        }
        let save_data = SaveData {
            known_users: self.known_users.clone(),
            user_data: self.user_data.clone(),
            fight: self.fight.clone(),
            deployed_enemies: self.deployed_enemies.clone(),
        };
        let save_data_str = ron::to_string(&save_data).unwrap();
        file.write_all(save_data_str.as_bytes()).unwrap();
    }

    /// Applies a closure to every active tcp stream (connection).
    pub fn foreach_streams<F>(&mut self, mut func: F) 
        where F: FnMut(&mut TcpStream) -> std::io::Result<()> {
        for stream in self.streams.iter_mut() {
            match func(stream) {
                Ok(_) => {},
                Err(_) => {},
            }
        }
    }

    /// Sends a packet to all connected users.
    pub fn send_to_all_players(&mut self, packet: ClientBoundPacket) {
        if let Ok(msg) = ron::to_string(&packet) {
            self.foreach_streams(|stream| {
                stream.write_all(msg.as_bytes())?;
                stream.write_all(&[255])?;
                stream.flush()?;
                Ok(())
            });      
        }
    }

    /// Sends a packet to a user by their ip address. Use this if they do not have a username yet.
    pub fn send_to_user_by_addr(&mut self, packet: ClientBoundPacket, user: SocketAddr) {
        if let Ok(msg) = ron::to_string(&packet) {
            for stream in &mut self.streams {
                if let Ok(addr) = stream.peer_addr() {
                    if addr == user {
                        let _ = stream.write_all(msg.as_bytes());
                        let _ = stream.write_all(&[255]);
                        let _ = stream.flush();
                        return;
                    }
                }
            }
        }
    }

    /// Sends a packet to a user by name.
    pub fn send_to_user(&mut self, packet: ClientBoundPacket, user: String) {
        if let Ok(msg) = ron::to_string(&packet) {
            if let Some(addr) = self.connected_users.get(&user) {
                for stream in &mut self.streams {
                    if let Ok(a) = stream.peer_addr() {
                        if a == *addr {
                            let _ = stream.write_all(msg.as_bytes());
                            let _ = stream.write_all(&[255]);
                            let _ = stream.flush();
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Gets the first connected user with the specified ip address, or None.
    pub fn get_username_by_addr(&self, addr: SocketAddr) -> Option<String> {
        for (name, user) in &self.connected_users {
            if *user == addr {
                return Some(name.clone());
            }
        }
        None
    }

    pub fn log_public(&mut self, msg: impl Into<String> + Clone) {
        self.logs.insert(0, msg.clone().into());
        self.send_to_all_players(ClientBoundPacket::ChatMessage(msg.into()));
    }

    pub fn log_private(&mut self, msg: impl Into<String>) {
        self.logs.insert(0, msg.into());
    }

    /// Passes a mutable reference to the combatant's stats to the provided callback, or None if
    /// it doesn't exist.
    pub fn get_combatant_stats<F, R>(&mut self, combatant: &CombatantType, f: F) -> R 
    where F: FnOnce(Option<&mut CombatantStats>) -> R {
        match combatant {
            CombatantType::Enemy(type_id, id, _) => {
                if let Some((_, group)) = self.deployed_enemies.get_mut(type_id) {
                    if let Some(enemy) = group.get_mut(*id as usize) {
                        return f(Some(&mut enemy.combat_stats));
                    }
                }
                f(None)
            },
            CombatantType::PC(user, name) => {
                if let Some(ud) = self.user_data.get_mut(user) {
                    if let Some(stats) = ud.characters.get_mut(name) {
                        return f(Some(&mut stats.combat_stats));
                    }
                }
                f(None)
            },
        }
    }

    /// Like `get_combatant_stats()`, but does not call the callback at all if the stats could
    /// not be found, instead making an optional return value.
    pub fn get_combatant_stats_alt<F, R>(&mut self, combatant: &CombatantType, f: F) -> Option<R> 
    where F: FnOnce(&mut CombatantStats) -> R {
        match combatant {
            CombatantType::Enemy(type_id, id, _) => {
                if let Some((_, group)) = self.deployed_enemies.get_mut(type_id) {
                    if let Some(enemy) = group.get_mut(*id as usize) {
                        return Some(f(&mut enemy.combat_stats));
                    }
                }
                None
            },
            CombatantType::PC(user, name) => {
                if let Some(ud) = self.user_data.get_mut(user) {
                    if let Some(stats) = ud.characters.get_mut(name) {
                        return Some(f(&mut stats.combat_stats));
                    }
                }
                None
            },
        }
    }

    /// If the combatant exists and is a player character, sends an update packet to the client.
    pub fn update_combatant(&mut self, combatant: &CombatantType) {
        if let CombatantType::PC(user, name) = combatant {
            if let Some(user_data) = self.user_data.get(user) {
                if let Some(sheet) = user_data.characters.get(name) {
                    self.send_to_user(ClientBoundPacket::UpdateCharacter(name.clone(), sheet.clone()), user.clone());
                }
            }
        }
    }
}

/// The main app.
pub struct DMApp {
    /// Data is wrapped in an `Arc<Mutex<_>>` because it is shared state between threads.
    pub data: Arc<Mutex<DMAppData>>,
}

impl DMApp {
    pub fn new(data: Arc<Mutex<DMAppData>>) -> Self {
        Self { 
            data,
        }
    }

    fn chat_window(ctx: &egui::Context, data: &mut DMAppData) {
        egui::Window::new("Chat").collapsible(true).vscroll(true).resizable(true).show(ctx, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                let response = ui.text_edit_singleline(&mut data.temp_state.chat);
                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    if data.temp_state.chat.starts_with("/") {
                        Self::parse_command(data, data.temp_state.chat.clone());
                    } else if !data.temp_state.chat.trim().is_empty() {
                        data.temp_state.chat.insert_str(0, "[server]: ");
                        data.log_public(data.temp_state.chat.clone());
                    }
                    data.temp_state.chat.clear();
                }
                for (i, log) in (&data.logs).into_iter().enumerate() {
                    ui.label(log);
                    if i == 0 {
                        ui.separator();
                    }
                }
            });
        });
    }

    fn parse_command(data: &mut DMAppData, mut command: String) {
        command.remove(0);
        let mut in_quotes = false;
        let mut tree = command.split(|c: char| {
            if c == '\'' || c == '\"' {
                in_quotes = !in_quotes;
            }
            !in_quotes && c.is_whitespace()
        }).map(|s| s.trim_matches(|c| c == '\'' || c == '\"')).into_iter();
        if let Some(token) = tree.next() {
            match token {
                "kick" => {
                    if let Some(token) = tree.next() {
                        if let Some(addr) = data.connected_users.get(token) {
                            let mut msg = "Error".to_owned();
                            for stream in &mut data.streams {
                                if let Ok(a) = stream.peer_addr() {
                                    if *addr == a {
                                        msg = format!("Kicking user \"{}\".", token);
                                        let _ = stream.shutdown(std::net::Shutdown::Both);
                                    }
                                }
                            }
                            data.log_public(msg);
                        } else {
                            if data.known_users.contains_key(token) {
                                data.log_private(format!("User \"{}\" is not connected.", token));
                            } else {
                                data.log_private(format!("User \"{}\" does not exist.", token));
                            }
                        }
                    } else {
                        data.log_private("You must specify a user to kick.");
                    }
                },
                "known_users" => {
                    for user in data.known_users.clone().keys() {
                        data.log_private(format!("- {}", user));
                    }
                    data.log_private("List of all known users:");
                },
                "players" => {
                    let mut empty = true;
                    for user in data.connected_users.clone().keys() {
                        empty = false;
                        data.log_private(format!("- {}", user));
                    }
                    if empty {
                        data.log_private("There are no connected players!");  
                    } else {
                        data.log_private("List of all connected players:");  
                    }
                },
                "save" => {
                    data.save();
                },
                "load" => {
                    data.load();
                },
                "level" => {
                    if let Some(token) = tree.next() {
                        if let Some(user_data) = data.user_data.get_mut(token) {
                            if let Some(token) = tree.next() {
                                if let Some(sheet) = user_data.characters.get_mut(token) {
                                    sheet.level_up();
                                } else {
                                    data.log_private(format!("The character \"{}\" does not exist.", token));
                                }
                            } else {
                                data.log_private("You must specify a character. Make sure to wrap their name in \"quotes\".");
                            }
                        } else {
                            data.log_private(format!("The user \"{}\" does not exist.", token));
                        }
                    } else {
                        data.log_private("You must specify a user. Make sure to wrap their name in \"quotes\".");
                    }
                },
                "r" | "roll" => {
                    data.log_private("Parsing dice notation is not implemented yet. Use the dice roller window for now.");
                },
                _ => {
                    Self::unknown_command(data);
                },
            }
        } else {
            Self::unknown_command(data);
        }
    }
    
    fn unknown_command(data: &mut DMAppData) {
        data.log_public("Unknown command.");
    }

    fn dice_roll_window(ctx: &egui::Context, data: &mut DMAppData) {
        data.create_window(ctx, "Dice Roller", "dice_roller".to_owned(), |window| {
            window.resizable(false)
        }, |ui, data| {
            if ui.checkbox(&mut data.temp_state.dice_roll_advanced, "Show advanced").clicked() {
                data.temp_state.dice_roll.modifier_type = ModifierType::Add;
                data.temp_state.dice_roll.apply_modifier_to_all = false;
                data.temp_state.dice_roll.drop = Drop::None;
                data.temp_state.dice_roll.min_value = 1;
            }
            if data.temp_state.dice_roll_advanced {
                common_ui::dice_roll_editor(ui, &mut data.temp_state.dice_roll);
            } else {
                common_ui::dice_roll_editor_simple(ui, &mut data.temp_state.dice_roll);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Roll public").clicked() {
                    let r = roll(data.temp_state.dice_roll);
                    data.log_public(format!("[server](#roll): {}", r));
                }
                if ui.button("Roll private").clicked() {
                    let r = roll(data.temp_state.dice_roll);
                    data.log_private(format!("[server](#roll): {}", r));
                }
            });
        });
    }

    fn top_bar(ctx: &egui::Context, frame: &mut eframe::Frame, ui: &mut Ui, data: &mut DMAppData) {
        ui.horizontal_top(|ui| {
            if data.host_addr.is_none() {
                if ui.button("Host").clicked() {
                    if let Ok(ip) = local_ip_address::local_ip() {
                        data.host_addr = Some(SocketAddr::new(ip, data.host_port));
                    }
                }
                ui.add(egui::DragValue::new(&mut data.host_port).prefix("Port: "));
            } else {
                let ip = data.host_addr.map(|addr| addr.to_string()).unwrap_or("error".to_owned());
                ui.label(format!("Hosting at: {}", ip));
                if ui.button("Copy").clicked() {
                    ctx.output_mut(|output| output.copied_text = ip);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if ui.button("Save and Exit").clicked() {
                    frame.close();
                }
                if ui.button("Exit Without Saving").clicked() {
                    data.temp_state.window_states.insert("exit_are_you_sure".to_owned(), true);
                }
            });
        });
    }

    fn exit_are_you_sure(ctx: &egui::Context, frame: &mut eframe::Frame, data: &mut DMAppData) {
        egui::Window::new("Exit Without Saving")
            .anchor(egui::Align2::CENTER_CENTER, (0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.colored_label(ctx.style().visuals.error_fg_color, "Are you sure? Everything since the last save will be lost!");
                if ui.button("Yes, I'm sure").clicked() {
                    data.temp_state.exit_without_saving = true;
                    frame.close();
                }
                if ui.button("Wait, go back!").clicked() {
                    data.temp_state.window_states.insert("exit_are_you_sure".to_owned(), false);
                }
            });
        });
    }

    fn users_window(ctx: &egui::Context, data: &mut DMAppData) {
        data.create_window(ctx, "Users", "users_window".to_owned(), |window| {
            window.collapsible(true).resizable(true)
        }, |ui, data| {
            ui.checkbox(&mut data.temp_state.show_offline_users, "Show offline");
            if data.temp_state.show_offline_users {
                for (user, _) in &data.known_users {
                    if ui.button(format!("View: {}", user)).clicked() {
                        let open = data.temp_state.window_states.entry(format!("user_window_<{}>", user)).or_insert(false);
                        *open = !*open;
                    }
                }
            } else {
                for (user, _) in &data.connected_users {
                    if ui.button(format!("View: {}", user)).clicked() {
                        let open = data.temp_state.window_states.entry(format!("user_window_<{}>", user)).or_insert(false);
                        *open = !*open;
                    }
                }
            }
        });
        let mut users: Vec<&String> = Vec::new();
        if data.temp_state.show_offline_users {
            for (user, _) in &data.known_users {
                users.push(user);
            }
        } else {
            for (user, _) in &data.connected_users {
                users.push(user);
            }
        }
        let mut packets = Vec::new();
        for user in users {
            if let Some(user_data) = data.user_data.get_mut(user) {
                let open = data.temp_state.window_states.entry(format!("user_window_<{}>", user)).or_insert(false);
                let mut temp_open = open.clone();
                egui::Window::new(format!("User: {}", user))   
                    .open(&mut temp_open)
                    .show(ctx, |ui| {
                        for (name, _) in &user_data.characters {
                            if ui.button(format!("Character: {}", name)).clicked() {
                                let open = data.temp_state.window_states.entry(format!("user_character_window_<{}>", name)).or_insert(false);
                                *open = !*open;
                            }
                        }
                    });
                data.temp_state.window_states.insert(format!("user_window_<{}>", user), temp_open);

                for (name, sheet) in user_data.characters.iter_mut() {
                    egui::Window::new(format!("Character: {} ({})", name, user))
                        .open(data.temp_state.window_states.entry(format!("user_character_window_<{}>", name)).or_insert(false))
                        .show(ctx, |ui| {
                            ui.vertical(|ui| {
                                let mut changed = false;

                                common_ui::tabs(&mut data.temp_state.user_charsheet_tab, "user_charsheet_tabs".to_owned(), ui, |ui, tab| {
                                    match tab {
                                        CharacterSheetTab::Stats => {
                                            let attrs = sheet.combat_stats.attributes;
                                            ui.label(format!("STR: {} ({:+})", attrs.strength, attrs.modifier(Attr::STR)))
                                                .on_hover_text("Strength represents brute force and muscle mass. It modifies your melee attack and damage rolls.");
                                            ui.label(format!("DEX: {} ({:+})", attrs.dexterity, attrs.modifier(Attr::DEX)))
                                                .on_hover_text("Dexterity represents agility, gracefulness, and hand-eye coordination. It modifies your missile (ranged) attack rolls, armor class, and initiative.");
                                            ui.label(format!("CON: {} ({:+})", attrs.constitution, attrs.modifier(Attr::CON)))
                                                .on_hover_text("Constitution represents health and general hardiness. It modifies your health roll whenever you level up.");
                                            ui.label(format!("INT: {} ({:+})", attrs.intelligence, attrs.modifier(Attr::INT)))
                                                .on_hover_text("Intelligence represents knowledge and academic aptitude. It modifies your spell repertoire, languages spoken, and general proficiencies.");
                                            ui.label(format!("WIS: {} ({:+})", attrs.wisdom, attrs.modifier(Attr::WIS)))
                                                .on_hover_text("Wisdom represents intuition, willpower, and common sense. It modifies all of your saving throws.");
                                            ui.label(format!("CHA: {} ({:+})", attrs.charisma, attrs.modifier(Attr::CHA)))
                                                .on_hover_text("Charisma represents sociability, charm, and leadership. It modifies your reaction rolls, henchmen morale, as well as maximum number of henchmen.");
                                            ui.horizontal(|ui| {
                                                ui.label(format!("HP: {}/{}", sheet.combat_stats.health.current_hp, sheet.combat_stats.health.max_hp));
                                                if ui.small_button("+").clicked() {
                                                    sheet.combat_stats.health.current_hp += 1;
                                                    changed = true;
                                                }
                                                if ui.small_button("-").clicked() {
                                                    sheet.combat_stats.health.current_hp -= 1;
                                                    changed = true;
                                                }
                                            });
                                            ui.label(format!("AC: {}", sheet.combat_stats.armor_class + sheet.combat_stats.modifiers.armor_class.total()));
                                            ui.label(format!("Initiative: {:+}", sheet.combat_stats.modifiers.initiative.total()));
                                            ui.label(format!("Surprise: {:+}", sheet.combat_stats.modifiers.surprise.total()));
                                            ui.label(format!("ATK: {:+}", sheet.combat_stats.attack_throw));
                                            ui.label(format!("Base damage: {}", sheet.combat_stats.damage.display()));
                                            ui.label(format!("Melee ATK bonus: {:+}", sheet.combat_stats.modifiers.melee_attack.total()));
                                            ui.label(format!("Missile ATK bonus: {:+}", sheet.combat_stats.modifiers.missile_attack.total()));
                                            ui.label(format!("Melee DMG bonus: {:+}", sheet.combat_stats.modifiers.melee_damage.total()));
                                            ui.label(format!("Missile DMG bonus: {:+}", sheet.combat_stats.modifiers.missile_damage.total()));
                                            ui.separator();
                                            let saves = sheet.combat_stats.saving_throws;
                                            ui.label("Saving throws:");
                                            ui.label(format!("Petrification & Paralysis: {:+}", saves.petrification_paralysis + sheet.combat_stats.modifiers.save_petrification_paralysis.total()));
                                            ui.label(format!("Poison & Death: {:+}", saves.poison_death + sheet.combat_stats.modifiers.save_poison_death.total()));
                                            ui.label(format!("Blast & Breath: {:+}", saves.blast_breath + sheet.combat_stats.modifiers.save_blast_breath.total()));
                                            ui.label(format!("Staffs & Wands: {:+}", saves.staffs_wands + sheet.combat_stats.modifiers.save_staffs_wands.total()));
                                            ui.label(format!("Spells: {:+}", saves.spells + sheet.combat_stats.modifiers.save_spells.total()));
                                        },
                                        CharacterSheetTab::Class => {

                                        },
                                        CharacterSheetTab::Inventory => {

                                        },
                                        CharacterSheetTab::Proficiencies => {

                                        },
                                        CharacterSheetTab::Spells => {

                                        },
                                        CharacterSheetTab::Notes => {
                                            ui.label(&sheet.notes);
                                        },
                                    }
                                });

                                if changed {
                                    packets.push((ClientBoundPacket::UpdateCharacter(name.clone(), sheet.clone()), user.clone()));
                                }
                            });
                        });
                }
            }
        }
        for (packet, user) in packets {
            data.send_to_user(packet, user);
        }
    }
    fn combat_window(ctx: &egui::Context, data: &mut DMAppData) {
        let open = &mut data.temp_state.window_states.entry("combat".to_owned()).or_insert(false);
        let mut temp_open = open.clone();
        egui::Window::new("Combat")
            .open(&mut temp_open)
            .show(ctx, |ui| {
                if let Some(fight) = &data.fight {
                    let mut fight = fight.clone();
                    match fight.ongoing_round {
                        true => {
                            match &fight.awaiting_response {
                                Some(owner) => {
                                    match owner {
                                        Owner::DM => {
                                            ui.label(format!("Decide action ({})", fight.get_current_actor().name()));
                                            egui::ComboBox::from_label("Target")
                                                .show_index(ui, &mut data.temp_state.selected_target, data.temp_state.combatant_list.len(), |i| data.temp_state.combatant_list[i].name());
                                            if ui.button("Attack").clicked() {
                                                let action = CombatAction::Attack(data.temp_state.combatant_list.remove(data.temp_state.selected_target));
                                                fight.resolve_action(data, action);
                                            }
                                        },
                                        Owner::Player(name) => {
                                            ui.label(format!("Awaiting response from {}", name));
                                        },
                                    }
                                },
                                None => {
                                    if ui.button("Next turn").clicked() {
                                        fight.next_turn(data);
                                    }
                                },
                            }
                        },
                        false => {
                            if ui.button("Begin round").clicked() {
                                fight.start_round(data);
                            }
                        },
                    }
                    data.fight = Some(fight);
                    if ui.button("End combat").clicked() {
                        data.fight = None;
                    }
                } else if ui.button("Start").clicked() {
                    let mut combatants = Vec::new();
                    for (user, _) in &data.connected_users {
                        if let Some(user_data) = data.user_data.get(user) {
                            for (name, _) in &user_data.characters {
                                combatants.push((Owner::Player(user.clone()), CombatantType::PC(user.clone(), name.clone())));
                            }
                        }
                    }
                    for (type_id, (typ, group)) in &data.deployed_enemies {
                        for (id, _) in group.iter().enumerate() {
                            combatants.push((Owner::DM, CombatantType::Enemy(type_id.clone(), id as u32, typ.name.clone())));
                        }
                    }
                    data.fight = Some(Fight::new(combatants));
                }
            });
        data.temp_state.window_states.insert("combat".to_owned(), temp_open);
    }
    fn enemy_viewer_window(ctx: &egui::Context, data: &mut DMAppData) {
        let open = &mut data.temp_state.window_states.entry("enemy_viewer".to_owned()).or_insert(false);
        let mut temp_open = open.clone();
        egui::Window::new("Enemy Viewer")
            .collapsible(true)
            .resizable(true)
            .vscroll(true)
            .open(&mut temp_open)
            .show(ctx, |ui| {
                let mut go_back = false;
                match &mut data.temp_state.viewed_enemy {
                    Some(path) => {
                        match data.enemy_type_registry.get(path) {
                            Some(node) => {
                                match node {
                                    RegistryNode::Value(enemy) => {
                                        ui.horizontal(|ui| {
                                            if ui.small_button("⬅").clicked() {
                                                go_back = true;
                                            }
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.small_button("Deploy").clicked() {
                                                    if !data.deployed_enemies.contains_key(path) {
                                                        data.deployed_enemies.insert(path.clone(), (enemy.clone(), Vec::new()));
                                                    }
                                                    if let Some((_, list)) = data.deployed_enemies.get_mut(path) {
                                                        list.push(Enemy::from_type(enemy));
                                                    }
                                                }
                                            });
                                        });
                                        ui.separator();
                                        ui.heading(&enemy.name);
                                        ui.label(RichText::new(&enemy.description).weak().italics());
                                        ui.separator();
                                        ui.label(format!("HD: {}", enemy.hit_dice.display()));
                                        ui.label(format!("ATK: {:+}", enemy.base_attack_throw));
                                        ui.label(format!("AC: {}", enemy.base_armor_class));
                                        ui.label(format!("DMG: {}", enemy.base_damage.display()));
                                        ui.label(format!("Morale: {:+}", enemy.morale));
                                        ui.label(format!("XP: {}", enemy.xp));
                                        ui.separator();
                                        ui.label(format!("Alignment: {}", enemy.alignment));
                                        let mut list = "Categories: ".to_owned();
                                        if enemy.categories.is_empty() {
                                            list.push_str("None");
                                        } else {
                                            for (i, cat) in enemy.categories.iter().enumerate() {
                                                if i == 0 {
                                                    list.push_str(&format!("{}", cat));
                                                } else {
                                                    list.push_str(&format!(", {}", cat));
                                                }
                                            }
                                        }
                                        ui.label(list);
                                        ui.separator();
                                        ui.label("Saves:");
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.label("P&P");
                                                ui.label(format!("{:+}", enemy.saves.petrification_paralysis));
                                            });
                                            ui.vertical(|ui| {
                                                ui.label("P&D");
                                                ui.label(format!("{:+}", enemy.saves.poison_death));
                                            });
                                            ui.vertical(|ui| {
                                                ui.label("B&B");
                                                ui.label(format!("{:+}", enemy.saves.blast_breath));
                                            });
                                            ui.vertical(|ui| {
                                                ui.label("S&W");
                                                ui.label(format!("{:+}", enemy.saves.staffs_wands));
                                            });
                                            ui.vertical(|ui| {
                                                ui.label("Spells");
                                                ui.label(format!("{:+}", enemy.saves.spells));
                                            });
                                        });
                                    },
                                    RegistryNode::SubRegistry(map) => {
                                        ui.horizontal(|ui| {
                                            if ui.small_button("⬅").clicked() {
                                                go_back = true;
                                            }
                                        });
                                        ui.separator();
                                        if map.is_empty() {
                                            ui.label(RichText::new("There\'s nothing here...").weak().italics());
                                        }
                                        for (subpath, subnode) in map {
                                            match subnode {
                                                RegistryNode::Value(enemy) => {
                                                    if ui.button(format!("View: {}", enemy.name)).clicked() {
                                                        path.push_str("/");
                                                        path.push_str(subpath);
                                                    }
                                                },
                                                RegistryNode::SubRegistry(_) => {
                                                    if ui.button(format!("Folder: {}", subpath)).clicked() {
                                                        path.push_str("/");
                                                        path.push_str(subpath);
                                                    }
                                                },
                                            }
                                        }
                                    },
                                }
                            },
                            None => {
                                data.temp_state.viewed_enemy = None;
                            },
                        }
                    },
                    None => {
                        if data.enemy_type_registry.tree.is_empty() {
                            ui.label(RichText::new("There\'s nothing here...").weak().italics());
                        }
                        for (path, node) in &data.enemy_type_registry.tree {
                            match node {
                                RegistryNode::Value(enemy) => {
                                    if ui.button(format!("View: {}", enemy.name)).clicked() {
                                        data.temp_state.viewed_enemy = Some(path.clone());
                                    }
                                },
                                RegistryNode::SubRegistry(_) => {
                                    if ui.button(format!("Folder: {}", path)).clicked() {
                                        data.temp_state.viewed_enemy = Some(path.clone());
                                    }
                                },
                            }
                        }
                    },
                }
                if go_back {
                    if let Some(path) = &mut data.temp_state.viewed_enemy {
                        data.temp_state.viewed_enemy = path.rsplit_once("/").map(|(s, _)| s.to_owned());
                    }
                }
            });
        data.temp_state.window_states.insert("enemy_viewer".to_owned(), temp_open);
    }
    fn deployed_enemies_window(ctx: &egui::Context, data: &mut DMAppData) {
        let open = &mut data.temp_state.window_states.entry("deployed_enemies".to_owned()).or_insert(false);
        let mut temp_open = open.clone();
        egui::Window::new("Deployed Enemies")
            .collapsible(true)
            .resizable(true)
            .open(&mut temp_open)
            .show(ctx, |ui| {
                if data.deployed_enemies.is_empty() {
                    ui.label("There are currently no deployed enemies. Use the enemy viewer to deploy some!");
                } else {
                    for (_, (typ, group)) in &data.deployed_enemies {
                        for (n, enemy) in group.iter().enumerate() {
                            if n == 0 {
                                ui.label(format!("{}: {}/{}", typ.name, enemy.combat_stats.health.current_hp, enemy.combat_stats.health.max_hp));
                            } else {
                                ui.label(format!("{} {}: {}/{}", typ.name, n + 1, enemy.combat_stats.health.current_hp, enemy.combat_stats.health.max_hp));
                            }
                        }
                    }
                }
            });
        data.temp_state.window_states.insert("deployed_enemies".to_owned(), temp_open);
    }
    fn enemy_creator_window(ctx: &egui::Context, data: &mut DMAppData) {
        let open = &mut data.temp_state.window_states.entry("enemy_creator".to_owned()).or_insert(false);
        let mut temp_open = open.clone();
        egui::Window::new("Enemy Creator")
            .collapsible(true)
            .resizable(true)
            .vscroll(true)
            .open(&mut temp_open)
            .show(ctx, |ui| {
                if let Some(enemy) = &mut data.temp_state.temp_enemy_type {
                    if ui.vertical(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut enemy.name);
                        ui.label("Description:");
                        ui.text_edit_multiline(&mut enemy.description);
                        ui.separator();
                        egui::ComboBox::from_label("Hit dice")
                            .selected_text(format!("{}", enemy.hit_dice))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut enemy.hit_dice, EnemyHitDice::Standard(1), "Standard");
                                ui.selectable_value(&mut enemy.hit_dice, EnemyHitDice::WithModifier(1, 1), "With Modifier");
                                ui.selectable_value(&mut enemy.hit_dice, EnemyHitDice::Special(DiceRoll::simple(1, 8)), "Custom");
                            });
                        match &mut enemy.hit_dice {
                            EnemyHitDice::Standard(amount) => {
                                ui.add(egui::Slider::new(amount, 1..=20).text("Amount").clamp_to_range(true));
                            },
                            EnemyHitDice::WithModifier(amount, modifier) => {
                                ui.add(egui::Slider::new(amount, 1..=20).text("Amount").clamp_to_range(true));
                                ui.add(egui::Slider::new(modifier, -2..=2).text("Modifier").clamp_to_range(false));
                            },
                            EnemyHitDice::Special(roll) => {
                                common_ui::dice_roll_editor_simple(ui, roll);
                            },
                        }
                        ui.separator();
                        ui.label("Armor class:");
                        ui.add(egui::Slider::new(&mut enemy.base_armor_class, 0..=20).clamp_to_range(false));
                        ui.label("Attack throw:");
                        ui.add(egui::Slider::new(&mut enemy.base_attack_throw, 0..=20).clamp_to_range(false));
                        ui.separator();
                        egui::ComboBox::from_label("Attack routine")
                            .selected_text(format!("{}", enemy.base_damage))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut enemy.base_damage, AttackRoutine::One(DamageRoll::default()), "One per round");
                                ui.selectable_value(&mut enemy.base_damage, AttackRoutine::Two(DamageRoll::default(), DamageRoll::default()), "Two per round");
                                ui.selectable_value(&mut enemy.base_damage, AttackRoutine::Three(DamageRoll::default(), DamageRoll::default(), DamageRoll::default()), "Three per round");
                            });
                        ui.add_space(3.0);
                        match &mut enemy.base_damage {
                            AttackRoutine::One(roll1) => {
                                ui.label("Damage roll:");
                                common_ui::damage_roll_editor(ui, roll1);
                            },
                            AttackRoutine::Two(roll1, roll2) => {
                                ui.label("Damage roll (first):");
                                common_ui::damage_roll_editor(ui, roll1);
                                ui.add_space(3.0);
                                ui.label("Damage roll (second):");
                                common_ui::damage_roll_editor(ui, roll2);
                            },
                            AttackRoutine::Three(roll1, roll2, roll3) => {
                                ui.label("Damage roll (first):");
                                common_ui::damage_roll_editor(ui, roll1);
                                ui.add_space(3.0);
                                ui.label("Damage roll (second):");
                                common_ui::damage_roll_editor(ui, roll2);
                                ui.add_space(3.0);
                                ui.label("Damage roll (third):");
                                common_ui::damage_roll_editor(ui, roll3);
                            },
                        }
                        ui.separator();
                        ui.label("XP:");
                        ui.add(egui::Slider::new(&mut enemy.xp, 0..=1000).clamp_to_range(false));
                        ui.label("Morale:");
                        ui.add(egui::Slider::new(&mut enemy.morale, -6..=4).clamp_to_range(false));
                        ui.separator();
                        ui.label("Enemy categories:");
                        if ui.add(egui::SelectableLabel::new(enemy.categories.contains(&EnemyCategory::Animal), "Animal")).clicked() {
                            if !enemy.categories.remove(&EnemyCategory::Animal) {
                                enemy.categories.insert(EnemyCategory::Animal);
                            }
                        }
                        if ui.add(egui::SelectableLabel::new(enemy.categories.contains(&EnemyCategory::Beastman), "Beastman")).clicked() {
                            if !enemy.categories.remove(&EnemyCategory::Beastman) {
                                enemy.categories.insert(EnemyCategory::Beastman);
                            }
                        }
                        if ui.add(egui::SelectableLabel::new(enemy.categories.contains(&EnemyCategory::Construct), "Construct")).clicked() {
                            if !enemy.categories.remove(&EnemyCategory::Construct) {
                                enemy.categories.insert(EnemyCategory::Construct);
                            }
                        }
                        if ui.add(egui::SelectableLabel::new(enemy.categories.contains(&EnemyCategory::Enchanted), "Enchanted")).clicked() {
                            if !enemy.categories.remove(&EnemyCategory::Enchanted) {
                                enemy.categories.insert(EnemyCategory::Enchanted);
                            }
                        }
                        if ui.add(egui::SelectableLabel::new(enemy.categories.contains(&EnemyCategory::Fantastic), "Fantastic")).clicked() {
                            if !enemy.categories.remove(&EnemyCategory::Fantastic) {
                                enemy.categories.insert(EnemyCategory::Fantastic);
                            }
                        }
                        if ui.add(egui::SelectableLabel::new(enemy.categories.contains(&EnemyCategory::GiantHumanoid), "Giant Humanoid")).clicked() {
                            if !enemy.categories.remove(&EnemyCategory::GiantHumanoid) {
                                enemy.categories.insert(EnemyCategory::GiantHumanoid);
                            }
                        }
                        if ui.add(egui::SelectableLabel::new(enemy.categories.contains(&EnemyCategory::Humanoid), "Humanoid")).clicked() {
                            if !enemy.categories.remove(&EnemyCategory::Humanoid) {
                                enemy.categories.insert(EnemyCategory::Humanoid);
                            }
                        }
                        if ui.add(egui::SelectableLabel::new(enemy.categories.contains(&EnemyCategory::Ooze), "Ooze")).clicked() {
                            if !enemy.categories.remove(&EnemyCategory::Ooze) {
                                enemy.categories.insert(EnemyCategory::Ooze);
                            }
                        }
                        if ui.add(egui::SelectableLabel::new(enemy.categories.contains(&EnemyCategory::Summoned), "Summoned")).clicked() {
                            if !enemy.categories.remove(&EnemyCategory::Summoned) {
                                enemy.categories.insert(EnemyCategory::Summoned);
                            }
                        }
                        if ui.add(egui::SelectableLabel::new(enemy.categories.contains(&EnemyCategory::Undead), "Undead")).clicked() {
                            if !enemy.categories.remove(&EnemyCategory::Undead) {
                                enemy.categories.insert(EnemyCategory::Undead);
                            }
                        }
                        if ui.add(egui::SelectableLabel::new(enemy.categories.contains(&EnemyCategory::Vermin), "Vermin")).clicked() {
                            if !enemy.categories.remove(&EnemyCategory::Vermin) {
                                enemy.categories.insert(EnemyCategory::Vermin);
                            }
                        }
                        ui.separator();
                        egui::ComboBox::from_label("Alignment")
                            .selected_text(enemy.alignment.to_string())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut enemy.alignment, Alignment::Lawful, "Lawful");
                                ui.selectable_value(&mut enemy.alignment, Alignment::Neutral, "Neutral");
                                ui.selectable_value(&mut enemy.alignment, Alignment::Chaotic, "Chaotic");
                            });
                        ui.separator();
                        ui.label("Saving throws:");
                        if ui.add(egui::SelectableLabel::new(data.temp_state.temp_enemy_saves_preset.is_some(), "Use preset")).clicked() {
                            if data.temp_state.temp_enemy_saves_preset.take().is_none() {
                                data.temp_state.temp_enemy_saves_preset = Some((SavingThrowProgressionType::Fighter, 1));
                            }
                        }
                        if let Some((typ, level)) = &mut data.temp_state.temp_enemy_saves_preset {
                            egui::ComboBox::from_label("Type")
                                .selected_text(typ.to_string())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(typ, SavingThrowProgressionType::Fighter, "Fighter");
                                    ui.selectable_value(typ, SavingThrowProgressionType::Thief, "Thief");
                                    ui.selectable_value(typ, SavingThrowProgressionType::Cleric, "Cleric");
                                    ui.selectable_value(typ, SavingThrowProgressionType::Mage, "Mage");
                                });
                                ui.add(egui::Slider::new(level, 0..=14).text("Level").clamp_to_range(false));
                        } else {
                            ui.add(egui::Slider::new(&mut enemy.saves.petrification_paralysis, 0..=20).text("P&P").clamp_to_range(false));
                            ui.add(egui::Slider::new(&mut enemy.saves.poison_death, 0..=20).text("P&D").clamp_to_range(false));
                            ui.add(egui::Slider::new(&mut enemy.saves.blast_breath, 0..=20).text("B&B").clamp_to_range(false));
                            ui.add(egui::Slider::new(&mut enemy.saves.staffs_wands, 0..=20).text("S&W").clamp_to_range(false));
                            ui.add(egui::Slider::new(&mut enemy.saves.spells, 0..=20).text("Spells").clamp_to_range(false));
                        }
                        ui.separator();

                        ui.horizontal(|ui| {
                            ui.label("Save as:");
                            ui.text_edit_singleline(&mut data.temp_state.temp_enemy_filename);
                        });
                        ui.label(RichText::new("Hint: you can use \"/ \" to specify a folder.").weak().italics());
                        if ui.button("Save").clicked() {
                            if let Some((typ, level)) = data.temp_state.temp_enemy_saves_preset {
                                enemy.saves = SavingThrows::calculate_simple(typ, level);
                            }
                            if let Ok(_) = enemy.save(data.temp_state.temp_enemy_filename.trim()) {
                                data.temp_state.temp_enemy_filename = "enemy".to_owned();
                                return true;
                            }
                        }
                        false
                    }).inner {
                        data.temp_state.temp_enemy_type = None;
                        data.register_enemy_types();
                    }
                } else {
                    if ui.button("Create new").clicked() {
                        data.temp_state.temp_enemy_type = Some(EnemyType::default());
                    }
                }
            });
        data.temp_state.window_states.insert("enemy_creator".to_owned(), temp_open);
    }
    fn item_creator_window(ctx: &egui::Context, data: &mut DMAppData) {
        let open = &mut data.temp_state.window_states.entry("item_creator".to_owned()).or_insert(false);
        let mut temp_open = open.clone();
        egui::Window::new("Item Creator")
            .collapsible(true)
            .resizable(true)
            .vscroll(true)
            .open(&mut temp_open)
            .show(ctx, |ui| {
                if let Some(item) = &mut data.temp_state.temp_item_type {
                    if ui.vertical(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut item.name);
                        ui.label("Description:");
                        ui.text_edit_multiline(&mut item.description);
                        ui.separator();
                        egui::ComboBox::from_label("Encumbrance")
                            .selected_text(format!("{}", item.encumbrance))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut item.encumbrance, Encumbrance::Negligible, Encumbrance::Negligible.to_string());
                                ui.selectable_value(&mut item.encumbrance, Encumbrance::Treasure, Encumbrance::Treasure.to_string());
                                ui.selectable_value(&mut item.encumbrance, Encumbrance::OneSixth, Encumbrance::OneSixth.to_string());
                                ui.selectable_value(&mut item.encumbrance, Encumbrance::OneHalf, Encumbrance::OneHalf.to_string());
                                ui.selectable_value(&mut item.encumbrance, Encumbrance::OneStone, Encumbrance::OneStone.to_string());
                                ui.selectable_value(&mut item.encumbrance, Encumbrance::VeryHeavy(1), Encumbrance::VeryHeavy(1).to_string());
                            });
                        if let Encumbrance::VeryHeavy(stone) = &mut item.encumbrance {
                            ui.add(egui::Slider::new(stone, 1..=10).clamp_to_range(false).text("Stone"));
                        }
                        ui.add(egui::Slider::new(&mut item.value.0, 0.0..=1000.0).clamp_to_range(false).text("Value (in silver)"));
                        ui.label("Tags:");
                        ui.label(RichText::new("Enter a comma-seperated list of tags, i.e. arrow, wooden, magical").weak().italics());
                        ui.text_edit_singleline(&mut data.temp_state.temp_item_tags);
                        ui.separator();
                        let mut is_weapon = item.weapon_stats.is_some();
                        if ui.checkbox(&mut is_weapon, "Weapon").clicked() {
                            if item.weapon_stats.take().is_none() {
                                item.weapon_stats = Some(WeaponStats::default());
                            }
                        }
                        if let Some(weapon) = &mut item.weapon_stats {
                            egui::ComboBox::from_label("Weapon Type")
                                .selected_text(&weapon.damage.display())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut weapon.damage, WeaponDamage::Melee(MeleeDamage::OneHanded(DamageRoll::melee())), "Melee");
                                    ui.selectable_value(&mut weapon.damage, WeaponDamage::Missile(DamageRoll::missile(), "arrow".to_owned()), "Missile");
                                });
                            match &mut weapon.damage {
                                WeaponDamage::Melee(melee) => {
                                    egui::ComboBox::from_label("Melee Style")
                                        .selected_text(&melee.display())
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(melee, MeleeDamage::OneHanded(DamageRoll::melee()), "One-Handed");
                                            ui.selectable_value(melee, MeleeDamage::Versatile(DamageRoll::melee(), DamageRoll::melee()), "Versatile");
                                            ui.selectable_value(melee, MeleeDamage::TwoHanded(DamageRoll::melee()), "Two-Handed");
                                        });
                                    ui.label("Damage:");
                                    match melee {
                                        MeleeDamage::OneHanded(dmg) => {
                                            common_ui::damage_roll_editor(ui, dmg);
                                        },
                                        MeleeDamage::Versatile(dmg1, dmg2) => {
                                            ui.label("One-Handed:");
                                            common_ui::damage_roll_editor(ui, dmg1);
                                            ui.label("Two-Handed:");
                                            common_ui::damage_roll_editor(ui, dmg2);
                                        },
                                        MeleeDamage::TwoHanded(dmg) => {
                                            common_ui::damage_roll_editor(ui, dmg);
                                        },
                                    }
                                },
                                WeaponDamage::Missile(missile, ammo) => {
                                    ui.label("Damage:");
                                    common_ui::damage_roll_editor(ui, missile);
                                    ui.label("Ammo Type:");
                                    ui.text_edit_singleline(ammo);
                                },
                            }
                            ui.separator();
                        }
                        let mut is_armor = item.armor_stats.is_some();
                        if ui.checkbox(&mut is_armor, "Armor").clicked() {
                            if item.armor_stats.take().is_none() {
                                item.armor_stats = Some(1);
                            }
                        }
                        if let Some(armor) = &mut item.armor_stats {
                            ui.add(egui::Slider::new(armor, 1..=10).clamp_to_range(false).text("Armor Class"));
                            ui.separator();
                        }
                        let mut is_shield = item.shield_stats.is_some();
                        if ui.checkbox(&mut is_shield, "Shield").clicked() {
                            if item.shield_stats.take().is_none() {
                                item.shield_stats = Some(1);
                            }
                        }
                        if let Some(shield) = &mut item.shield_stats {
                            ui.add(egui::Slider::new(shield, 1..=2).clamp_to_range(false).text("Armor Class Bonus"));
                            ui.separator();
                        }
                        let mut is_container = item.container_stats.is_some();
                        if ui.checkbox(&mut is_container, "Container").clicked() {
                            if item.container_stats.take().is_none() {
                                item.container_stats = Some(ContainerStats::Stone(1));
                            }
                        }
                        if let Some(container) = &mut item.container_stats {
                            egui::ComboBox::from_label("Container Type")
                                .selected_text(container.to_string())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(container, ContainerStats::Items(1), "Items");
                                    ui.selectable_value(container, ContainerStats::Stone(1), "Stone");
                                });
                            match container {
                                ContainerStats::Items(i) => {
                                    ui.add(egui::Slider::new(i, 1..=10).clamp_to_range(false).text("Capacity"));
                                },
                                ContainerStats::Stone(i) => {
                                    ui.add(egui::Slider::new(i, 1..=10).clamp_to_range(false).text("Capacity"));
                                },
                            }
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Save as:");
                            ui.text_edit_singleline(&mut data.temp_state.temp_item_filename);
                        });
                        ui.label(RichText::new("Hint: you can use \"/ \" to specify a folder.").weak().italics());
                        if ui.button("Save").clicked() {
                            for tag in data.temp_state.temp_item_tags.split(",") {
                                if !tag.trim().is_empty() {
                                    item.tags.insert(tag.trim().to_owned());
                                }
                            }
                            if let Ok(_) = item.save(data.temp_state.temp_item_filename.trim()) {
                                data.temp_state.temp_item_tags.clear();
                                data.temp_state.temp_item_filename = "item".to_owned();
                                return true;
                            } else {
                                item.tags.clear();
                            }
                        }
                        false
                    }).inner {
                        data.temp_state.temp_item_type = None;
                        data.register_item_types();
                    }
                } else {
                    if ui.button("Create new").clicked() {
                        data.temp_state.temp_item_type = Some(ItemType::default());
                    } 
                }
            });
        data.temp_state.window_states.insert("item_creator".to_owned(), temp_open);
    }
    fn item_viewer_window(ctx: &egui::Context, data: &mut DMAppData) {
        let open = &mut data.temp_state.window_states.entry("item_viewer".to_owned()).or_insert(false);
        let mut temp_open = open.clone();
        egui::Window::new("Item Viewer")
            .collapsible(true)
            .resizable(true)
            .vscroll(true)
            .open(&mut temp_open)
            .show(ctx, |ui| {
                let mut packets: Vec<(ClientBoundPacket, String)> = Vec::new();
                let mut go_back = false;
                match &mut data.temp_state.viewed_item {
                    Some(path) => {
                        match data.item_type_registry.get(path) {
                            Some(node) => {
                                match node {
                                    RegistryNode::Value(item) => {
                                        ui.horizontal(|ui| {
                                            if ui.small_button("⬅").clicked() {
                                                go_back = true;
                                            }
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.menu_button("Give to...", |ui| {
                                                    ui.add(egui::Slider::new(&mut data.temp_state.item_give_count, 1..=1000).clamp_to_range(false).text("Count"));
                                                    ui.separator();
                                                    for (user, _) in &mut data.connected_users {
                                                        if let Some(user_data) = data.user_data.get_mut(user) {
                                                            for (name, sheet) in &mut user_data.characters {
                                                                if ui.button(format!("{} ({})", name, user)).clicked() {
                                                                    sheet.inventory.add(Item::from_type(item.clone(), data.temp_state.item_give_count));
                                                                    packets.push((ClientBoundPacket::UpdateCharacter(name.clone(), sheet.clone()), user.clone()));
                                                                    data.temp_state.item_give_count = 1;
                                                                    ui.close_menu();
                                                                }
                                                            }
                                                        }
                                                    }
                                                });
                                            });
                                        });
                                        ui.separator();
                                        ui.heading(&item.name);
                                        ui.label(RichText::new(&item.description).weak().italics());
                                        ui.separator();
                                        ui.label(format!("Encumbrance: {}", item.encumbrance.display()));
                                        ui.label(format!("Value: {:.1} sp", item.value.0))
                                            .on_hover_text(
                                            RichText::new(format!("{:.1} cp\n{:.1} sp\n{:.1} ep\n{:.1} gp\n{:.1} pp", 
                                                item.value.as_copper(),
                                                item.value.as_silver(),
                                                item.value.as_electrum(),
                                                item.value.as_gold(),
                                                item.value.as_platinum(),
                                            )).weak().italics());
                                        let mut list = "Tags: ".to_owned();
                                        if item.tags.is_empty() {
                                            list.push_str("None");
                                        } else {
                                            for (i, tag) in item.tags.iter().enumerate() {
                                                if i == 0 {
                                                    list.push_str(&format!("{}", tag));
                                                } else {
                                                    list.push_str(&format!(", {}", tag));
                                                }
                                            }
                                        }
                                        ui.label(list);
                                        ui.separator();
                                        if let Some(weapon) = &item.weapon_stats {
                                            match &weapon.damage {
                                                WeaponDamage::Melee(melee) => {
                                                    ui.label(RichText::new("Melee weapon").strong().underline());
                                                    ui.label(format!("Style: {}", melee.display()));
                                                    match melee {
                                                        MeleeDamage::OneHanded(dmg) => {
                                                            ui.label(format!("Damage: {}", dmg.to_notation()));
                                                        },
                                                        MeleeDamage::Versatile(dmg1, dmg2) => {
                                                            ui.label(format!("Damage: {}/{}", dmg1.to_notation(), dmg2.to_notation()));
                                                        },
                                                        MeleeDamage::TwoHanded(dmg) => {
                                                            ui.label(format!("Damage: {}", dmg.to_notation()));
                                                        },
                                                    }
                                                },
                                                WeaponDamage::Missile(damage, ammo) => {
                                                    ui.label(RichText::new("Missile weapon").strong().underline());
                                                    ui.label(format!("Damage: {}", damage.to_notation()));
                                                    ui.label(format!("Ammo: {}", ammo));
                                                },
                                            }
                                            ui.separator();
                                        }
                                        if let Some(armor) = &item.armor_stats {
                                            ui.label(RichText::new("Armor").strong().underline());
                                            ui.label(format!("AC: {}", armor));
                                            ui.separator();
                                        }
                                        if let Some(shield) = &item.shield_stats {
                                            ui.label(RichText::new("Shield").strong().underline());
                                            ui.label(format!("AC: {:+}", shield));
                                            ui.separator();
                                        }
                                        if let Some(container) = &item.container_stats {
                                            ui.label(RichText::new("Container").strong().underline());
                                            match container {
                                                ContainerStats::Items(i) => {
                                                    ui.label(format!("Holds: {} items", i));
                                                },
                                                ContainerStats::Stone(i) => {
                                                    ui.label(format!("Holds: {} stone", i));
                                                },
                                            }
                                            ui.separator();
                                        }
                                    },
                                    RegistryNode::SubRegistry(map) => {
                                        ui.horizontal(|ui| {
                                            if ui.small_button("⬅").clicked() {
                                                go_back = true;
                                            }
                                        });
                                        ui.separator();
                                        if map.is_empty() {
                                            ui.label(RichText::new("There\'s nothing here...").weak().italics());
                                        }
                                        for (subpath, subnode) in map {
                                            match subnode {
                                                RegistryNode::Value(item) => {
                                                    if ui.button(format!("View: {}", item.name)).clicked() {
                                                        path.push_str("/");
                                                        path.push_str(subpath);
                                                    }
                                                },
                                                RegistryNode::SubRegistry(_) => {
                                                    if ui.button(format!("Folder: {}", subpath)).clicked() {
                                                        path.push_str("/");
                                                        path.push_str(subpath);
                                                    }
                                                },
                                            }
                                        }
                                    },
                                }
                            },
                            None => {
                                data.temp_state.viewed_item = None;
                            },
                        }
                    },
                    None => {
                        if data.item_type_registry.tree.is_empty() {
                            ui.label(RichText::new("There\'s nothing here...").weak().italics());
                        }
                        for (path, node) in &data.item_type_registry.tree {
                            match node {
                                RegistryNode::Value(item) => {
                                    if ui.button(format!("View: {}", item.name)).clicked() {
                                        data.temp_state.viewed_item = Some(path.clone());
                                    }
                                },
                                RegistryNode::SubRegistry(_) => {
                                    if ui.button(format!("Folder: {}", path)).clicked() {
                                        data.temp_state.viewed_item = Some(path.clone());
                                    }
                                },
                            }
                        }
                    },
                }
                if go_back {
                    if let Some(path) = &mut data.temp_state.viewed_item {
                        data.temp_state.viewed_item = path.rsplit_once("/").map(|(s, _)| s.to_owned());
                    }
                }
                for (packet, user) in packets {
                    data.send_to_user(packet, user);
                }
            });
        data.temp_state.window_states.insert("item_viewer".to_owned(), temp_open);
    }
    fn prof_viewer(ctx: &egui::Context, data: &mut DMAppData) {
        let open = &mut data.temp_state.window_states.entry("prof_viewer".to_owned()).or_insert(false);
        let mut temp_open = open.clone();
        egui::Window::new("Proficiency Viewer")
            .collapsible(true)
            .resizable(true)
            .vscroll(true)
            .open(&mut temp_open)
            .show(ctx, |ui| {
                let mut go_back = false;
                let mut packets = Vec::new();
                if let Some(id) = &data.temp_state.viewed_prof {
                    if let Some(prof) = data.proficiency_registry.get(id) {
                        ui.horizontal(|ui| {
                            if ui.small_button("⬅").clicked() {
                                go_back = true;
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.menu_button("Grant to...", |ui| {
                                    for (user, _) in &mut data.connected_users {
                                        if let Some(user_data) = data.user_data.get_mut(user) {
                                            for (name, sheet) in &mut user_data.characters {
                                                if ui.button(format!("{} ({})", name, user)).clicked() {
                                                    sheet.add_prof(id, ProficiencyInstance::from_prof(prof.clone()));
                                                    packets.push((ClientBoundPacket::UpdateCharacter(name.clone(), sheet.clone()), user.clone()));
                                                    ui.close_menu();
                                                }
                                            }
                                        }
                                    } 
                                });
                            });
                        });
                        ui.separator();
                        ui.heading(&prof.name);
                        if prof.is_general {
                            ui.colored_label(Color32::GREEN, "General Proficiency");
                        } else {
                            ui.colored_label(Color32::YELLOW, "Class Proficiency");
                        }
                        ui.label(RichText::new(&prof.description).weak().italics());
                        ui.separator();
                        if prof.max_level > 0 {
                            ui.label("This proficiency can be taken more than once.");
                        }
                    } else {
                        data.temp_state.viewed_prof = None;
                    }
                } else {
                    for (id, name) in &data.sorted_prof_list {
                        if ui.button(name).clicked() {
                            data.temp_state.viewed_prof = Some(id.clone());
                        }
                    }
                }
                if go_back {
                    data.temp_state.viewed_prof = None;
                }
                for (packet, user) in packets {
                    data.send_to_user(packet, user);
                }
            });
        data.temp_state.window_states.insert("prof_viewer".to_owned(), temp_open);
    }
    fn prof_creator(ctx: &egui::Context, data: &mut DMAppData) {
        let open = &mut data.temp_state.window_states.entry("prof_creator".to_owned()).or_insert(false);
        let mut temp_open = open.clone();
        egui::Window::new("Proficiency Creator")
            .collapsible(true)
            .resizable(true)
            .vscroll(true)
            .open(&mut temp_open)
            .show(ctx, |ui| {
                if let Some(prof) = &mut data.temp_state.temp_prof {
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut prof.name);
                    });
                    ui.label("Description:");
                    ui.text_edit_multiline(&mut prof.description);
                    ui.separator();
                    ui.checkbox(&mut prof.is_general, "General proficiency");
                    let mut temp = prof.max_level + 1;
                    ui.add(egui::Slider::new(&mut temp, 1..=10).clamp_to_range(true).text("How many times can this be taken?"));
                    prof.max_level = temp - 1;
                    ui.checkbox(&mut prof.requires_specification, "Requires a type");
                    if prof.requires_specification {
                        let mut temp = prof.valid_specifications.is_some();
                        if ui.checkbox(&mut temp, "Requires specific types").clicked() {
                            if prof.valid_specifications.take().is_none() {
                                prof.valid_specifications = Some(HashSet::new());
                            }
                        }
                        if prof.valid_specifications.is_some() {
                            ui.add(egui::TextEdit::singleline(&mut data.temp_state.temp_prof_valid).hint_text("Comma-separated list, e.g. Air, Earth, Water, Fire"));
                        }
                    }
                    let mut temp = prof.starting_throw.is_some();
                    if ui.checkbox(&mut temp, "Can be rolled against").clicked() {
                        if prof.starting_throw.take().is_none() {
                            prof.starting_throw = Some(0);
                        }
                    }
                    if let Some(throw) = &mut prof.starting_throw {
                        ui.add(egui::Slider::new(throw, 0..=20).clamp_to_range(false).text("Starting throw modifier"));
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Save as:");
                        ui.text_edit_singleline(&mut data.temp_state.temp_prof_filename);
                    });
                    if ui.button("Save").clicked() {
                        if let Some(valid) = &mut prof.valid_specifications {
                            valid.clear();
                            for v in data.temp_state.temp_prof_valid.split(",") {
                                if !v.trim().is_empty() {
                                    valid.insert(v.trim().to_owned());
                                }
                            }   
                        }
                        if let Ok(_) = prof.save(data.temp_state.temp_prof_filename.trim()) {
                            data.temp_state.temp_prof_valid.clear();
                            data.temp_state.temp_prof_filename = "prof".to_owned();
                            data.temp_state.temp_prof = None;
                            data.register_profs();
                        }
                    }
                } else {
                    if ui.button("Create new").clicked() {
                        data.temp_state.temp_prof = Some(Proficiency::new());
                    }
                }
            });
        data.temp_state.window_states.insert("prof_creator".to_owned(), temp_open);
    }
}

impl eframe::App for DMApp {
    fn on_close_event(&mut self) -> bool {
        let data = &mut *self.data.lock().unwrap();
        if data.temp_state.exit_without_saving {
            return true;
        }
        data.save();
        true
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let data = &mut *self.data.lock().unwrap();
        Self::chat_window(ctx, data);
        Self::dice_roll_window(ctx, data);
        Self::users_window(ctx, data);
        Self::combat_window(ctx, data);
        Self::enemy_viewer_window(ctx, data);
        Self::deployed_enemies_window(ctx, data);
        Self::enemy_creator_window(ctx, data);
        Self::item_viewer_window(ctx, data);
        Self::item_creator_window(ctx, data);
        Self::prof_viewer(ctx, data);
        Self::prof_creator(ctx, data);
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.add_space(5.0);
                Self::top_bar(ctx, frame, ui, data);
                ui.add_space(5.0);
            });
        });
        egui::SidePanel::left("left_panel").show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.add_space(4.0);
                ui.style_mut().spacing.item_spacing = egui::vec2(4.0, 4.0);
                if ui.button("Dice Roller").clicked() {
                    data.toggle_window_state("dice_roller");
                }
                if ui.button("Users").clicked() {
                    data.toggle_window_state("users_window");
                }
                if ui.button("Combat").clicked() {
                    data.toggle_window_state("combat");
                }
                if ui.button("Enemy Viewer").clicked() {
                    data.toggle_window_state("enemy_viewer");
                }
                if ui.button("Deployed Enemies").clicked() {
                    data.toggle_window_state("deployed_enemies");
                }
                if ui.button("Enemy Creator").clicked() {
                    data.toggle_window_state("enemy_creator");
                }
                if ui.button("Item Viewer").clicked() {
                    data.toggle_window_state("item_viewer");
                }
                if ui.button("Item Creator").clicked() {
                    data.toggle_window_state("item_creator");
                }
                if ui.button("Proficiency Viewer").clicked() {
                    data.toggle_window_state("prof_viewer");
                }
                if ui.button("Proficiency Creator").clicked() {
                    data.toggle_window_state("prof_creator");
                }
                if ui.button("Test Mortal Wounds").clicked() {
                    data.log_private(
                        MortalWoundsResult::roll(
                            MortalWoundsModifiers::new(1, HitDiceValue::D6, -4, 10, 1, 1, false, 
                                TreatmentTiming::OneHour, 0))
                                .condition.description()
                    );
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |_ui| {

        });
        if *data.temp_state.window_states.entry("exit_are_you_sure".to_owned()).or_insert(false) {
            Self::exit_are_you_sure(ctx, frame, data);
        }
        ctx.request_repaint();
    }
}