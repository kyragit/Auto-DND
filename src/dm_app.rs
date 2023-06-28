use crate::{AppPreferences, WindowPreferences};
use crate::character::{PlayerCharacter, SavingThrows, Attr};
use crate::class::{SavingThrowProgressionType, Class, ClassDamageBonus, Cleaves, HitDie, AttackThrowProgression, WeaponSelection, BroadWeapons, NarrowWeapons, RestrictedWeapons, ArmorSelection, THIEF_SKILLS};
use crate::combat::{Fight, Owner, CombatantType, CombatantStats, DamageRoll};
use crate::common_ui::{self, CharacterSheetTab, back_arrow};
use crate::dice::{ModifierType, Drop, DiceRoll, roll};
use crate::enemy::{Enemy, EnemyType, EnemyHitDice, EnemyCategory, Alignment, AttackRoutine};
use crate::item::{ItemType, Encumbrance, WeaponStats, WeaponDamage, MeleeDamage, ContainerStats, Item};
use crate::proficiency::{Proficiency, ProficiencyInstance};
use crate::race::Race;
use crate::spell::{Spell, MagicType, SpellRange, SpellDuration, SpellRegistry};
use eframe::egui::{self, Ui, RichText, WidgetText};
use eframe::epaint::Color32;
use egui_dock::{DockArea, Tree, NodeIndex, TabViewer};
use crate::packets::{ClientBoundPacket, ServerBoundPacket, CombatAction, Request};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::io::prelude::*;
use std::path::Path;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use egui_phosphor as ep;

/// How often the server reads for packets, in milliseconds. Setting this too low may cause 
/// performance problems as it needs a lock on the app data.
pub const SERVER_UPDATE_CLOCK: u64 = 50;

/// Runs the DM (server) application.
pub fn run(prefs: AppPreferences) -> Result<(), eframe::Error> {
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
        if let Some(p) = prefs.dm_window {
            p.to_native_options()
        } else {
            eframe::NativeOptions {
                centered: true,
                initial_window_size: Some(egui::vec2(1280.0, 720.0)),
                follow_system_theme: false,
                ..Default::default()
            }
        }, 
        Box::new(|ctx| {
            ctx.egui_ctx.set_visuals(egui::Visuals {
                ..Default::default()
            });
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts);
            ctx.egui_ctx.set_fonts(fonts);
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

/// A tree-like data structure that holds values and folders of values.
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
    /// Gets the node at the specified `path`.
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
    /// Gets the value at the node at `path`, if it exists.
    pub fn get_value(&self, path: &str) -> Option<&T> {
        match self.get(path) {
            Some(node) => {
                match node {
                    RegistryNode::Value(value) => Some(value),
                    _ => None,
                }
            },
            None => None,
        }
    }
    /// Inserts a `value` at `path`.
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

/// Saves a serializable value to disk.
pub fn save_ron<S: Serialize>(obj: &S, dir: &str, file: &str) -> Result<(), ()> {
    if let Ok(s) = ron::to_string(obj) {
        let file = format!("{}/{}.ron", dir, file);
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
    #[serde(skip)]
    pub charsheet_tabs: HashMap<String, CharacterSheetTab>,
}

impl UserData {
    pub fn new() -> Self {
        Self {
            characters: HashMap::new(),
            notes: String::new(),
            charsheet_tabs: HashMap::new(),
        }
    }
}

/// Superficial app state that is not saved.
pub struct AppTempState {
    pub exit_without_saving: bool,
    pub window_states: HashMap<String, bool>,
    pub user_charsheet_tab: CharacterSheetTab,
    pub chat: String,
    pub last_chat: String,
    pub unread_messages: u32,
    pub unread_msg_buffer: bool,
    pub dice_roll: DiceRoll,
    pub dice_roll_advanced: bool,
    pub dice_roll_public: bool,
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
    pub viewed_class: Option<String>,
    pub temp_class: Option<Class>,
    pub temp_class_filename: String,
    pub temp_class_profs: String,
    pub viewed_spell: Option<(MagicType, Option<(u8, Option<String>)>)>,
    pub temp_spell: Option<Spell>,
    pub temp_spell_filename: String,
    pub requests: Vec<(String, Request)>,
}

impl AppTempState {
    pub fn new() -> Self {
        Self {
            exit_without_saving: false,
            window_states: HashMap::new(),
            user_charsheet_tab: CharacterSheetTab::Stats,
            chat: String::new(),
            last_chat: String::new(),
            unread_messages: 0,
            unread_msg_buffer: false,
            dice_roll: DiceRoll::simple(1, 20),
            dice_roll_advanced: false,
            dice_roll_public: true,
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
            viewed_class: None,
            temp_class: None,
            temp_class_filename: "class".to_owned(),
            temp_class_profs: String::new(),
            viewed_spell: None,
            temp_spell: None,
            temp_spell_filename: "spell".to_owned(),
            requests: Vec::new(),
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
    pub spell_registry: SpellRegistry,
    pub prefs: WindowPreferences,
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
            spell_registry: SpellRegistry::new(),
            prefs: WindowPreferences::new(),
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
        self.register_spells();
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

    fn register_spells(&mut self) {
        Self::read_dir_recursive("spells", |path, s| {
            if let Ok(spell) = ron::from_str::<Spell>(&s) {
                let path = path.split(|c| c == '/' || c == '\\').last().unwrap_or("error").to_owned();
                match spell.magic_type {
                    MagicType::Arcane => {
                        if (spell.spell_level as usize) < self.spell_registry.arcane.len() {
                            self.spell_registry.arcane[spell.spell_level as usize].insert(path, spell);
                        }
                    },
                    MagicType::Divine => {
                        if (spell.spell_level as usize) < self.spell_registry.divine.len() {
                            self.spell_registry.divine[spell.spell_level as usize].insert(path, spell);
                        }
                    },
                }
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
        if let Ok(s) = std::fs::read_to_string("preferences.ron") {
            if let Ok(mut prefs) = ron::from_str::<AppPreferences>(&s) {
                prefs.dm_window = Some(self.prefs.clone());
                let _ = std::fs::write("preferences.ron", ron::to_string(&prefs).unwrap_or(s));
            }
        }
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

    /// Sends a chat message to all users.
    pub fn log_public(&mut self, msg: impl Into<String> + Clone) {
        self.logs.insert(0, msg.clone().into());
        self.send_to_all_players(ClientBoundPacket::ChatMessage(msg.into()));
    }

    /// Sends a chat message to the DM only.
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

    /// Gets a reference to a player character, if it exists.
    pub fn get_player_char(&self, user: impl Into<String>, name: impl Into<String>) -> Option<&PlayerCharacter> {
        if let Some(user_data) = self.user_data.get(&user.into()) {
            if let Some(sheet) = user_data.characters.get(&name.into()) {
                return Some(sheet);
            }
        }
        None
    }

    /// Gets a mutable reference to a player character, if it exists.
    pub fn get_player_char_mut(&mut self, user: impl Into<String>, name: impl Into<String>) -> Option<&mut PlayerCharacter> {
        if let Some(user_data) = self.user_data.get_mut(&user.into()) {
            if let Some(sheet) = user_data.characters.get_mut(&name.into()) {
                return Some(sheet);
            }
        }
        None
    }

    /// If the character exists, passes it to the given closure.
    pub fn apply_to_pc<R>(&mut self, user: impl Into<String>, name: impl Into<String>, mut func: impl FnMut(&mut PlayerCharacter) -> R) -> Option<R> {
        if let Some(user_data) = self.user_data.get_mut(&user.into()) {
            if let Some(sheet) = user_data.characters.get_mut(&name.into()) {
                return Some(func(sheet));
            }
        }
        None
    }

    /// If the character exists, passes it to the given closure. Otherwise, returns the given default.
    pub fn apply_to_pc_or<R>(&mut self, user: impl Into<String>, name: impl Into<String>, default: R, mut func: impl FnMut(&mut PlayerCharacter) -> R) -> R {
        if let Some(user_data) = self.user_data.get_mut(&user.into()) {
            if let Some(sheet) = user_data.characters.get_mut(&name.into()) {
                return func(sheet);
            }
        }
        default
    }

    pub fn get_chat_title(&self) -> WidgetText {
        if self.temp_state.unread_messages == 0 || self.temp_state.unread_msg_buffer {
            format!("{}", ep::CHAT_TEXT).into()
        } else {
            RichText::new(format!("{}({})", ep::CHAT_TEXT, self.temp_state.unread_messages)).color(Color32::RED).into()
        }
    }
}

/// The main app.
pub struct DMApp {
    /// Data is wrapped in an `Arc<Mutex<_>>` because it is shared state between threads.
    pub data: Arc<Mutex<DMAppData>>,
    pub tree: Tree<DMTab>,
}

impl DMApp {
    pub fn new(data: Arc<Mutex<DMAppData>>) -> Self {
        Self { 
            data,
            tree: {
                let tree = Tree::new(vec![DMTab::Chat]);
                tree
            },
        }
    }

    /// The chat log window.
    fn chat_window(ctx: &egui::Context, data: &mut DMAppData, tree: &mut Tree<DMTab>) {
        let open = data.temp_state.window_states.entry("chat_window".to_owned()).or_insert(false);
        let prev_open = open.clone();
        let mut temp_open = open.clone();
        egui::Window::new(data.get_chat_title())
            .id("chat_window".into())
            .collapsible(true)
            .vscroll(true)
            .resizable(true)
            .default_pos((20.0, 20.0))
            .open(&mut temp_open)
            .show(ctx, |ui| {
                data.temp_state.unread_messages = 0;
                chat(ui, data);
            });
        if prev_open && !temp_open {
            if tree.find_tab(&DMTab::Chat).is_none() {
                tree.push_to_focused_leaf(DMTab::Chat);
            }
        }
        data.temp_state.window_states.insert("chat_window".to_owned(), temp_open);
    }

    /// Top bar (network info mostly).
    fn top_bar(ctx: &egui::Context, ui: &mut Ui, data: &mut DMAppData) {
        ui.horizontal(|ui| {
            if data.host_addr.is_none() {
                if ui.button("Host").clicked() {
                    if let Ok(ip) = local_ip_address::local_ip() {
                        data.host_addr = Some(SocketAddr::new(ip, data.host_port));
                    }
                }
                ui.add(egui::DragValue::new(&mut data.host_port).prefix("Port: "));
            }
            match data.host_addr {
                Some(ip) => {
                    ui.label(format!("{}", ep::WIFI_HIGH));
                    ui.label(RichText::new(ip.to_string()).weak());
                    if ui.add(egui::Button::new(format!("{}", ep::COPY)).small().frame(false)).on_hover_text("Copy").clicked() {
                        ctx.output_mut(|output| output.copied_text = ip.to_string());
                    }
                },
                None => {
                    ui.label(format!("{}", ep::WIFI_SLASH))
                        .on_hover_text("Not currently hosting");
                },
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(format!("{}", ep::SIGN_OUT)).small().frame(false)).clicked() {
                    data.temp_state.window_states.insert("exit_are_you_sure".to_owned(), true);
                }
            });
        });
    }

    fn exit_are_you_sure(ctx: &egui::Context, frame: &mut eframe::Frame, data: &mut DMAppData) {
        egui::Window::new("exit_window")
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, (0.0, 0.0))
            .fixed_size([150.0, 150.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if ui.button(RichText::new("Save and exit").size(14.0)).clicked() {
                        frame.close();
                    }
                    if ui.button(RichText::new("Exit without saving").size(14.0).color(ui.visuals().error_fg_color)).on_hover_text("Are you sure? Everything since the last save will be lost!").clicked() {
                        data.temp_state.exit_without_saving = true;
                        frame.close();
                    }
                    ui.add_space(3.0);
                    if ui.add(egui::Button::new(RichText::new("Wait, go back!").italics()).frame(false)).clicked() {
                        data.temp_state.window_states.insert("exit_are_you_sure".to_owned(), false);
                    }
                });
            });
    }

    fn requests_window(ctx: &egui::Context, data: &mut DMAppData) {
        if !data.temp_state.requests.is_empty() {
            egui::Window::new(format!("Action needed ({}):", data.temp_state.requests.len()))
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .collapsible(false)
                .resizable(false)
                .vscroll(true)
                .show(ctx, |ui| {
                    let mut packets = Vec::new();
                    data.temp_state.requests.retain(|(user, request)| {
                        ui.horizontal(|ui| {
                            ui.label(format!("User \"{}\" requests to: {}", user, request));
                            if ui.small_button(RichText::new(format!("{}", egui_phosphor::CHECK)).color(Color32::GREEN)).clicked() {
                                packets.push((ClientBoundPacket::RespondToRequest(*request, true), user.clone()));
                                return false;
                            }
                            if ui.small_button(RichText::new(format!("{}", egui_phosphor::X)).color(Color32::RED)).clicked() {
                                packets.push((ClientBoundPacket::RespondToRequest(*request, false), user.clone()));
                                return false;
                            }
                            true
                        }).inner
                    });
                    for (packet, user) in packets {
                        data.send_to_user(packet, user);
                    }
                });
        }
    }
}

pub fn chat(ui: &mut Ui, data: &mut DMAppData) {
    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
        let response = ui.text_edit_singleline(&mut data.temp_state.chat);
        if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::ArrowUp) || input.key_pressed(egui::Key::ArrowDown)) {
            data.temp_state.chat = data.temp_state.last_chat.clone();
        }
        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            data.temp_state.last_chat = data.temp_state.chat.clone();
            if data.temp_state.chat.starts_with("/") {
                parse_command(data, data.temp_state.chat.clone());
            } else if !data.temp_state.chat.trim().is_empty() {
                data.temp_state.chat.insert_str(0, "[server]: ");
                data.log_public(data.temp_state.chat.clone());
            }
            data.temp_state.chat.clear();
        }
        for (i, log) in data.logs.iter().enumerate() {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.label(log);
                if i == 0 {
                    ui.separator();
                }
            });
            if i >= 40 {
                break;
            }
        }
    });
}

pub fn parse_command(data: &mut DMAppData, mut command: String) {
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
            "xp" => {
                if let Some(user) = tree.next() {
                    if let Some(user_data) = data.user_data.get_mut(user) {
                        if let Some(name) = tree.next() {
                            if let Some(sheet) = user_data.characters.get_mut(name) {
                                if let Some(token) = tree.next() {
                                    if let Ok(amount) = token.parse::<u32>() {
                                        sheet.add_xp(amount);
                                        let sheet = sheet.clone();
                                        data.send_to_user(ClientBoundPacket::UpdateCharacter(name.to_owned(), sheet), user.to_owned());
                                        data.log_private("Added XP");
                                    } else {
                                        data.log_private(format!("The token \"{}\" could not be interpreted as a number.", token));
                                    }
                                } else {
                                    data.log_private("You must specify an amount of XP to add.");
                                }
                            } else {
                                data.log_private(format!("The character \"{}\" does not exist.", name));
                            }
                        } else {
                            data.log_private("You must specify a character. Make sure to wrap their name in \"quotes\".");
                        }
                    } else {
                        data.log_private(format!("The user \"{}\" does not exist.", user));
                    }
                } else {
                    data.log_private("You must specify a user. Make sure to wrap their name in \"quotes\".");
                }
            },
            "r" | "roll" => {
                if let Some(token) = tree.next() {
                    let mut public = false;
                    let s: Option<&str>;
                    match token {
                        "pub" | "public" => {
                            public = true;
                            s = tree.next();
                        },
                        "priv" | "private" => {
                            public = false;
                            s = tree.next();
                        }
                        token => {
                            s = Some(token);
                        },
                    }
                    if let Some(s) = s {
                        match DiceRoll::from_notation(s) {
                            Ok(roll) => {
                                if public {
                                    data.log_public(format!("[server]({}): {}", ep::DICE_SIX, roll.roll()));
                                } else {
                                    data.log_private(format!("[server]({}): {}", ep::DICE_SIX, roll.roll()));
                                }
                            },
                            Err(e) => {
                                data.log_private(format!("Error: {}", e));
                            },
                        }
                    } else {
                        data.log_private("You must enter dice notation. Run /help roll for more info.");
                    }
                } else {
                    data.log_private("You must enter dice notation. Run /help roll for more info.");
                }
            },
            "help" => {
                if let Some(token) = tree.next() {
                    match token {
                        "roll" => {
                            data.log_private("min: Denotes a minimum value, inclusive or exclusive. A '>' symbol, optionally followed by a '=' symbol, then a value. Defaults to >=1.");
                            data.log_private("X: How many dice to drop. Defaults to 1, and cannot be greater than N.");
                            data.log_private("drop: An underscore ('_') followed by an 'h' or 'l'. Denotes to drop one or more dice, either highest or lowest.");
                            data.log_private("A: The modifier value. Mandatory if <op> is present.");
                            data.log_private("op: One of +, -, *, or /. Division is rounded normally by default, but append a 'u' or 'd' to the '/' to round up or down.");
                            data.log_private("&: If present, apply the modifier to each die rather than the sum of all dice.");
                            data.log_private("M: How many sides the dice have. Mandatory.");
                            data.log_private("d: The literal letter \'d\'.");
                            data.log_private("N: Number of dice to roll. Defaults to 1.");
                            data.log_private("(Parentheses) denote optional values.");
                            data.log_private("(N)dM(&)(<op>A)(<drop>(X))(<min>)");
                            data.log_private("Rolls dice using dice notation. <visibility> can be public/pub, private/priv, or absent (defaults private). <string> uses modified dice notation syntax:");
                            data.log_private("/roll <visibility> <string>");
                        },
                        _ => {
                            unknown_command(data);
                        }
                    }
                } else {
                    let mut msg = "List of all known commands (run /help <command> for more info):".to_owned();
                    msg.push_str("\n- help");
                    msg.push_str("\n- kick");
                    msg.push_str("\n- known_users");
                    msg.push_str("\n- players");
                    msg.push_str("\n- save");
                    msg.push_str("\n- load");
                    msg.push_str("\n- xp");
                    msg.push_str("\n- roll");
                    data.log_private(msg);
                }
            },
            _ => {
                unknown_command(data);
            },
        }
    } else {
        unknown_command(data);
    }
}

pub fn unknown_command(data: &mut DMAppData) {
    data.log_private("Unknown command.");
}

pub struct DMTabViewer<'a, F: FnMut(DMTab, bool)> {
    pub callback: &'a mut F,
    pub data: &'a mut DMAppData,
}

impl<'a, F: FnMut(DMTab, bool) + 'a> DMTabViewer<'a, F> {
    fn dice_roller(ui: &mut Ui, data: &mut DMAppData) {
        ui.vertical(|ui| {
            let advanced = data.temp_state.dice_roll_advanced;
            if ui.checkbox(&mut data.temp_state.dice_roll_advanced, format!("{}", ep::LIST_PLUS))
                .on_hover_text(if advanced {"Showing advanced options"} else {"Hiding advanced options"})
                .clicked() {
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
                if ui.button(RichText::new("Roll!").strong()).clicked() {
                    let r = roll(data.temp_state.dice_roll);
                    if data.temp_state.dice_roll_public {
                        data.log_public(format!("[server]({}): {}", ep::DICE_SIX, r));
                    } else {
                        data.log_private(format!("[server]({}): {}", ep::DICE_SIX, r));
                    }
                }
                let public = data.temp_state.dice_roll_public;
                ui.checkbox(&mut data.temp_state.dice_roll_public, if public {format!("{}", ep::EYE)} else {format!("{}", ep::EYE_CLOSED)})
                    .on_hover_text(if public {"Roll public"} else {"Roll private"});
            });
        });
    }
    fn player_list(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            let show_offline = self.data.temp_state.show_offline_users;
            ui.checkbox(&mut self.data.temp_state.show_offline_users, format!("{}", if show_offline {ep::CELL_SIGNAL_SLASH} else {ep::CELL_SIGNAL_FULL}))
                .on_hover_text(if show_offline {"Showing offline users"} else {"Hiding offline users"});
            ui.separator();
            let mut users = Vec::new();
            if self.data.temp_state.show_offline_users {
                for (user, _) in &self.data.known_users {
                    users.push(user.clone());
                }
            } else {
                for (user, _) in &self.data.connected_users {
                    users.push(user.clone());
                }
            }
            for user in users {
                self.open_tab_button(ui, format!("User: {}", user), DMTab::Player(user), DMTab::PlayerList);
            }
        });
    }
    fn player_tab(&mut self, ui: &mut Ui, user: &String) {
        ui.vertical(|ui| {
            if back_arrow(ui) {
                (self.callback)(DMTab::Player(user.clone()), false);
                (self.callback)(DMTab::PlayerList, true);
            }
            ui.separator();
            if let Some(user_data) = self.data.user_data.get(user) {
                let mut names = Vec::new();
                for (name, _) in &user_data.characters {
                    names.push(name.clone());
                }
                for name in names {
                    self.open_tab_button(ui, format!("Character: {}", name), DMTab::PlayerCharacter(user.clone(), name), DMTab::Player(user.clone()));
                }
            } else {
                ui.colored_label(ui.visuals().error_fg_color, "Something went wrong. This user doesn't appear to exist!");
            }
        });
    }
    fn player_character(&mut self, ui: &mut Ui, user: &String, name: &String) {
        if back_arrow(ui) {
            (self.callback)(DMTab::PlayerCharacter(user.clone(), name.clone()), false);
            (self.callback)(DMTab::Player(user.clone()), true);
        }
        ui.separator();
        if let Some(user_data) = self.data.user_data.get_mut(user) {
            if let Some(sheet) = user_data.characters.get_mut(name) {
                let tab = user_data.charsheet_tabs.entry(name.clone()).or_insert(CharacterSheetTab::Stats);
                let mut changed = false;
                common_ui::tabs(tab, format!("<{}>_charsheet_tab_<{}>", user, name), ui, |_, _| {}, |ui, tab| {
                    match tab {
                        CharacterSheetTab::Stats => {
                            let attrs = sheet.combat_stats.attributes;
                            ui.label(format!("STR: {} ({:+})", attrs.strength, attrs.modifier(Attr::STR)));
                            ui.label(format!("DEX: {} ({:+})", attrs.dexterity, attrs.modifier(Attr::DEX)));
                            ui.label(format!("CON: {} ({:+})", attrs.constitution, attrs.modifier(Attr::CON)));
                            ui.label(format!("INT: {} ({:+})", attrs.intelligence, attrs.modifier(Attr::INT)));
                            ui.label(format!("WIS: {} ({:+})", attrs.wisdom, attrs.modifier(Attr::WIS)));
                            ui.label(format!("CHA: {} ({:+})", attrs.charisma, attrs.modifier(Attr::CHA)));
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
                                if ui.small_button("Restore").clicked() {
                                    sheet.combat_stats.health.current_hp = sheet.combat_stats.health.max_hp as i32;
                                    changed = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("AC: {}", sheet.combat_stats.armor_class + sheet.combat_stats.modifiers.armor_class.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.armor_class.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Initiative: {:+}", sheet.combat_stats.modifiers.initiative.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.initiative.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Surprise: {:+}", sheet.combat_stats.modifiers.surprise.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.surprise.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                            ui.label(format!("ATK: {:+}", sheet.combat_stats.attack_throw));
                            ui.label(format!("Base damage: {}", sheet.combat_stats.damage.display()));
                            ui.horizontal(|ui| {
                                ui.label(format!("Melee ATK bonus: {:+}", sheet.combat_stats.modifiers.melee_attack.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.melee_attack.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Missile ATK bonus: {:+}", sheet.combat_stats.modifiers.missile_attack.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.missile_attack.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Melee DMG bonus: {:+}", sheet.combat_stats.modifiers.melee_damage.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.melee_damage.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Missile DMG bonus: {:+}", sheet.combat_stats.modifiers.missile_damage.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.missile_damage.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                            ui.separator();
                            let saves = sheet.combat_stats.saving_throws;
                            ui.label("Saving throws:");
                            ui.horizontal(|ui| {
                                ui.label(format!("Petrification & Paralysis: {:+}", saves.petrification_paralysis + sheet.combat_stats.modifiers.save_petrification_paralysis.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.save_petrification_paralysis.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Poison & Death: {:+}", saves.poison_death + sheet.combat_stats.modifiers.save_poison_death.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.save_poison_death.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Blast & Breath: {:+}", saves.blast_breath + sheet.combat_stats.modifiers.save_blast_breath.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.save_blast_breath.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Staffs & Wands: {:+}", saves.staffs_wands + sheet.combat_stats.modifiers.save_staffs_wands.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.save_staffs_wands.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Spells: {:+}", saves.spells + sheet.combat_stats.modifiers.save_spells.total()));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.save_spells.view_all() {
                                        ui.label(format!("{}: {:+}", id, amount));
                                    }
                                });
                            });
                        },
                        CharacterSheetTab::Class => {
                            ui.label(format!("Class: {}", sheet.class.name));
                            ui.label(RichText::new(&sheet.class.description).italics().weak());
                            ui.label(format!("Title: {}", sheet.title));
                            ui.label(format!("Race: {}", sheet.race));
                            ui.label(format!("Level: {}", sheet.level));
                            ui.horizontal(|ui| {
                                ui.label(format!("XP: {}/{} ({:+.1}%)", sheet.xp, sheet.xp_to_level, sheet.combat_stats.modifiers.xp_gain.total() * 100.0));
                                ui.menu_button("...", |ui| {
                                    ui.strong("Modifiers");
                                    ui.separator();
                                    for (id, amount) in sheet.combat_stats.modifiers.xp_gain.view_all() {
                                        ui.label(format!("{}: {:+.1}%", id, amount * 100.0));
                                    }
                                });
                            });
                            ui.label(format!("Hit Die: {}", sheet.class.hit_die));
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
                    let sheet = sheet.clone();
                    self.data.send_to_user(ClientBoundPacket::UpdateCharacter(name.clone(), sheet), user.clone());
                }
            } else {
                ui.colored_label(ui.visuals().error_fg_color, "Something went wrong. This character doesn't appear to exist!");
            }
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Something went wrong. This user doesn't appear to exist!");
        }
    }
    fn open_tab_button(&mut self, ui: &mut Ui, text: impl Into<WidgetText>, tab_to_open: DMTab, current_tab: DMTab) {
        let response = ui.button(text);
        if (ui.input(|i| i.modifiers.shift) && response.clicked()) || response.clicked_by(egui::PointerButton::Middle) {
            (self.callback)(tab_to_open, true);
        } else if response.clicked() {
            (self.callback)(tab_to_open, true);
            (self.callback)(current_tab, false);
        }
    }
    fn chat_tab(&mut self, ui: &mut Ui) {
        self.data.temp_state.unread_messages = 0;
        chat(ui, self.data);
    }
    fn enemy_viewer(ui: &mut Ui, data: &mut DMAppData) {
        let mut go_back = false;
        match &mut data.temp_state.viewed_enemy {
            Some(path) => {
                match data.enemy_type_registry.get(path) {
                    Some(node) => {
                        match node {
                            RegistryNode::Value(enemy) => {
                                ui.horizontal(|ui| {
                                    if back_arrow(ui) {
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
                                    if back_arrow(ui) {
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
    }
    fn deployed_enemies(ui: &mut Ui, data: &mut DMAppData) {
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
    }
    fn enemy_creator(ui: &mut Ui, data: &mut DMAppData) {
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
    }
    fn item_viewer(ui: &mut Ui, data: &mut DMAppData) {
        let mut packets: Vec<(ClientBoundPacket, String)> = Vec::new();
        let mut go_back = false;
        match &mut data.temp_state.viewed_item {
            Some(path) => {
                match data.item_type_registry.get(path) {
                    Some(node) => {
                        match node {
                            RegistryNode::Value(item) => {
                                ui.horizontal(|ui| {
                                    if back_arrow(ui) {
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
                                    if back_arrow(ui) {
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
    }
    fn item_creator(ui: &mut Ui, data: &mut DMAppData) {
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
    }
    fn prof_viewer(ui: &mut Ui, data: &mut DMAppData) {
        let mut go_back = false;
        let mut packets = Vec::new();
        if let Some(id) = &data.temp_state.viewed_prof {
            if let Some(prof) = data.proficiency_registry.get(id) {
                ui.horizontal(|ui| {
                    if back_arrow(ui) {
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
    }
    fn prof_creator(ui: &mut Ui, data: &mut DMAppData) {
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
    }
    fn spell_viewer(ui: &mut Ui, data: &mut DMAppData) {
        let mut go_back = false;
        match &mut data.temp_state.viewed_spell {
            Some((typ, maybe_lvl)) => {
                if back_arrow(ui) {
                    go_back = true;
                }
                ui.separator();
                match typ {
                    MagicType::Arcane => {
                        match maybe_lvl {
                            Some((lvl, maybe_spell)) => {
                                match maybe_spell {
                                    Some(spell) => {
                                        if let Some(arcane) = data.spell_registry.arcane.get(*lvl as usize) {
                                            if let Some(spell) = arcane.get(spell) {
                                                Self::display_spell(ui, spell);
                                            } else {
                                                go_back = true;
                                            }
                                        } else {
                                            go_back = true;
                                        }
                                    },
                                    None => {
                                        if *lvl < 9 {
                                            for (id, spell) in &data.spell_registry.arcane[*lvl as usize] {
                                                if ui.button(&spell.name).clicked() {
                                                    *maybe_spell = Some(id.clone());
                                                }
                                            }
                                        } else {
                                            *maybe_lvl = None;
                                        }
                                    },
                                }
                            },
                            None => {
                                for i in 0u8..9u8 {
                                    if ui.button(format!("Level {} ({})", i + 1, data.spell_registry.arcane.get(i as usize).map_or(0, |s| s.len()))).clicked() {
                                        *maybe_lvl = Some((i, None));
                                    }
                                }
                            },
                        }
                    },
                    MagicType::Divine => {
                        match maybe_lvl {
                            Some((lvl, maybe_spell)) => {
                                match maybe_spell {
                                    Some(spell) => {
                                        if let Some(divine) = data.spell_registry.divine.get(*lvl as usize) {
                                            if let Some(spell) = divine.get(spell) {
                                                Self::display_spell(ui, spell);
                                            } else {
                                                go_back = true;
                                            }
                                        } else {
                                            go_back = true;
                                        }
                                    },
                                    None => {
                                        if *lvl < 7 {
                                            for (id, spell) in &data.spell_registry.divine[*lvl as usize] {
                                                if ui.button(&spell.name).clicked() {
                                                    *maybe_spell = Some(id.clone());
                                                }
                                            }
                                        } else {
                                            *maybe_lvl = None;
                                        }
                                    },
                                }
                            },
                            None => {
                                for i in 0u8..7u8 {
                                    if ui.button(format!("Level {} ({})", i + 1, data.spell_registry.divine.get(i as usize).map_or(0, |s| s.len()))).clicked() {
                                        *maybe_lvl = Some((i, None));
                                    }
                                }
                            },
                        }
                    },
                }
            },
            None => {
                if ui.button("Arcane").clicked() {
                    data.temp_state.viewed_spell = Some((MagicType::Arcane, None));
                }
                if ui.button("Divine").clicked() {
                    data.temp_state.viewed_spell = Some((MagicType::Divine, None));
                }
            },
        }
        if go_back {
            if let Some((_, maybe_lvl)) = &mut data.temp_state.viewed_spell {
                if let Some((_, maybe_spell)) = maybe_lvl {
                    if maybe_spell.is_some() {
                        *maybe_spell = None;
                    } else {
                        *maybe_lvl = None;
                    }
                } else {
                    data.temp_state.viewed_spell = None;
                }
            }
        }
    }
    fn display_spell(ui: &mut Ui, spell: &Spell) {
        ui.heading(&spell.name);
        ui.label(format!("{} {}{}", spell.magic_type, spell.spell_level + 1, if spell.reversed.is_some() {" (Reversible)"} else {""}));
        ui.label(format!("Range: {}", spell.range));
        ui.label(format!("Duration: {}", spell.duration));
        ui.separator();
        ui.label(RichText::new(&spell.description).weak().italics());
    }
    fn spell_creator(ui: &mut Ui, data: &mut DMAppData) {
        if let Some(spell) = &mut data.temp_state.temp_spell {
            ui.label("Name:");
            ui.text_edit_singleline(&mut spell.name);
            ui.label("Description:");
            ui.text_edit_multiline(&mut spell.description);
            egui::ComboBox::from_label("Magic Type")
                .selected_text(spell.magic_type.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut spell.magic_type, MagicType::Arcane, "Arcane");
                    ui.selectable_value(&mut spell.magic_type, MagicType::Divine, "Divine");
                });
            let mut temp_level = spell.spell_level + 1;
            ui.add(egui::Slider::new(&mut temp_level, 1..=9).clamp_to_range(true).text("Spell Level"));
            spell.spell_level = temp_level - 1;
            egui::ComboBox::from_label("Range")
                .selected_text(spell.range.display())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut spell.range, SpellRange::OnSelf, "Self");
                    ui.selectable_value(&mut spell.range, SpellRange::Touch, "Touch");
                    ui.selectable_value(&mut spell.range, SpellRange::Feet(0), "Feet (specify)");
                    ui.selectable_value(&mut spell.range, SpellRange::RadiusFeet(0), "Feet Radius (specify)");
                    ui.selectable_value(&mut spell.range, SpellRange::Unlimited, "Unlimited");
                    ui.selectable_value(&mut spell.range, SpellRange::Special, "Special");
                });
            match &mut spell.range {
                SpellRange::Feet(feet) => {
                    ui.add(egui::Slider::new(feet, 0..=360).clamp_to_range(false).text("Range (feet)"));
                },
                SpellRange::RadiusFeet(feet) => {
                    ui.add(egui::Slider::new(feet, 0..=360).clamp_to_range(false).text("Radius (feet)"));
                },
                _ => {},
            }
            egui::ComboBox::from_label("Duration")
                .selected_text(spell.duration.display())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut spell.duration, SpellDuration::Instant, "Instantaneous");
                    ui.selectable_value(&mut spell.duration, SpellDuration::Rounds(1), "Rounds (specify)");
                    ui.selectable_value(&mut spell.duration, SpellDuration::RoundsPerLevel(1), "Rounds per Level (specify)");
                    ui.selectable_value(&mut spell.duration, SpellDuration::Turns(1), "Turns (specify)");
                    ui.selectable_value(&mut spell.duration, SpellDuration::TurnsPerLevel(1), "Turns per Level (specify)");
                    ui.selectable_value(&mut spell.duration, SpellDuration::Days(1), "Days (specify)");
                    ui.selectable_value(&mut spell.duration, SpellDuration::DaysPerLevel(1), "Days per Level (specify)");
                    ui.selectable_value(&mut spell.duration, SpellDuration::Concentration, "Concentration");
                    ui.selectable_value(&mut spell.duration, SpellDuration::Permanent, "Permanent");
                    ui.selectable_value(&mut spell.duration, SpellDuration::Special, "Special");
                });
            match &mut spell.duration {
                SpellDuration::Rounds(dur) => {
                    ui.add(egui::Slider::new(dur, 1..=20).clamp_to_range(false).text("Rounds"));
                },
                SpellDuration::RoundsPerLevel(dur) => {
                    ui.add(egui::Slider::new(dur, 1..=20).clamp_to_range(false).text("Rounds"));
                },
                SpellDuration::Turns(dur) => {
                    ui.add(egui::Slider::new(dur, 1..=20).clamp_to_range(false).text("Turns"));
                },
                SpellDuration::TurnsPerLevel(dur) => {
                    ui.add(egui::Slider::new(dur, 1..=20).clamp_to_range(false).text("Turns"));
                },
                SpellDuration::Days(dur) => {
                    ui.add(egui::Slider::new(dur, 1..=20).clamp_to_range(false).text("Days"));
                },
                SpellDuration::DaysPerLevel(dur) => {
                    ui.add(egui::Slider::new(dur, 1..=20).clamp_to_range(false).text("Days"));
                },
                _ => {},
            }
            let mut temp = spell.reversed.is_some();
            if ui.checkbox(&mut temp, "Reversible").clicked() {
                if spell.reversed.take().is_none() {
                    spell.reversed = Some(String::new());
                }
            }
            if let Some(reversed) = &mut spell.reversed {
                ui.label("Reversed form (spell ID):");
                ui.text_edit_singleline(reversed);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Save as:");
                ui.text_edit_singleline(&mut data.temp_state.temp_spell_filename);
            });
            ui.label(RichText::new("Hint: you can use \"/ \" to specify a folder.").weak().italics());
            if ui.button("Save").clicked() {
                if let Ok(_) = save_ron(spell, "spells", data.temp_state.temp_spell_filename.trim()) {
                    data.temp_state.temp_spell_filename = "spell".to_owned();
                    data.temp_state.temp_spell = None;
                    data.register_spells();
                }
            }
        } else {
            if ui.button("Create new").clicked() {
                data.temp_state.temp_spell = Some(Spell::default());
            } 
        }
    }
    fn class_viewer(ui: &mut Ui, data: &mut DMAppData) {
        let mut go_back = false;
        match &mut data.temp_state.viewed_class {
            Some(path) => {
                match data.class_registry.get(path) {
                    Some(node) => {
                        match node {
                            RegistryNode::Value(class) => {
                                ui.horizontal(|ui| {
                                    if back_arrow(ui) {
                                        go_back = true;
                                    }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button("Edit").clicked() {
                                            let mut profs = String::new();
                                            for (i, (prof, specs)) in class.class_proficiencies.iter().enumerate() {
                                                if i == 0 {
                                                    profs.push_str(prof);
                                                } else {
                                                    profs.push_str(&format!(", {}", prof));
                                                }
                                                if let Some(specs) = specs {
                                                    let mut specs_str = "(".to_owned();
                                                    for (j, spec) in specs.iter().enumerate() {
                                                        if j == 0 {
                                                            specs_str.push_str(spec);
                                                        } else {
                                                            specs_str.push_str(&format!(", {}", spec));
                                                        }
                                                    }
                                                    specs_str.push(')');
                                                    profs.push_str(&specs_str);
                                                }
                                            }
                                            data.temp_state.temp_class_profs = profs;
                                            data.temp_state.temp_class = Some(class.clone());
                                            data.temp_state.temp_class_filename = path.clone();
                                            data.temp_state.window_states.insert("class_creator".to_owned(), true);
                                        }
                                    });
                                });
                                ui.separator();
                                ui.heading(&class.name);
                                ui.label(RichText::new(&class.description).weak().italics());
                                ui.separator();
                                ui.label(format!("Race: {}", class.race));
                                let mut list = "Prime Requisite(s): ".to_owned();
                                for (i, attr) in class.prime_reqs.iter().enumerate() {
                                    if i == 0 {
                                        list.push_str(&format!("{}", attr));
                                    } else {
                                        list.push_str(&format!(", {}", attr));
                                    }
                                }
                                ui.label(list);
                                ui.label(format!("Maximum Level: {}", class.maximum_level));
                                ui.label(format!("Hit Die: {}", class.hit_die));
                                ui.label(format!("XP to 2nd level: {}", class.base_xp_cost));
                                ui.separator();
                                ui.add_space(3.0);
                                ui.label(class.fighting_styles.display(&class.name));
                                ui.add_space(3.0);
                                ui.label(class.weapon_selection.display());
                                ui.add_space(3.0);
                                ui.label(class.armor_selection.display());
                                match class.class_damage_bonus {
                                    ClassDamageBonus::Both => {
                                        ui.add_space(3.0);
                                        ui.label(format!("The {} class deals +1 damage at level 1, and another +1 every third level.", class.name));
                                    },
                                    ClassDamageBonus::MeleeOnly => {
                                        ui.add_space(3.0);
                                        ui.label(format!("The {} class deals +1 melee damage at level 1, and another +1 every third level.", class.name));
                                    },
                                    ClassDamageBonus::MissileOnly => {
                                        ui.add_space(3.0);
                                        ui.label(format!("The {} class deals +1 missile (ranged) damage at level 1, and another +1 every third level.", class.name));
                                    },
                                    _ => {},
                                }
                                match class.cleaves {
                                    Cleaves::Full => {
                                        ui.add_space(3.0);
                                        ui.label("They may cleave once per level of experience.");
                                    },
                                    Cleaves::Half => {
                                        ui.add_space(3.0);
                                        ui.label("They may cleave once per half their level, rounded down.");
                                    },
                                    _ => {},
                                }
                                if !class.thief_skills.0.is_empty() {
                                    ui.add_space(3.0);
                                    ui.label(class.thief_skills.display(&class.name));
                                }
                                ui.separator();
                                ui.label("Class Proficiencies:");
                                let mut list = String::new();
                                for (i, (prof, specs)) in class.class_proficiencies.iter().enumerate() {
                                    if let Some(prof) = data.proficiency_registry.get(prof) {
                                        if i == 0 {
                                            list.push_str(&prof.name);
                                        } else {
                                            list.push_str(&format!(", {}", prof.name));
                                        }
                                        if let Some(specs) = specs {
                                            list.push('(');
                                            for (j, spec) in specs.iter().enumerate() {
                                                if j == 0 {
                                                    list.push_str(spec);
                                                } else {
                                                    list.push_str(&format!(", {}", spec));
                                                }
                                            }
                                            list.push(')');
                                        }
                                    }
                                }
                                ui.label(list);
                            },
                            RegistryNode::SubRegistry(map) => {
                                ui.horizontal(|ui| {
                                    if back_arrow(ui) {
                                        go_back = true;
                                    }
                                });
                                ui.separator();
                                if map.is_empty() {
                                    ui.label(RichText::new("There\'s nothing here...").weak().italics());
                                }
                                for (subpath, subnode) in map {
                                    match subnode {
                                        RegistryNode::Value(class) => {
                                            if ui.button(format!("View: {}", class.name)).clicked() {
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
                        data.temp_state.viewed_class = None;
                    },
                }
            },
            None => {
                if data.class_registry.tree.is_empty() {
                    ui.label(RichText::new("There\'s nothing here...").weak().italics());
                }
                for (path, node) in &data.class_registry.tree {
                    match node {
                        RegistryNode::Value(class) => {
                            if ui.button(format!("View: {}", class.name)).clicked() {
                                data.temp_state.viewed_class = Some(path.clone());
                            }
                        },
                        RegistryNode::SubRegistry(_) => {
                            if ui.button(format!("Folder: {}", path)).clicked() {
                                data.temp_state.viewed_class = Some(path.clone());
                            }
                        },
                    }
                }
            },
        }
        if go_back {
            if let Some(path) = &mut data.temp_state.viewed_class {
                data.temp_state.viewed_class = path.rsplit_once("/").map(|(s, _)| s.to_owned());
            }
        }
    }
    fn class_creator(ui: &mut Ui, data: &mut DMAppData) {
        if let Some(class) = &mut data.temp_state.temp_class {
            ui.label("Name:");
            ui.text_edit_singleline(&mut class.name);
            ui.label("Description:");
            ui.text_edit_multiline(&mut class.description);
            egui::ComboBox::from_label("Race")
                .selected_text(class.race.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut class.race, Race::Human, "Human");
                    ui.selectable_value(&mut class.race, Race::Dwarf, "Dwarf");
                    ui.selectable_value(&mut class.race, Race::Elf, "Elf");
                    ui.selectable_value(&mut class.race, Race::Halfling, "Halfling");
                    ui.selectable_value(&mut class.race, Race::Gnome, "Gnome");
                    ui.selectable_value(&mut class.race, Race::Zaharan, "Zaharan");
                    ui.selectable_value(&mut class.race, Race::Thrassian, "Thrassian");
                    ui.selectable_value(&mut class.race, Race::Nobiran, "Nobiran");
                });
            ui.label("Prime Requisites:");
            ui.horizontal(|ui| {
                if ui.add(egui::SelectableLabel::new(class.prime_reqs.contains(&Attr::STR), "STR")).clicked() {
                    if !class.prime_reqs.remove(&Attr::STR) {
                        class.prime_reqs.insert(Attr::STR);
                    }
                }
                if ui.add(egui::SelectableLabel::new(class.prime_reqs.contains(&Attr::DEX), "DEX")).clicked() {
                    if !class.prime_reqs.remove(&Attr::DEX) {
                        class.prime_reqs.insert(Attr::DEX);
                    }
                }
                if ui.add(egui::SelectableLabel::new(class.prime_reqs.contains(&Attr::CON), "CON")).clicked() {
                    if !class.prime_reqs.remove(&Attr::CON) {
                        class.prime_reqs.insert(Attr::CON);
                    }
                }
                if ui.add(egui::SelectableLabel::new(class.prime_reqs.contains(&Attr::INT), "INT")).clicked() {
                    if !class.prime_reqs.remove(&Attr::INT) {
                        class.prime_reqs.insert(Attr::INT);
                    }
                }
                if ui.add(egui::SelectableLabel::new(class.prime_reqs.contains(&Attr::WIS), "WIS")).clicked() {
                    if !class.prime_reqs.remove(&Attr::WIS) {
                        class.prime_reqs.insert(Attr::WIS);
                    }
                }
                if ui.add(egui::SelectableLabel::new(class.prime_reqs.contains(&Attr::CHA), "CHA")).clicked() {
                    if !class.prime_reqs.remove(&Attr::CHA) {
                        class.prime_reqs.insert(Attr::CHA);
                    }
                }
            });
            egui::ComboBox::from_label("Hit Die")
                .selected_text(class.hit_die.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut class.hit_die, HitDie::D4, "D4");
                    ui.selectable_value(&mut class.hit_die, HitDie::D6, "D6");
                    ui.selectable_value(&mut class.hit_die, HitDie::D8, "D8");
                    ui.selectable_value(&mut class.hit_die, HitDie::D10, "D10");
                    ui.selectable_value(&mut class.hit_die, HitDie::D12, "D12");
                });
            ui.separator();
            ui.add(egui::Slider::new(&mut class.maximum_level, 1..=14).clamp_to_range(true).text("Max Level"));
            ui.label("Titles:");
            for level in 1..=class.maximum_level {
                ui.horizontal(|ui| {
                    ui.label(format!("{}:", level));
                    if let Some(title) = class.titles.0.get_mut(level as usize - 1) {
                        ui.text_edit_singleline(title);
                    } else {
                        class.titles.0.push(String::new());
                    }
                });
            }
            ui.separator();
            ui.add(egui::Slider::new(&mut class.base_xp_cost, 0..=4000).clamp_to_range(false).text("XP to second level"));
            egui::ComboBox::from_label("Saving Throws")
                .selected_text(class.saving_throw_progression_type.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut class.saving_throw_progression_type, SavingThrowProgressionType::Fighter, "Fighter");
                    ui.selectable_value(&mut class.saving_throw_progression_type, SavingThrowProgressionType::Thief, "Thief");
                    ui.selectable_value(&mut class.saving_throw_progression_type, SavingThrowProgressionType::Cleric, "Cleric");
                    ui.selectable_value(&mut class.saving_throw_progression_type, SavingThrowProgressionType::Mage, "Mage");
                });
            ui.separator();
            ui.label("Class Proficiencies:");
            ui.label(RichText::new("Enter a comma-separated list of prof IDs (e.g. acrobatics, weapon_finesse, combat_trickery(Disarm), elementalism(Air, Earth)).").weak().italics());
            ui.text_edit_multiline(&mut data.temp_state.temp_class_profs);
            ui.separator();
            egui::ComboBox::from_label("Attack Throw Progression")
                .selected_text(class.attack_throw_progression.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut class.attack_throw_progression, AttackThrowProgression::OnePerThree, AttackThrowProgression::OnePerThree.to_string());
                    ui.selectable_value(&mut class.attack_throw_progression, AttackThrowProgression::OnePerTwo, AttackThrowProgression::OnePerTwo.to_string());
                    ui.selectable_value(&mut class.attack_throw_progression, AttackThrowProgression::TwoPerThree, AttackThrowProgression::TwoPerThree.to_string());
                    ui.selectable_value(&mut class.attack_throw_progression, AttackThrowProgression::OnePerOne, AttackThrowProgression::OnePerOne.to_string());
                    ui.selectable_value(&mut class.attack_throw_progression, AttackThrowProgression::ThreePerTwo, AttackThrowProgression::ThreePerTwo.to_string());
                });
            egui::ComboBox::from_label("Weapon Selection")
                .selected_text(class.weapon_selection.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut class.weapon_selection, WeaponSelection::Unrestricted, "Unrestricted");
                    ui.selectable_value(&mut class.weapon_selection, WeaponSelection::Broad([BroadWeapons::OneHanded, BroadWeapons::TwoHanded]), "Broad");
                    ui.selectable_value(&mut class.weapon_selection, WeaponSelection::Narrow([NarrowWeapons::Axes, NarrowWeapons::SwordsDaggers]), "Narrow");
                    ui.selectable_value(&mut class.weapon_selection, WeaponSelection::Restricted([RestrictedWeapons::Club, RestrictedWeapons::Dagger, RestrictedWeapons::Dart, RestrictedWeapons::Staff]), "Restricted");
                });
            match &mut class.weapon_selection {
                WeaponSelection::Unrestricted => {},
                WeaponSelection::Broad(broad) => {
                    for (i, weapon) in broad.iter_mut().enumerate() {
                        egui::ComboBox::from_label(format!("{}", i))
                            .selected_text(weapon.display())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(weapon, BroadWeapons::OneHanded, BroadWeapons::OneHanded.display());
                                ui.selectable_value(weapon, BroadWeapons::TwoHanded, BroadWeapons::TwoHanded.display());
                                ui.selectable_value(weapon, BroadWeapons::AxesFlailsHammersMaces, BroadWeapons::AxesFlailsHammersMaces.display());
                                ui.selectable_value(weapon, BroadWeapons::SwordsDaggersSpearsPolearms, BroadWeapons::SwordsDaggersSpearsPolearms.display());
                                ui.selectable_value(weapon, BroadWeapons::Missile, BroadWeapons::Missile.display());
                                ui.selectable_value(weapon, BroadWeapons::AnyFive(String::new(), String::new(), String::new(), String::new(), String::new()), "Any Five Weapons");
                            });
                        if let BroadWeapons::AnyFive(w1, w2, w3, w4, w5) = weapon {
                            ui.text_edit_singleline(w1);
                            ui.text_edit_singleline(w2);
                            ui.text_edit_singleline(w3);
                            ui.text_edit_singleline(w4);
                            ui.text_edit_singleline(w5);
                        }
                    }
                    if broad[0] == broad[1] {
                        ui.colored_label(ui.visuals().error_fg_color, "Warning: overlapping weapon selection!");
                    }
                },
                WeaponSelection::Narrow(narrow) => {
                    for (i, weapon) in narrow.iter_mut().enumerate() {
                        egui::ComboBox::from_label(format!("{}", i))
                            .selected_text(weapon.display())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(weapon, NarrowWeapons::Axes, NarrowWeapons::Axes.display());
                                ui.selectable_value(weapon, NarrowWeapons::BowsCrossbows, NarrowWeapons::BowsCrossbows.display());
                                ui.selectable_value(weapon, NarrowWeapons::FlailsHammersMaces, NarrowWeapons::FlailsHammersMaces.display());
                                ui.selectable_value(weapon, NarrowWeapons::SwordsDaggers, NarrowWeapons::SwordsDaggers.display());
                                ui.selectable_value(weapon, NarrowWeapons::SpearsPolearms, NarrowWeapons::SpearsPolearms.display());
                                ui.selectable_value(weapon, NarrowWeapons::Special, NarrowWeapons::Special.display());
                                ui.selectable_value(weapon, NarrowWeapons::AnyThree(String::new(), String::new(), String::new()), "Any Three Weapons");
                            });
                        if let NarrowWeapons::AnyThree(w1, w2, w3) = weapon {
                            ui.text_edit_singleline(w1);
                            ui.text_edit_singleline(w2);
                            ui.text_edit_singleline(w3);
                        }
                    }
                    if narrow[0] == narrow[1] {
                        ui.colored_label(ui.visuals().error_fg_color, "Warning: overlapping weapon selection!");
                    } 
                },
                WeaponSelection::Restricted(res) => {
                    for (i, weapon) in res.iter_mut().enumerate() {
                        egui::ComboBox::from_label(format!("{}", i))
                            .selected_text(weapon.display())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(weapon, RestrictedWeapons::Bola, "Bolas");
                                ui.selectable_value(weapon, RestrictedWeapons::Club, "Clubs");
                                ui.selectable_value(weapon, RestrictedWeapons::Dagger, "Daggers");
                                ui.selectable_value(weapon, RestrictedWeapons::Dart, "Darts");
                                ui.selectable_value(weapon, RestrictedWeapons::Sap, "Saps");
                                ui.selectable_value(weapon, RestrictedWeapons::Sling, "Slings");
                                ui.selectable_value(weapon, RestrictedWeapons::Staff, "Staves");
                                ui.selectable_value(weapon, RestrictedWeapons::Whip, "Whips");
                            });
                    }
                    let mut overlap = false;
                    for (i, weapon) in res.iter().enumerate() {
                        if i != 0 {
                            if res[0] == *weapon {
                                overlap = true;
                            }
                        }
                        if i != 1 {
                            if res[1] == *weapon {
                                overlap = true;
                            }
                        }
                        if i != 2 {
                            if res[2] == *weapon {
                                overlap = true;
                            }
                        }
                        if i != 3 {
                            if res[3] == *weapon {
                                overlap = true;
                            }
                        }
                    }
                    if overlap {
                        ui.colored_label(ui.visuals().error_fg_color, "Warning: overlapping weapon selection!");
                    }
                },
            }
            egui::ComboBox::from_label("Armor Selection")
                .selected_text(class.armor_selection.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut class.armor_selection, ArmorSelection::Unrestricted, ArmorSelection::Unrestricted.to_string());
                    ui.selectable_value(&mut class.armor_selection, ArmorSelection::Broad, ArmorSelection::Broad.to_string());
                    ui.selectable_value(&mut class.armor_selection, ArmorSelection::Narrow, ArmorSelection::Narrow.to_string());
                    ui.selectable_value(&mut class.armor_selection, ArmorSelection::Restricted, ArmorSelection::Restricted.to_string());
                    ui.selectable_value(&mut class.armor_selection, ArmorSelection::Forbidden, ArmorSelection::Forbidden.to_string());
                });
            ui.label("Allowed Fighting Styles:");
            ui.horizontal(|ui| {
                ui.checkbox(&mut class.fighting_styles.two_weapons, "Two Weapons");
                ui.checkbox(&mut class.fighting_styles.weapon_and_shield, "Weapon and Shield");
                ui.checkbox(&mut class.fighting_styles.two_handed, "Two-Handed");
            });
            egui::ComboBox::from_label("Damage Bonus")
                .selected_text(class.class_damage_bonus.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut class.class_damage_bonus, ClassDamageBonus::None, "None");
                    ui.selectable_value(&mut class.class_damage_bonus, ClassDamageBonus::MeleeOnly, "Only Melee");
                    ui.selectable_value(&mut class.class_damage_bonus, ClassDamageBonus::MissileOnly, "Only Missile");
                    ui.selectable_value(&mut class.class_damage_bonus, ClassDamageBonus::Both, "Both");
                });
            egui::ComboBox::from_label("Cleaves")
                .selected_text(class.cleaves.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut class.cleaves, Cleaves::None, "None");
                    ui.selectable_value(&mut class.cleaves, Cleaves::Half, "Half");
                    ui.selectable_value(&mut class.cleaves, Cleaves::Full, "Full");
                });
            ui.separator();
            ui.label("Thief Skills:");
            for skill in THIEF_SKILLS {
                if ui.add(egui::SelectableLabel::new(class.thief_skills.0.contains(&skill), skill.to_string())).clicked() {
                    if !class.thief_skills.0.remove(&skill) {
                        class.thief_skills.0.insert(skill);
                    }
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Save as:");
                ui.text_edit_singleline(&mut data.temp_state.temp_class_filename);
            });
            ui.label(RichText::new("Hint: you can use \"/ \" to specify a folder.").weak().italics());
            if ui.button("Save").clicked() {
                let mut in_brackets = false;
                for prof in data.temp_state.temp_class_profs.split(|c| {
                    if c == '(' {
                        in_brackets = true;
                    }
                    if c == ')' {
                        in_brackets = false;
                    }
                    if in_brackets {
                        false
                    } else {
                        c == ','
                    }
                }) {
                    let prof = prof.trim();
                    if prof.ends_with(')') {
                        if let Some((prof, specs)) = prof.trim_end_matches(')').split_once('(') {
                            let mut set: HashSet<String> = HashSet::new();
                            for spec in specs.split(',') {
                                set.insert(spec.trim().to_owned());
                            }
                            if !prof.is_empty() && !set.is_empty() {
                                class.class_proficiencies.insert(prof.to_owned(), Some(set));
                            }
                        }
                    } else {
                        if !prof.is_empty() {
                            class.class_proficiencies.insert(prof.to_owned(), None);
                        }
                    }
                }
                if let Ok(_) = save_ron(class, "classes", data.temp_state.temp_class_filename.trim()) {
                    data.temp_state.temp_class_filename = "class".to_owned();
                    data.temp_state.temp_class = None;
                    data.temp_state.temp_class_profs.clear();
                    data.register_classes();
                }
            }
            if ui.button("Cancel").clicked() {
                data.temp_state.temp_class_filename = "class".to_owned();
                data.temp_state.temp_class = None;
                data.temp_state.temp_class_profs.clear();
            }
        } else {
            ui.vertical_centered(|ui| {
                if ui.button("Create new").clicked() {
                    data.temp_state.temp_class = Some(Class::default());
                }
            });
        }
    }
    fn combat(ui: &mut Ui, data: &mut DMAppData) {
        if let Some(fight) = &mut data.fight {
            if fight.started {
                let mut fight = fight.clone();
                match fight.ongoing_round {
                    true => {
                        ui.label("Turn order:");
                        for (i, (_, ctype)) in fight.turn_order.iter().enumerate() {
                            ui.horizontal(|ui| {
                                if fight.current_turn == i {
                                    ui.label(RichText::new(format!("- {}", ctype.name())).strong().underline());
                                } else {
                                    ui.label(format!("- {}", ctype.name()));
                                }
                                if ui.small_button(format!("{}", egui_phosphor::ARROW_SQUARE_OUT)).clicked() {
                                    match ctype {
                                        CombatantType::Enemy(id, _, _) => {
                                            data.temp_state.viewed_enemy = Some(id.clone());
                                            data.temp_state.window_states.insert("enemy_viewer".to_owned(), true);
                                        },
                                        CombatantType::PC(user, name) => {
                                            data.temp_state.window_states.insert(format!("user_character_window_<{}>_<{}>", user, name), true);
                                        },
                                    }
                                }
                            });
                        }
                        ui.separator();
                        match &fight.awaiting_response {
                            Some(owner) => {
                                match owner {
                                    Owner::DM => {
                                        ui.label(format!("Decide action ({})", fight.get_current_actor().name()));
                                        egui::ComboBox::from_label("Target")
                                            .show_index(ui, &mut data.temp_state.selected_target, data.temp_state.combatant_list.len(), |i| data.temp_state.combatant_list.get(i).map_or("error".to_owned(), |c| c.name()));
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
                                ui.vertical_centered(|ui| {
                                    if fight.current_turn >= fight.turn_order.len() {
                                        if ui.button("End round").clicked() {
                                            fight.next_turn(data);
                                        }
                                    } else {
                                        if ui.button(format!("Next turn: {}", fight.get_current_actor().name())).clicked() {
                                            fight.next_turn(data);
                                        }
                                    }
                                });
                            },
                        }
                    },
                    false => {
                        ui.vertical_centered(|ui| {
                            ui.label("The round is not started yet. Initiative is not yet calculated.");
                            if ui.button("Begin round").clicked() {
                                fight.start_round(data);
                            }
                        });
                    },
                }
                data.fight = Some(fight);
            } else {
                ui.vertical_centered(|ui| {
                    ui.label("The fight has not started yet. Use the deployed enemies window and the users window to add combatants, or use the buttons below.");
                    ui.separator();
                    ui.label("Combatants:");
                    let mut maybe_remove = None;
                    for (owner, ctype) in &fight.combatants {
                        ui.horizontal(|ui| {
                            ui.label(format!("- {} ({})", ctype.name(), match owner {
                                Owner::DM => "DM",
                                Owner::Player(p) => p,
                            }));
                            if ui.small_button("x").clicked() {
                                maybe_remove = Some((owner.clone(), ctype.clone()));
                            }
                        });
                    }
                    if let Some(c) = maybe_remove {
                        fight.combatants.remove(&c);
                    }
                    ui.separator();
                    if ui.button("Add all deployed enemies").clicked() {
                        for (id, (typ, group)) in &data.deployed_enemies {
                            for (i, _) in group.iter().enumerate() {
                                fight.combatants.insert((Owner::DM, CombatantType::Enemy(id.clone(), i as u32, typ.name.clone())));
                            }
                        }
                    }
                    if ui.button("Add all player characters").clicked() {
                        for (user, _) in &data.connected_users {
                            if let Some(user_data) = data.user_data.get(user) {
                                for (name, _) in &user_data.characters {
                                    fight.combatants.insert((Owner::Player(user.clone()), CombatantType::PC(user.clone(), name.clone())));
                                }
                            }
                        }
                    }
                    ui.separator();
                    if ui.button("Start!").clicked() {
                        fight.started = true;
                    }
                });
            }
        } else {
            ui.vertical_centered(|ui| {
                ui.label("There is currently no active combat.");
                if ui.button("Create").clicked() {
                    data.fight = Some(Fight::new());
                }
            });
        } 
    }
}

impl<F: FnMut(DMTab, bool)> TabViewer for DMTabViewer<'_, F> {
    type Tab = DMTab;

    fn add_popup(&mut self, ui: &mut Ui, _node: NodeIndex) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if ui.button("Chat").clicked() {
                    (self.callback)(DMTab::Chat, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Dice Roller").clicked() {
                    (self.callback)(DMTab::DiceRoller, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Player List").clicked() {
                    (self.callback)(DMTab::PlayerList, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Enemy Viewer").clicked() {
                    (self.callback)(DMTab::EnemyViewer, true);
                    ui.close_menu();
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Enemy Creator").clicked() {
                    (self.callback)(DMTab::EnemyCreator, true);
                    ui.close_menu();
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Deployed Enemies").clicked() {
                    (self.callback)(DMTab::DeployedEnemies, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Item Viewer").clicked() {
                    (self.callback)(DMTab::ItemViewer, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Item Creator").clicked() {
                    (self.callback)(DMTab::ItemCreator, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Class Viewer").clicked() {
                    (self.callback)(DMTab::ClassViewer, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Class Creator").clicked() {
                    (self.callback)(DMTab::ClassCreator, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Proficiency Viewer").clicked() {
                    (self.callback)(DMTab::ProficiencyViewer, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Proficiency Creator").clicked() {
                    (self.callback)(DMTab::ProficiencyCreator, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Spell Viewer").clicked() {
                    (self.callback)(DMTab::SpellViewer, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Spell Creator").clicked() {
                    (self.callback)(DMTab::SpellCreator, true);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Combat").clicked() {
                    (self.callback)(DMTab::Combat, true);
                }
            });
        });
    }

    fn context_menu(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            DMTab::Chat => {
                if ui.button("Detach").clicked() {
                    self.data.temp_state.window_states.insert("chat_window".to_owned(), true);
                    (self.callback)(DMTab::Chat, false);
                    ui.close_menu();
                }
            },
            _ => {},
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            DMTab::DiceRoller => {
                Self::dice_roller(ui, self.data);
            },
            DMTab::PlayerList => {
                self.player_list(ui);
            },
            DMTab::Player(user) => {
                self.player_tab(ui, &user);
            },
            DMTab::PlayerCharacter(user, name) => {
                self.player_character(ui, &user, &name);
            },
            DMTab::Chat => {
                self.chat_tab(ui);
            },
            DMTab::EnemyViewer => {
                Self::enemy_viewer(ui, self.data);
            },
            DMTab::DeployedEnemies => {
                Self::deployed_enemies(ui, self.data);
            },
            DMTab::EnemyCreator => {
                Self::enemy_creator(ui, self.data);
            },
            DMTab::ItemViewer => {
                Self::item_viewer(ui, self.data);
            },
            DMTab::ItemCreator => {
                Self::item_creator(ui, self.data);
            },
            DMTab::ProficiencyViewer => {
                Self::prof_viewer(ui, self.data);
            },
            DMTab::ProficiencyCreator => {
                Self::prof_creator(ui, self.data);
            },
            DMTab::SpellViewer => {
                Self::spell_viewer(ui, self.data);
            },
            DMTab::SpellCreator => {
                Self::spell_creator(ui, self.data);
            },
            DMTab::ClassViewer => {
                Self::class_viewer(ui, self.data);
            },
            DMTab::ClassCreator => {
                Self::class_creator(ui, self.data);
            },
            DMTab::Combat => {
                Self::combat(ui, self.data);
            },
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        if *tab == DMTab::Chat {
            self.data.get_chat_title()
        } else {
            (&*tab).to_string().into()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DMTab {
    DiceRoller,
    PlayerList,
    Player(String),
    PlayerCharacter(String, String),
    Chat,
    EnemyViewer,
    EnemyCreator,
    DeployedEnemies,
    ItemViewer,
    ItemCreator,
    ProficiencyViewer,
    ProficiencyCreator,
    SpellViewer,
    SpellCreator,
    ClassViewer,
    ClassCreator,
    Combat,
}

impl std::fmt::Display for DMTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::DiceRoller => "Dice Roller".to_owned(),
            Self::PlayerList => "Player List".to_owned(),
            Self::EnemyViewer => "Enemy Viewer".to_owned(),
            Self::EnemyCreator => "Enemy Creator".to_owned(),
            Self::DeployedEnemies => "Deployed Enemies".to_owned(),
            Self::ItemViewer => "Item Viewer".to_owned(),
            Self::ItemCreator => "Item Creator".to_owned(),
            Self::ProficiencyViewer => "Proficiency Viewer".to_owned(),
            Self::ProficiencyCreator => "Proficiency Creator".to_owned(),
            Self::SpellViewer => "Spell Viewer".to_owned(),
            Self::SpellCreator => "Spell Creator".to_owned(),
            Self::ClassViewer => "Class Viewer".to_owned(),
            Self::ClassCreator => "Class Creator".to_owned(),
            Self::Combat => "Combat".to_owned(),
            Self::Chat => format!("{}", ep::CHAT_TEXT),
            Self::PlayerCharacter(player, name) => format!("{} ({})", player, name),
            Self::Player(player) => format!("Player ({})", player),
        })
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
        Self::chat_window(ctx, data, &mut self.tree);
        Self::requests_window(ctx, data);
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.add_space(5.0);
                Self::top_bar(ctx, ui, data);
                ui.add_space(5.0);
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.tree.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() / 2.0) - 10.0);
                    if ui.add(egui::Button::new("Oh no, you closed the last tab! Click me to get one back.").frame(false)).clicked() {
                        self.tree.push_to_first_leaf(DMTab::Chat);
                    }
                });
            } else {
                let mut new_tab = None;
                let mut remove_tab = None;
                DockArea::new(&mut self.tree)
                    .show_add_buttons(true)
                    .show_add_popup(true)
                    .show_inside(ui, &mut DMTabViewer {
                        callback: &mut |tab, add| {
                            if add {
                                new_tab = Some(tab);
                            } else {
                                remove_tab = Some(tab);
                            }
                        },
                        data,
                    });
                if let Some(tab) = new_tab {
                    if let Some((node_i, tab_i)) = self.tree.find_tab(&tab) {
                        self.tree.set_focused_node(node_i);
                        self.tree.set_active_tab(node_i, tab_i);
                    } else {
                        self.tree.push_to_focused_leaf(tab);
                    }
                }
                if let Some(tab) = remove_tab {
                    if let Some(i) = self.tree.find_tab(&tab) {
                        self.tree.remove_tab(i);
                    }
                }
            }
        });
        data.temp_state.unread_msg_buffer = false;
        if *data.temp_state.window_states.entry("exit_are_you_sure".to_owned()).or_insert(false) {
            Self::exit_are_you_sure(ctx, frame, data);
        }
        let info = frame.info().window_info;
        let pos = info.position.unwrap_or_default();
        data.prefs.pos = (pos.x, pos.y);
        data.prefs.size = (info.size.x, info.size.y);
        ctx.request_repaint();
    }
}