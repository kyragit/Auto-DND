use crate::map::{Map, Room, RoomContainer, RoomTrap, RoomConnection};
use crate::party::Party;
use crate::{AppPreferences, WindowPreferences};
use crate::character::{PlayerCharacter, SavingThrows, Attr, PlayerEquipSlot};
use crate::class::{SavingThrowProgressionType, Class, ClassDamageBonus, Cleaves, HitDie, AttackThrowProgression, WeaponSelection, BroadWeapons, NarrowWeapons, RestrictedWeapons, ArmorSelection, THIEF_SKILLS};
use crate::combat::{Fight, Owner, Combatant, CombatantStats, DamageRoll, PreRoundAction, TurnType, MovementAction, AttackAction, SpecialManeuver, StatusEffect};
use crate::common_ui::*;
use crate::dice::{ModifierType, Drop, DiceRoll, roll};
use crate::enemy::{Enemy, EnemyType, EnemyHitDice, EnemyCategory, Alignment, AttackRoutine};
use crate::item::{ItemType, Encumbrance, WeaponStats, WeaponDamage, MeleeDamage, ContainerStats, Item};
use crate::proficiency::{Proficiency, ProficiencyInstance};
use crate::race::Race;
use crate::spell::{Spell, MagicType, SpellRange, SpellDuration, SpellRegistry};
use eframe::egui::{self, Ui, RichText, WidgetText, Color32};
use egui::collapsing_header::CollapsingState;
use egui::{Label, Sense, TextEdit, Id, Layout, Align};
use egui::text::LayoutJob;
use egui_dock::{DockArea, Tree, TabViewer};
use simple_enum_macro::simple_enum;
use thousands::Separable;
use crate::packets::{ClientBoundPacket, ServerBoundPacket, Request};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::net::{TcpListener, TcpStream, SocketAddr, SocketAddrV4, Ipv4Addr};
use std::io::{prelude::*, ErrorKind};
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

/// A default, fallback socket address to use in case something went wrong. 
const DEFAULT_IP: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 8080));

/// Responsible for reading and handling packets, as well as handling existing connections.
fn handle_streams(data: Arc<Mutex<DMAppData>>) {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(SERVER_UPDATE_CLOCK));
        let data = &mut *data.lock().unwrap();
        let mut closed: Vec<usize> = Vec::new();
        let mut packets: Vec<(ServerBoundPacket, SocketAddr)> = Vec::new();
        let mut client_packets: Vec<ClientBoundPacket> = Vec::new();
        for (i, (stream, buffer)) in data.streams.iter_mut().enumerate() {
            let addr = stream.peer_addr().unwrap_or(DEFAULT_IP);
            let mut reader = std::io::BufReader::new(&mut *stream);
            let mut recieved: Vec<u8> = Vec::new();
            recieved.append(buffer);
            match reader.fill_buf() {
                Ok(buf) => {
                    if buf.is_empty() {
                        continue;
                    }
                    recieved.append(&mut buf.to_vec());
                },
                Err(e) => {
                    match e.kind() {
                        ErrorKind::ConnectionReset |
                        ErrorKind::ConnectionAborted => {
                            closed.push(i);
                            for (name, ip) in &data.connected_users {
                                if *ip == addr {
                                    let msg = ChatMessage::no_sender(format!("User \"{}\" has disconnected.", name)).blue();
                                    data.logs.insert(0, msg.to_layout_job());
                                    client_packets.push(ClientBoundPacket::ChatMessage(msg));
                                }
                            }
                        },
                        _ => {},
                    }
                    continue;
                },
            }

            let mut iter = recieved.split_inclusive(|b| *b == 255);
            while let Some(bytes) = iter.next() {
                if bytes.ends_with(&[255]) {
                    reader.consume(bytes.len());
                    if bytes.len() < 2 {
                        continue;
                    }
                    let msg = String::from_utf8_lossy(&bytes[..bytes.len() - 1]);
                    match ron::from_str::<ServerBoundPacket>(&*msg) {
                        Ok(packet) => {
                            packets.push((packet, addr));
                        },
                        Err(_) => {},
                    }
                } else {
                    *buffer = bytes.to_vec();
                }
            }
        }
        if !closed.is_empty() {
            closed.sort();
            closed.reverse();
            for i in closed {
                let (stream, _) = data.streams.remove(i);
                if let Ok(addr) = stream.peer_addr() {
                    data.connected_users.retain(|_, v| *v != addr);
                }
            }
        }
        for (packet, user) in packets {
            if user != DEFAULT_IP {
                packet.handle(data, user);
            }
        }
        for packet in client_packets {
            data.send_to_all_players(packet);
        }
    }
}

/// Responsible for handling new incoming connections.
fn handle_connections(data: Arc<Mutex<DMAppData>>) {
    let listener;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(SERVER_UPDATE_CLOCK));
        let data = &mut *data.lock().unwrap();
        if let Some(addr) = data.host_addr {
            match TcpListener::bind(addr) {
                Ok(l) => {
                    listener = l;
                    break;
                },
                Err(e) => {
                    data.log(ChatMessage::no_sender(format!("Error when attempting to host: {}", e)).private().red());
                    data.host_addr = None;
                },
            }
        }
    }
    for stream in listener.incoming() {
        let data = &mut *data.lock().unwrap();
        match stream {
            Ok(s) => {
                s.set_nonblocking(true).unwrap();
                data.log(ChatMessage::no_sender(format!("Connection from user with ip: {:?}", s.peer_addr())).private().blue());
                data.streams.push((s, Vec::new()));
            },
            Err(e) => {
                data.log(ChatMessage::no_sender(format!("Connection error: {:?}", e)).private().red());
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
    pub parties: HashMap<String, Party>,
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
    pub chat: String,
    pub last_chat: String,
    pub unread_messages: u32,
    pub unread_msg_buffer: bool,
    pub dice_roll: DiceRoll,
    pub dice_roll_advanced: bool,
    pub dice_roll_public: bool,
    pub show_offline_users: bool,
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
    pub viewed_prof_spec: Option<String>,
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
    pub temp_party: Option<(String, Party)>,
    pub temp_map_filename: String,
    pub temp_map_name: String,
    pub map_editing_mode: bool,
    pub temp_map_room_id: String,
    pub temp_room_connect_one_way: bool,
    pub temp_container_section: String,
}

impl AppTempState {
    pub fn new() -> Self {
        Self {
            exit_without_saving: false,
            window_states: HashMap::new(),
            chat: String::new(),
            last_chat: String::new(),
            unread_messages: 0,
            unread_msg_buffer: false,
            dice_roll: DiceRoll::simple(1, 20),
            dice_roll_advanced: false,
            dice_roll_public: true,
            show_offline_users: false,
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
            viewed_prof_spec: None,
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
            temp_party: None,
            temp_map_filename: "map".to_owned(),
            temp_map_name: String::new(),
            map_editing_mode: false,
            temp_map_room_id: String::new(),
            temp_room_connect_one_way: false,
            temp_container_section: String::new(),
        }
    }
}

/// All the app's data.
pub struct DMAppData {
    pub host_port: u16,
    pub host_addr: Option<SocketAddr>,
    pub known_users: HashMap<String, String>,
    pub user_data: HashMap<String, UserData>,
    pub parties: HashMap<String, Party>,
    pub connected_users: HashMap<String, SocketAddr>,
    pub logs: Vec<LayoutJob>,
    pub streams: Vec<(TcpStream, Vec<u8>)>,
    pub temp_state: AppTempState,
    pub enemy_type_registry: Registry<EnemyType>,
    pub item_type_registry: Registry<ItemType>,
    pub class_registry: Registry<Class>,
    pub proficiency_registry: HashMap<String, Proficiency>,
    pub sorted_prof_list: Vec<(String, String)>,
    pub spell_registry: SpellRegistry,
    pub prefs: WindowPreferences,
    pub map_registry: HashMap<String, String>,
    pub loaded_map: Option<(String, Map)>,
}

impl DMAppData {
    pub fn new() -> Self {
        Self { 
            host_port: 8080,
            host_addr: None,
            known_users: HashMap::new(),
            user_data: HashMap::new(),
            parties: HashMap::new(),
            connected_users: HashMap::new(),
            logs: Vec::new(),
            streams: Vec::new(),
            temp_state: AppTempState::new(),
            enemy_type_registry: Registry::new(),
            item_type_registry: Registry::new(),
            class_registry: Registry::new(),
            proficiency_registry: HashMap::new(),
            sorted_prof_list: Vec::new(),
            spell_registry: SpellRegistry::new(),
            prefs: WindowPreferences::new(),
            map_registry: HashMap::new(),
            loaded_map: None,
        }
    }

    /// Reads data stored on disk, if it exists.
    pub fn load(&mut self) {
        if let Ok(mut file) = std::fs::read_to_string("savedata.ron") {
            match ron::from_str::<SaveData>(&file) {
                Ok(data) => {
                    self.known_users = data.known_users;
                    self.user_data = data.user_data;
                    self.parties = data.parties;
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
        self.register_maps();
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

    fn register_maps(&mut self) {
        self.map_registry.clear();
        Self::read_dir_recursive("maps", |path, s| {
            if let Ok(map) = ron::from_str::<Map>(&s) {
                self.map_registry.insert(path.strip_prefix("maps\\").unwrap().to_owned(), map.name);
            }
        });
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
        if let Some((file, map)) = &self.loaded_map {
            let _ = save_ron(map, "maps", file);
        }
        let mut file: File;
        if let Ok(f) = File::options().write(true).truncate(true).open("savedata.ron") {
            file = f;
        } else {
            file = File::create("savedata.ron").expect("Failed to create file");
        }
        let save_data = SaveData {
            known_users: self.known_users.clone(),
            user_data: self.user_data.clone(),
            parties: self.parties.clone(),
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

    /// Saves the currently loaded map to disc and unloads it.
    pub fn save_and_unload_map(&mut self) {
        if let Some((file, map)) = &self.loaded_map {
            let _ = save_ron(map, "maps", file);
            self.loaded_map = None;
        }
    }

    /// Applies a closure to every active tcp stream (connection).
    pub fn foreach_streams<F>(&mut self, mut func: F) 
        where F: FnMut(&mut TcpStream) -> std::io::Result<()> {
        for (stream, _) in self.streams.iter_mut() {
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
            for (stream, _) in &mut self.streams {
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
                for (stream, _) in &mut self.streams {
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
    pub fn log(&mut self, msg: ChatMessage) {
        self.logs.insert(0, msg.to_layout_job());
        if !msg.flags.private {
            self.send_to_all_players(ClientBoundPacket::ChatMessage(msg));
        }
    }

    /// Passes a mutable reference to the combatant's stats to the provided callback, or None if
    /// it doesn't exist.
    pub fn get_combatant_stats<F, R>(&mut self, combatant: &Combatant, f: F) -> R 
    where F: FnOnce(Option<&mut CombatantStats>) -> R {
        match combatant {
            Combatant::Enemy { room, type_id, index, display_name: _ } => {
                if let Some((_, map)) = &mut self.loaded_map {
                    if let Some(room) = map.rooms.get_mut(room) {
                        if let Some((_, group)) = room.enemies.get_mut(type_id) {
                            if let Some(enemy) = group.get_mut(*index) {
                                return f(Some(&mut enemy.combat_stats));
                            }
                        }
                    }
                }
                f(None)
            },
            Combatant::PC { user, name } => {
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
    pub fn get_combatant_stats_alt<F, R>(&mut self, combatant: &Combatant, f: F) -> Option<R> 
    where F: FnOnce(&mut CombatantStats) -> R {
        match combatant {
            Combatant::Enemy { room, type_id, index, display_name: _ } => {
                if let Some((_, map)) = &mut self.loaded_map {
                    if let Some(room) = map.rooms.get_mut(room) {
                        if let Some((_, group)) = room.enemies.get_mut(type_id) {
                            if let Some(enemy) = group.get_mut(*index) {
                                return Some(f(&mut enemy.combat_stats));
                            }
                        }
                    }
                }
                None
            },
            Combatant::PC { user, name } => {
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
    pub fn update_combatant(&mut self, combatant: &Combatant) {
        if let Combatant::PC { user, name } = combatant {
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

    /// If the character exists, passes it to the given function.
    pub fn apply_to_pc<F, R>(&mut self, user: impl Into<String>, name: impl Into<String>, func: F) -> Option<R> 
    where
        F: FnOnce(&mut PlayerCharacter) -> R
    {
        if let Some(user_data) = self.user_data.get_mut(&user.into()) {
            if let Some(sheet) = user_data.characters.get_mut(&name.into()) {
                return Some(func(sheet));
            }
        }
        None
    }

    /// If the character exists, passes it to the given function. Otherwise, returns the given default.
    pub fn apply_to_pc_or<F, R>(&mut self, user: impl Into<String>, name: impl Into<String>, default: R, func: F) -> R 
    where 
        F: FnOnce(&mut PlayerCharacter) -> R
    {
        if let Some(user_data) = self.user_data.get_mut(&user.into()) {
            if let Some(sheet) = user_data.characters.get_mut(&name.into()) {
                return func(sheet);
            }
        }
        default
    }

    fn get_chat_title(&self) -> WidgetText {
        if self.temp_state.unread_messages == 0 || self.temp_state.unread_msg_buffer {
            format!("{}", ep::CHAT_TEXT).into()
        } else {
            RichText::new(format!("{}({})", ep::CHAT_TEXT, self.temp_state.unread_messages)).color(Color32::RED).into()
        }
    }

    /// Passes a mutable reference to the currently active `Fight` to the provided function, if it
    /// exists.
    /// ### Returns
    /// Whatever the passed function returns, or `None` if there is no `Fight`.
    pub fn get_fight<F, R>(&mut self, func: F) -> Option<R>
    where 
        F: FnOnce(&mut Fight) -> R 
    {
        if let Some((_, map)) = &mut self.loaded_map {
            if let Some(fight) = &mut map.fight {
                return Some(func(fight));
            }
        }
        None
    }
}

/// The main app.
pub struct DMApp {
    /// Data is wrapped in an `Arc<Mutex<_>>` because it is shared state between threads.
    pub data: Arc<Mutex<DMAppData>>,
    pub tree: Tree<DMTab>,
    pub map_tree: Tree<MapTab>,
}

impl DMApp {
    pub fn new(data: Arc<Mutex<DMAppData>>) -> Self {
        Self { 
            data,
            tree: {
                let tree = Tree::new(vec![DMTab::Chat]);
                tree
            },
            map_tree: {
                let tree = Tree::new(vec![MapTab::Main]);
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

    fn open_or_focus(tree: &mut Tree<DMTab>, tab: DMTab) {
        if let Some((node_i, tab_i)) = tree.find_tab(&tab) {
            tree.set_active_tab(node_i, tab_i);
            tree.set_focused_node(node_i);
        } else {
            tree.push_to_focused_leaf(tab);
        }
    }

    fn top_bar(ctx: &egui::Context, ui: &mut Ui, data: &mut DMAppData, tree: &mut Tree<DMTab>) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Network", |ui| {
                if data.host_addr.is_none() {
                    if ui.button("Host").clicked() {
                        if let Ok(ip) = local_ip_address::local_ip() {
                            data.host_addr = Some(SocketAddr::new(ip, data.host_port));
                            ui.close_menu();
                        }
                    }
                    ui.menu_button("Port", |ui| {
                        ui.add(egui::DragValue::new(&mut data.host_port));
                    });
                } else {
                    ui.label("Hosting...");
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Chat").clicked() {
                    Self::open_or_focus(tree, DMTab::Chat);
                    ui.close_menu();
                }
                if ui.button("Classes").clicked() {
                    Self::open_or_focus(tree, DMTab::ClassViewer);
                    ui.close_menu();
                }
                if ui.button("Combat").clicked() {
                    Self::open_or_focus(tree, DMTab::Combat);
                    ui.close_menu();
                }
                if ui.button("Dice Roller").clicked() {
                    Self::open_or_focus(tree, DMTab::DiceRoller);
                    ui.close_menu();
                }
                if ui.button("Enemies").clicked() {
                    Self::open_or_focus(tree, DMTab::EnemyViewer);
                    ui.close_menu();
                }
                if ui.button("Items").clicked() {
                    Self::open_or_focus(tree, DMTab::ItemViewer);
                    ui.close_menu();
                }
                if ui.button("Maps").clicked() {
                    Self::open_or_focus(tree, DMTab::MapViewer);
                    ui.close_menu();
                }
                if ui.button("Parties").clicked() {
                    Self::open_or_focus(tree, DMTab::Parties);
                    ui.close_menu();
                }
                ui.menu_button("Players", |ui| {
                    if ui.button("Player List").clicked() {
                        Self::open_or_focus(tree, DMTab::PlayerList);
                        ui.close_menu();
                    }
                    ui.separator();
                    for (user, _) in &data.connected_users {
                        ui.menu_button(user, |ui| {
                            if ui.button("View").clicked() {
                                Self::open_or_focus(tree, DMTab::Player(user.clone()));
                                ui.close_menu();
                            }
                            ui.separator();
                            ui.menu_button("Characters", |ui| {
                                if let Some(user_data) = data.user_data.get(user) {
                                    for (name, _) in &user_data.characters {
                                        if ui.button(name).clicked() {
                                            Self::open_or_focus(tree, DMTab::PlayerCharacter(user.clone(), name.clone()));
                                            ui.close_menu();
                                        }
                                    }
                                }
                            });
                        });
                    }
                });
                if ui.button("Proficiencies").clicked() {
                    Self::open_or_focus(tree, DMTab::ProficiencyViewer);
                    ui.close_menu();
                }
                if ui.button("Spells").clicked() {
                    Self::open_or_focus(tree, DMTab::SpellViewer);
                    ui.close_menu();
                }
            });
            ui.menu_button("Create", |ui| {
                if ui.button("Classes").clicked() {
                    Self::open_or_focus(tree, DMTab::ClassCreator);
                    ui.close_menu();
                }
                if ui.button("Enemies").clicked() {
                    Self::open_or_focus(tree, DMTab::EnemyCreator);
                    ui.close_menu();
                }
                if ui.button("Items").clicked() {
                    Self::open_or_focus(tree, DMTab::ItemCreator);
                    ui.close_menu();
                }
                if ui.button("Maps").clicked() {
                    Self::open_or_focus(tree, DMTab::MapCreator);
                    ui.close_menu();
                }
                if ui.button("Proficiencies").clicked() {
                    Self::open_or_focus(tree, DMTab::ProficiencyCreator);
                    ui.close_menu();
                }
                if ui.button("Spells").clicked() {
                    Self::open_or_focus(tree, DMTab::SpellCreator);
                    ui.close_menu();
                }
            });
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
                data.log(ChatMessage::server(data.temp_state.chat.clone()));
            }
            data.temp_state.chat.clear();
        }
        for (i, log) in data.logs.iter().enumerate() {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.label(log.clone());
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
            "user" => {
                if let Some(username) = tree.next() {
                    if data.known_users.contains_key(username) {
                        if let Some(token) = tree.next() {
                            match token {
                                "kick" => {
                                    if let Some(addr) = data.connected_users.get(username) {
                                        let mut msg = "Error".to_owned();
                                        for (stream, _) in &mut data.streams {
                                            if let Ok(a) = stream.peer_addr() {
                                                if *addr == a {
                                                    msg = format!("Kicking user \"{}\".", username);
                                                    let _ = stream.shutdown(std::net::Shutdown::Both);
                                                }
                                            }
                                        }
                                        data.log(ChatMessage::no_sender(msg).red());
                                    } else {
                                        data.log(ChatMessage::no_sender(format!("The user \"{}\" is not connected.", username)).private().light_red());
                                    }
                                },
                                t => {
                                    unknown_command(data, t);
                                },
                            }
                        } else {
                            data.log(ChatMessage::no_sender(format!("You must specify a command. What are you trying to do with \"{}\"?", username)).private().light_red());
                        }
                    } else {
                        data.log(ChatMessage::no_sender(format!("The user \"{}\" doesn't appear to exist. If their name contains a space, remember to wrap it in \"quotes\".", username)).private().light_red()); 
                    }
                } else {
                    data.log(ChatMessage::no_sender("You must specify a user.").private().light_red());
                }
            },
            "known_users" => {
                for user in data.known_users.clone().keys() {
                    data.log(ChatMessage::no_sender(format!("- {}", user)).private());
                }
                data.log(ChatMessage::no_sender("List of all known users:").private());
            },
            "players" => {
                let mut empty = true;
                for user in data.connected_users.clone().keys() {
                    empty = false;
                    data.log(ChatMessage::no_sender(format!("- {}", user)).private());
                }
                if empty {
                    data.log(ChatMessage::no_sender("There are no connected players!").private());  
                } else {
                    data.log(ChatMessage::no_sender("List of all connected players:").private());  
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
                                        data.log(ChatMessage::no_sender("Added XP").private().green());
                                    } else {
                                        data.log(ChatMessage::no_sender(format!("The token \"{}\" could not be interpreted as a number.", token)).private().light_red());
                                    }
                                } else {
                                    data.log(ChatMessage::no_sender("You must specify an amount of XP to add.").private().light_red());
                                }
                            } else {
                                data.log(ChatMessage::no_sender(format!("The character \"{}\" doesn't appear to exist. If their name contains a space, remember to wrap it in \"quotes\".", name)).private().light_red()); 
                            }
                        } else {
                            data.log(ChatMessage::no_sender("You must specify a character.").private());
                        }
                    } else {
                        data.log(ChatMessage::no_sender(format!("The user \"{}\" doesn't appear to exist. If their name contains a space, remember to wrap it in \"quotes\".", user)).private().light_red()); 
                    }
                } else {
                    data.log(ChatMessage::no_sender("You must specify a user.").private().light_red());
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
                                    data.log(ChatMessage::server(format!("{}", roll.roll())).dice_roll());
                                } else {
                                    data.log(ChatMessage::server(format!("{}", roll.roll())).private().dice_roll());
                                }
                            },
                            Err(e) => {
                                data.log(ChatMessage::no_sender(format!("Error: {}", e)).private().light_red());
                            },
                        }
                    } else {
                        data.log(ChatMessage::no_sender("You must enter dice notation. Run /help roll for more info.").private().light_red());
                    }
                } else {
                    data.log(ChatMessage::no_sender("You must enter dice notation. Run /help roll for more info.").private().light_red());
                }
            },
            "say" => {
                let mut message = ChatMessage::no_sender(String::new());                
                while let Some(token) = tree.next() {
                    match token {
                        "-c" | "-color" => {
                            if let Some(token) = tree.next() {
                                let color = token.strip_prefix('#').unwrap_or(token);
                                if color.len() != 6 {
                                    data.log(ChatMessage::no_sender(format!("Token \"{}\" is not a valid color.", token)).private());
                                    return;
                                } else {
                                    let red = u8::from_str_radix(color.get(0..=1).unwrap_or("??"), 16);
                                    let green = u8::from_str_radix(color.get(2..=3).unwrap_or("??"), 16);
                                    let blue = u8::from_str_radix(color.get(4..=5).unwrap_or("??"), 16);
                                    if red.is_err() || green.is_err() || blue.is_err() {
                                        data.log(ChatMessage::no_sender(format!("Token \"{}\" is not a valid color.", token)).private());
                                        return;
                                    }
                                    message.color = Color32::from_rgb(red.unwrap(), green.unwrap(), blue.unwrap());
                                }
                            } else {
                                data.log(ChatMessage::no_sender("You must provide a color hex code (e.g. #00FFAA).").private());
                                return;
                            }
                        },
                        "-u" | "-underline" => {
                            message = message.underline();
                        },
                        "-s" | "-strikethrough" => {
                            message = message.strikethrough();
                        },
                        "-i" | "-italics" => {
                            message = message.italics();
                        },
                        "-size" => {
                            if let Some(token) = tree.next() {
                                if let Ok(mut size) = token.parse::<f32>() {
                                    if size > 100.0 {
                                        size = 100.0;
                                    }
                                    message = message.size(size);
                                } else {
                                    data.log(ChatMessage::no_sender(format!("Token \"{}\" could not be parsed as a number.", token)).private());
                                    return;
                                }
                            } else {
                                data.log(ChatMessage::no_sender("You must provide a size.").private());
                                return;
                            }
                        },
                        "-as" => {
                            if let Some(token) = tree.next() {
                                match token {
                                    "server" => {
                                        message.sender = MessageSender::Server;
                                    },
                                    t => {
                                        message.sender = MessageSender::Player(t.to_owned());
                                    },
                                }
                            } else {
                                data.log(ChatMessage::no_sender("You must provide a user, or \"server\".").private());
                                return;
                            }
                        },
                        "-v" | "-valign" => {
                            if let Some(token) = tree.next() {
                                match token {
                                    "top" | "min" | "above" => {
                                        message.valign = Align::Min;
                                    },
                                    "center" | "centre" | "middle" => {
                                        message.valign = Align::Center;
                                    },
                                    "bottom" | "max" | "below" => {
                                        message.valign = Align::Max;
                                    },
                                    t => {
                                        data.log(ChatMessage::no_sender(format!("Token \"{}\" could not be interpreted as a vertical alignment. Valid options are \"top\", \"center\", or\"bottom\".", t)).private());
                                        return;
                                    },
                                }
                            } else {
                                data.log(ChatMessage::no_sender("You must provide a vertical alignment.").private());
                                return;
                            }
                        },
                        "-r" | "-roll" => {
                            message = message.dice_roll();
                        },
                        "-o" | "-combat" => {
                            message = message.combat();
                        },
                        "-p" | "-private" => {
                            message = message.private();
                        },
                        "-a" | "-parties" => {
                            message = message.parties();
                        },
                        t => {
                            message.message = t.to_owned();
                            break;
                        },
                    }
                }
                if message.message.is_empty() {
                    data.log(ChatMessage::no_sender("You must provide text. Make sure to put it in \"quotes\".").private());
                } else {
                    data.log(message);
                }
            },
            "help" => {
                if let Some(token) = tree.next() {
                    match token {
                        "roll" => {
                            data.log(ChatMessage::no_sender("min: Denotes a minimum value, inclusive or exclusive. A '>' symbol, optionally followed by a '=' symbol, then a value. Defaults to >=1.").private());
                            data.log(ChatMessage::no_sender("X: How many dice to drop. Defaults to 1, and cannot be greater than N.").private());
                            data.log(ChatMessage::no_sender("drop: An underscore ('_') followed by an 'h' or 'l'. Denotes to drop one or more dice, either highest or lowest.").private());
                            data.log(ChatMessage::no_sender("A: The modifier value. Mandatory if <op> is present.").private());
                            data.log(ChatMessage::no_sender("op: One of +, -, *, or /. Division is rounded normally by default, but append a 'u' or 'd' to the '/' to round up or down.").private());
                            data.log(ChatMessage::no_sender("&: If present, apply the modifier to each die rather than the sum of all dice.").private());
                            data.log(ChatMessage::no_sender("M: How many sides the dice have. Mandatory.").private());
                            data.log(ChatMessage::no_sender("d: The literal letter \'d\'.").private());
                            data.log(ChatMessage::no_sender("N: Number of dice to roll. Defaults to 1.").private());
                            data.log(ChatMessage::no_sender("(Parentheses) denote optional values.").private());
                            data.log(ChatMessage::no_sender("(N)dM(&)(<op>A)(<drop>(X))(<min>)").private().strong());
                            data.log(ChatMessage::no_sender("Rolls dice using dice notation. <visibility> can be public/pub, private/priv, or absent (defaults private). <string> uses modified dice notation syntax:").private());
                            data.log(ChatMessage::no_sender("/roll <visibility> <string>").private().strong());
                        },
                        t => {
                            unknown_command(data, t);
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
                    data.log(ChatMessage::no_sender(msg).private());
                }
            },
            t => {
                unknown_command(data, t);
            },
        }
    } else {
        unknown_command(data, "");
    }
}

pub fn unknown_command(data: &mut DMAppData, token: impl Into<String>) {
    data.log(ChatMessage::no_sender(format!("Unknown command \"{}\".", token.into())).private().light_red());
}

pub struct DMTabViewer<'a, F: FnMut(DMTab, bool)> {
    pub callback: &'a mut F,
    pub data: &'a mut DMAppData,
    pub map_tree: &'a mut Tree<MapTab>,
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
                dice_roll_editor(ui, &mut data.temp_state.dice_roll);
            } else {
                dice_roll_editor_simple(ui, &mut data.temp_state.dice_roll);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Roll!").strong()).clicked() {
                    let r = roll(data.temp_state.dice_roll);
                    if data.temp_state.dice_roll_public {
                        data.log(ChatMessage::server(format!("{}", r)).dice_roll());
                    } else {
                        data.log(ChatMessage::server(format!("{}", r)).private().dice_roll());
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
                    ui.horizontal(|ui| {
                        self.open_tab_button(ui, format!("Character: {}", name), DMTab::PlayerCharacter(user.clone(), name.clone()), DMTab::Player(user.clone()));
                        // if let Some(fight) = &mut self.data.fight {
                        //     if !fight.started {
                        //         if ui.button("Add to fight").clicked() {
                        //             fight.combatants.insert((Owner::Player(user.clone()), Combatant::pc(user.clone(), name)));
                        //         }
                        //     }
                        // }
                    });
                }
            } else {
                ui.colored_label(ui.visuals().error_fg_color, "Something went wrong. This user doesn't appear to exist!");
            }
        });
    }
    fn player_character(&mut self, ui: &mut Ui, user: &String, name: &String) {
        let data = &mut *self.data;
        let mut parties_changed = false;
        let mut msg = None;
        if back_arrow(ui) {
            (self.callback)(DMTab::PlayerCharacter(user.clone(), name.clone()), false);
            (self.callback)(DMTab::Player(user.clone()), true);
        }
        ui.separator();
        if let Some(user_data) = data.user_data.get_mut(user) {
            if let Some(sheet) = user_data.characters.get_mut(name) {
                let tab = user_data.charsheet_tabs.entry(name.clone()).or_insert(CharacterSheetTab::Stats);
                let mut changed = false;
                tabs(tab, format!("<{}>_charsheet_tab_<{}>", user, name), ui, |_, _| {}, |ui, tab| {
                    match tab {
                        CharacterSheetTab::Stats => {
                            if let Some(party_name) = &sheet.party {
                                ui.horizontal(|ui| {
                                    ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 0.0);
                                    ui.label("Member of ");
                                    if let Some(party) = data.parties.get(party_name) {
                                        ui.colored_label(party.color, party_name);
                                    } else {
                                        ui.colored_label(Color32::RED, format!("{}ERROR: Nonexistent party!{}", ep::WARNING, ep::WARNING));
                                    }
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    ui.label("Not a member of any party");
                                    ui.menu_button(RichText::new(format!("{}", ep::PLUS)).color(Color32::LIGHT_GREEN), |ui| {
                                        for (party_name, party) in &mut data.parties {
                                            if ui.button(RichText::new(party_name).color(party.color)).clicked() {
                                                party.members.insert((user.clone(), name.clone()));
                                                sheet.party = Some(party_name.clone());
                                                changed = true;
                                                parties_changed = true;
                                                msg = Some(ChatMessage::no_sender(format!("{} has joined {}!", name, party_name)).parties().color(party.color));
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                });
                            }
                            ui.separator();
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
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.armor_class);
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Initiative: {:+}", sheet.combat_stats.modifiers.initiative.total()));
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.initiative);
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Surprise: {:+}", sheet.combat_stats.modifiers.surprise.total()));
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.surprise);
                            });
                            ui.label(format!("ATK: {:+}", sheet.combat_stats.attack_throw));
                            ui.label(format!("Base damage: {}", sheet.combat_stats.damage.display()));
                            ui.horizontal(|ui| {
                                ui.label(format!("Melee ATK bonus: {:+}", sheet.combat_stats.modifiers.melee_attack.total()));
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.melee_attack);
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Missile ATK bonus: {:+}", sheet.combat_stats.modifiers.missile_attack.total()));
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.missile_attack);
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Melee DMG bonus: {:+}", sheet.combat_stats.modifiers.melee_damage.total()));
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.melee_damage);
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Missile DMG bonus: {:+}", sheet.combat_stats.modifiers.missile_damage.total()));
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.missile_damage);
                            });
                            ui.separator();
                            let saves = sheet.combat_stats.saving_throws;
                            ui.label("Saving throws:");
                            ui.horizontal(|ui| {
                                ui.label(format!("Petrification & Paralysis: {:+}", saves.petrification_paralysis + sheet.combat_stats.modifiers.save_petrification_paralysis.total()));
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.save_petrification_paralysis);
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Poison & Death: {:+}", saves.poison_death + sheet.combat_stats.modifiers.save_poison_death.total()));
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.save_poison_death);
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Blast & Breath: {:+}", saves.blast_breath + sheet.combat_stats.modifiers.save_blast_breath.total()));
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.save_blast_breath);
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Staffs & Wands: {:+}", saves.staffs_wands + sheet.combat_stats.modifiers.save_staffs_wands.total()));
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.save_staffs_wands);
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Spells: {:+}", saves.spells + sheet.combat_stats.modifiers.save_spells.total()));
                                stat_mod_i32_button(ui, &mut sheet.combat_stats.modifiers.save_spells);
                            });
                        },
                        CharacterSheetTab::Class => {
                            ui.label(format!("Class: {}", sheet.class.name));
                            ui.label(RichText::new(&sheet.class.description).italics().weak());
                            ui.label(format!("Title: {}", sheet.title));
                            ui.label(format!("Race: {}", sheet.race));
                            ui.label(format!("Level: {}", sheet.level));
                            ui.horizontal(|ui| {
                                ui.label(format!("XP: {}/{} ({:+.1}%)", sheet.xp.separate_with_commas(), if sheet.level >= sheet.class.maximum_level {ep::INFINITY.to_owned()} else {sheet.xp_to_level.separate_with_commas()}, sheet.combat_stats.modifiers.xp_gain.total() * 100.0));
                                stat_mod_percent_button(ui, &mut sheet.combat_stats.modifiers.xp_gain);
                            });
                            ui.label(format!("Hit Die: {}", sheet.class.hit_die));
                        },
                        CharacterSheetTab::Inventory => {
                            let mut to_equip = None;
                            sheet.inventory.foreach_enumerate(|i, item| {
                                ui.horizontal(|ui| {
                                    let response = ui.add(egui::Label::new(format!("{} x{}", item.item_type.name, item.count)).sense(egui::Sense::click()));
                                    ui.menu_button("...", |ui| {
                                        ui.horizontal(|ui| {
                                            ui.heading(&item.item_type.name);
                                            if item.count > 1 {
                                                ui.weak(format!("x{}", item.count));
                                            }
                                        });
                                        ui.label(RichText::new(&item.item_type.description).weak().italics());
                                        ui.separator();
                                        ui.label(format!("Encumbrance: {}", item.item_type.encumbrance.display()));
                                        ui.label(format!("Value: {:.1} sp", item.item_type.value.0))
                                            .on_hover_text(
                                            RichText::new(format!("{:.1} cp\n{:.1} sp\n{:.1} ep\n{:.1} gp\n{:.1} pp", 
                                                item.item_type.value.as_copper(),
                                                item.item_type.value.as_silver(),
                                                item.item_type.value.as_electrum(),
                                                item.item_type.value.as_gold(),
                                                item.item_type.value.as_platinum(),
                                            )).weak().italics());
                                        ui.separator();
                                        if let Some(weapon) = &item.item_type.weapon_stats {
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
                                        if let Some(armor) = &item.item_type.armor_stats {
                                            ui.label(RichText::new("Armor").strong().underline());
                                            ui.label(format!("AC: {}", armor));
                                            ui.separator();
                                        }
                                        if let Some(shield) = &item.item_type.shield_stats {
                                            ui.label(RichText::new("Shield").strong().underline());
                                            ui.label(format!("AC: {:+}", shield));
                                            ui.separator();
                                        }
                                        if let Some(container) = &item.item_type.container_stats {
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
                                    });
                                    response.context_menu(|ui| {
                                        if item.item_type.shield_stats.is_some() {
                                            if ui.button("Equip: Off Hand").clicked() {
                                                to_equip = Some((PlayerEquipSlot::LeftHand, i));
                                                ui.close_menu();
                                            }
                                        }
                                        if let Some(weapon) = &item.item_type.weapon_stats {
                                            match &weapon.damage {
                                                WeaponDamage::Melee(melee) => {
                                                    match melee  {
                                                        MeleeDamage::OneHanded(_) => {
                                                            if ui.button("Equip: Main Hand").clicked() {
                                                                to_equip = Some((PlayerEquipSlot::RightHand, i));
                                                                ui.close_menu();
                                                            }
                                                            if ui.button("Equip: Off Hand").clicked() {
                                                                to_equip = Some((PlayerEquipSlot::LeftHand, i));
                                                                ui.close_menu();
                                                            }
                                                        },
                                                        MeleeDamage::Versatile(_, _) => {
                                                            if ui.button("Equip: Main Hand").clicked() {
                                                                to_equip = Some((PlayerEquipSlot::RightHand, i));
                                                                ui.close_menu();
                                                            }
                                                            if ui.button("Equip: Both Hands").clicked() {
                                                                to_equip = Some((PlayerEquipSlot::BothHands, i));
                                                                ui.close_menu();
                                                            }
                                                            if ui.button("Equip: Off Hand").clicked() {
                                                                to_equip = Some((PlayerEquipSlot::LeftHand, i));
                                                                ui.close_menu();
                                                            }
                                                        },
                                                        MeleeDamage::TwoHanded(_) => {
                                                            if ui.button("Equip: Both Hands").clicked() {
                                                                to_equip = Some((PlayerEquipSlot::BothHands, i));
                                                                ui.close_menu();
                                                            }
                                                        },
                                                    }
                                                },
                                                WeaponDamage::Missile(_, _) => {
                                                    if ui.button("Equip: Both Hands").clicked() {
                                                        to_equip = Some((PlayerEquipSlot::BothHands, i));
                                                        ui.close_menu();
                                                    }
                                                },
                                            }
                                        }
                                        if item.item_type.armor_stats.is_some() {
                                            if ui.button("Equip: Armor").clicked() {
                                                to_equip = Some((PlayerEquipSlot::Armor, i));
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                });
                            });
                            if let Some((slot, index)) = to_equip {
                                sheet.equip_item(slot, index);
                                changed = true;
                            }
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.label(format!("Off hand: {}", sheet.inventory.get_equip_slot(PlayerEquipSlot::LeftHand).map_or("None", |i| &i.item_type.name)));
                                if sheet.inventory.left_hand.is_some() {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("Unequip").clicked() {
                                            sheet.unequip_item(PlayerEquipSlot::LeftHand);
                                            changed = true;
                                        }
                                    });
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Main hand: {}", sheet.inventory.get_equip_slot(PlayerEquipSlot::RightHand).map_or("None", |i| &i.item_type.name)));
                                if sheet.inventory.right_hand.is_some() {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("Unequip").clicked() {
                                            sheet.unequip_item(PlayerEquipSlot::RightHand);
                                            changed = true;
                                        }
                                    });
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("Armor: {}", sheet.inventory.get_equip_slot(PlayerEquipSlot::Armor).map_or("None", |i| &i.item_type.name)));
                                if sheet.inventory.armor.is_some() {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("Unequip").clicked() {
                                            sheet.unequip_item(PlayerEquipSlot::Armor);
                                            changed = true;
                                        }
                                    });   
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label(format!("{} cp", sheet.inventory.get_equip_slot(PlayerEquipSlot::CP).map_or(0, |i| i.count)));
                                ui.weak("|");
                                ui.label(format!("{} sp", sheet.inventory.get_equip_slot(PlayerEquipSlot::SP).map_or(0, |i| i.count)));
                                ui.weak("|");
                                ui.label(format!("{} ep", sheet.inventory.get_equip_slot(PlayerEquipSlot::EP).map_or(0, |i| i.count)));
                                ui.weak("|");
                                ui.label(format!("{} gp", sheet.inventory.get_equip_slot(PlayerEquipSlot::GP).map_or(0, |i| i.count)));
                                ui.weak("|");
                                ui.label(format!("{} pp", sheet.inventory.get_equip_slot(PlayerEquipSlot::PP).map_or(0, |i| i.count)));
                            });
                            ui.separator();
                            ui.label(format!("Encumbrance: {:.2}", sheet.inventory.total_weight()));
                        },
                        CharacterSheetTab::Proficiencies => {
                            ui.horizontal(|ui| {
                                ui.colored_label(Color32::GREEN, format!("General slots: {}", sheet.proficiencies.general_slots));
                                if ui.small_button(RichText::new("-").color(Color32::GREEN)).clicked() {
                                    if sheet.proficiencies.general_slots > 0 {
                                        sheet.proficiencies.general_slots -= 1;
                                        changed = true;
                                    }
                                }
                                if ui.small_button(RichText::new("+").color(Color32::GREEN)).clicked() {
                                    if sheet.proficiencies.general_slots < 255 {
                                        sheet.proficiencies.general_slots += 1;
                                        changed = true;
                                    }
                                }
                                ui.colored_label(Color32::YELLOW, format!("Class slots: {}", sheet.proficiencies.class_slots));
                                if ui.small_button(RichText::new("-").color(Color32::YELLOW)).clicked() {
                                    if sheet.proficiencies.class_slots > 0 {
                                        sheet.proficiencies.class_slots -= 1;
                                        changed = true;
                                    }
                                }
                                if ui.small_button(RichText::new("+").color(Color32::YELLOW)).clicked() {
                                    if sheet.proficiencies.class_slots < 255 {
                                        sheet.proficiencies.class_slots += 1;
                                        changed = true;
                                    }
                                }
                            });
                            ui.separator();
                            let mut remove = None;
                            for ((id, spec), prof) in &sheet.proficiencies.profs {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(prof.display()).strong());
                                    ui.add_space(5.0);
                                    if link_button(ui) {
                                        data.temp_state.viewed_prof_spec = None;
                                        data.temp_state.viewed_prof = Some(id.clone());
                                        (self.callback)(DMTab::ProficiencyViewer, true);
                                    }
                                    if x_button(ui) {
                                        remove = Some((id.clone(), spec.clone()));
                                    }
                                });
                            }
                            if let Some(id) = remove {
                                sheet.remove_prof(&id);
                                changed = true;
                            }
                        },
                        CharacterSheetTab::Spells => {
                            if let Some(divine) = &mut sheet.divine_spells {
                                ui.vertical_centered(|ui| {
                                    ui.heading("Divine Spellcaster");
                                    if ui.button("Restore all").clicked() {
                                        divine.restore_all();
                                        changed = true;
                                    }
                                    ui.add_space(5.0);
                                    for (i, (curr, max)) in divine.spell_slots.iter_mut().enumerate() {
                                        if *max > 0 {
                                            ui.horizontal(|ui| {
                                                let mut show_slots = |ui: &mut egui::Ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.label(format!("Level {}:", i + 1));
                                                        for j in 0..*max {
                                                            if *curr > j {
                                                                ui.colored_label(Color32::GREEN, format!("{}", ep::STAR));
                                                            } else {
                                                                ui.colored_label(Color32::RED, format!("{}", ep::STAR_HALF));
                                                            }
                                                        }
                                                        ui.label(RichText::new(format!("{}/{}", curr, max)).weak());
                                                        if ui.small_button("-").clicked() {
                                                            if *curr > 0 {
                                                                *curr -= 1;
                                                                changed = true;
                                                            }
                                                        }
                                                        if ui.small_button("+").clicked() {
                                                            if *curr < *max {
                                                                *curr += 1;
                                                                changed = true;
                                                            }
                                                        }
                                                    });
                                                };
                                                if egui::CollapsingHeader::new(RichText::new("").size(1.0))
                                                    .id_source(format!("{}_{}_divine_spells_{}", user, name, i))
                                                    .show_unindented(ui, |ui| {
                                                        show_slots(ui);
                                                        ui.separator();
                                                        for spell_id in &divine.spell_repertoire[i] {
                                                            if let Some(spell) = data.spell_registry.divine[i].get(spell_id) {
                                                                ui.horizontal(|ui| {
                                                                    ui.label(&spell.name);
                                                                    if link_button(ui) {
                                                                        data.temp_state.viewed_spell = Some((spell.magic_type, Some((spell.spell_level, Some(spell_id.clone())))));
                                                                        (self.callback)(DMTab::SpellViewer, true);
                                                                    }
                                                                });       
                                                            }
                                                        }
                                                    }).body_returned.is_none() {
                                                        show_slots(ui);
                                                    }
                                            });
                                            ui.add_space(4.0);
                                        }
                                    }
                                });
                            } 
                            if let Some(arcane) = &mut sheet.arcane_spells {
                                ui.vertical_centered(|ui| {
                                    ui.heading("Arcane Spellcaster");
                                    if ui.button("Restore all").clicked() {
                                        arcane.restore_all();
                                        changed = true;
                                    }
                                    ui.add_space(5.0);
                                    for (i, (curr, max)) in arcane.spell_slots.iter_mut().enumerate() {
                                        if *max > 0 {
                                            ui.horizontal(|ui| {
                                                let mut show_slots = |ui: &mut egui::Ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.label(format!("Level {}:", i + 1));
                                                        for j in 0..*max {
                                                            if *curr > j {
                                                                ui.colored_label(Color32::GREEN, format!("{}", ep::STAR));
                                                            } else {
                                                                ui.colored_label(Color32::RED, format!("{}", ep::STAR_HALF));
                                                            }
                                                        }
                                                        ui.label(RichText::new(format!("{}/{}", curr, max)).weak());
                                                        if ui.small_button("-").clicked() {
                                                            if *curr > 0 {
                                                                *curr -= 1;
                                                                changed = true;
                                                            }
                                                        }
                                                        if ui.small_button("+").clicked() {
                                                            if *curr < *max {
                                                                *curr += 1;
                                                                changed = true;
                                                            }
                                                        }
                                                    });
                                                };
                                                if egui::CollapsingHeader::new(RichText::new("").size(1.0))
                                                    .id_source(format!("{}_{}_arcane_spells_{}", user, name, i))
                                                    .show_unindented(ui, |ui| {
                                                        show_slots(ui);
                                                        ui.label(format!("Repertoire size: {}/{}", arcane.spell_repertoire[i].0.len(),  arcane.spell_repertoire[i].1));
                                                        ui.separator();
                                                        for spell_id in &arcane.spell_repertoire[i].0 {
                                                            if let Some(spell) = data.spell_registry.arcane[i].get(spell_id) {
                                                                ui.horizontal(|ui| {
                                                                    ui.label(&spell.name);
                                                                    if link_button(ui) {
                                                                        data.temp_state.viewed_spell = Some((spell.magic_type, Some((spell.spell_level, Some(spell_id.clone())))));
                                                                        (self.callback)(DMTab::SpellViewer, true);
                                                                    }
                                                                });       
                                                            }
                                                        }
                                                    }).body_returned.is_none() {
                                                        show_slots(ui);
                                                    }
                                            });
                                            ui.add_space(4.0);
                                        }
                                    }
                                });
                            } 
                            if sheet.divine_spells.is_none() && sheet.arcane_spells.is_none() {
                                ui.vertical_centered(|ui| {
                                    ui.label("This character isn't a spellcaster.")
                                });
                            }
                        },
                        CharacterSheetTab::Notes => {
                            ui.label(&sheet.notes);
                        },
                    }
                });
                if changed {
                    let sheet = sheet.clone();
                    data.send_to_user(ClientBoundPacket::UpdateCharacter(name.clone(), sheet), user.clone());
                }
            } else {
                ui.colored_label(ui.visuals().error_fg_color, "Something went wrong. This character doesn't appear to exist!");
            }
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Something went wrong. This user doesn't appear to exist!");
        }
        if let Some(msg) = msg {
            data.log(msg);
        }
        if parties_changed {
            data.send_to_all_players(ClientBoundPacket::UpdateParties(data.parties.clone()));
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
        let (viewed, _, _) = registry_viewer(
            ui, 
            &data.enemy_type_registry, 
            data.temp_state.viewed_enemy.clone(), 
            |e| format!("View: {}", e.name).into(), 
            |_| None, 
            || None, 
            |_, _, _| {}, 
            |ui, _, enemy| {
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
        );
        data.temp_state.viewed_enemy = viewed;
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
                        dice_roll_editor_simple(ui, roll);
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
                        damage_roll_editor(ui, roll1);
                    },
                    AttackRoutine::Two(roll1, roll2) => {
                        ui.label("Damage roll (first):");
                        damage_roll_editor(ui, roll1);
                        ui.add_space(3.0);
                        ui.label("Damage roll (second):");
                        damage_roll_editor(ui, roll2);
                    },
                    AttackRoutine::Three(roll1, roll2, roll3) => {
                        ui.label("Damage roll (first):");
                        damage_roll_editor(ui, roll1);
                        ui.add_space(3.0);
                        ui.label("Damage roll (second):");
                        damage_roll_editor(ui, roll2);
                        ui.add_space(3.0);
                        ui.label("Damage roll (third):");
                        damage_roll_editor(ui, roll3);
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
        let (viewed, _, _) = registry_viewer(
            ui,
            &data.item_type_registry,
            data.temp_state.viewed_item.clone(),
            |i| format!("View: {}", i.name).into(),
            |_| None,
            || None,
            |ui, _, item| {
                ui.menu_button("Give to...", |ui| {
                    ui.add(egui::Slider::new(&mut data.temp_state.item_give_count, 1..=1000).clamp_to_range(false).text("Count"));
                    ui.separator();
                    for (user, _) in &data.known_users {
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
            },
            |ui, _, item| {
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
        );
        data.temp_state.viewed_item = viewed;
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
                                    damage_roll_editor(ui, dmg);
                                },
                                MeleeDamage::Versatile(dmg1, dmg2) => {
                                    ui.label("One-Handed:");
                                    damage_roll_editor(ui, dmg1);
                                    ui.label("Two-Handed:");
                                    damage_roll_editor(ui, dmg2);
                                },
                                MeleeDamage::TwoHanded(dmg) => {
                                    damage_roll_editor(ui, dmg);
                                },
                            }
                        },
                        WeaponDamage::Missile(missile, ammo) => {
                            ui.label("Damage:");
                            damage_roll_editor(ui, missile);
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
                            for (user, _) in &mut data.known_users {
                                if let Some(user_data) = data.user_data.get_mut(user) {
                                    for (name, sheet) in &mut user_data.characters {
                                        if ui.button(format!("{} ({})", name, user)).clicked() {
                                            if let Some(prof_instance) = sheet.proficiencies.profs.get(&(id.clone(), data.temp_state.viewed_prof_spec.clone())) {
                                                if prof_instance.prof_level < prof.max_level {
                                                    let lvl = prof_instance.prof_level + 1;
                                                    sheet.remove_prof(&(id.clone(), data.temp_state.viewed_prof_spec.clone()));
                                                    let mut p = ProficiencyInstance::from_prof(prof.clone(), data.temp_state.viewed_prof_spec.clone());
                                                    p.prof_level = lvl;
                                                    sheet.add_prof(id, p);
                                                }
                                            } else {
                                                sheet.add_prof(id, ProficiencyInstance::from_prof(prof.clone(), data.temp_state.viewed_prof_spec.clone()));
                                            }
                                            data.temp_state.viewed_prof_spec = None;
                                            packets.push((ClientBoundPacket::UpdateCharacter(name.clone(), sheet.clone()), user.clone()));
                                            ui.close_menu();
                                        }
                                    }
                                }
                            } 
                        });
                        if prof.requires_specification {
                            if let Some(spec) = &mut data.temp_state.viewed_prof_spec {
                                if let Some(valid) = &prof.valid_specifications {
                                    egui::ComboBox::from_id_source("specification")
                                        .selected_text(&*spec)
                                        .show_ui(ui, |ui| {
                                            for v in valid {
                                                ui.selectable_value(spec, v.clone(), v);
                                            }
                                        });
                                } else {
                                    ui.add(egui::TextEdit::singleline(spec).hint_text("Specify..."));
                                }
                            } else {
                                if let Some(valid) = &prof.valid_specifications {
                                    data.temp_state.viewed_prof_spec = Some(valid.iter().next().cloned().unwrap_or("".to_owned()));
                                } else {
                                    data.temp_state.viewed_prof_spec = Some("".to_owned());
                                }
                            }
                        } else {
                            data.temp_state.viewed_prof_spec = None;
                        }
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
            data.temp_state.viewed_prof_spec = None;
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
                                    Some(spell_id) => {
                                        if let Some(arcane) = data.spell_registry.arcane.get(*lvl as usize) {
                                            if let Some(spell) = arcane.get(spell_id) {
                                                ui.horizontal(|ui| {
                                                    ui.heading(&spell.name);
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.menu_button("Give to...", |ui| {
                                                            for (user, user_data) in &mut data.user_data {
                                                                for (name, sheet) in &mut user_data.characters {
                                                                    if let Some(arcane) = &mut sheet.arcane_spells {
                                                                        if let Some((rep, max)) = arcane.spell_repertoire.get_mut(*lvl as usize) {
                                                                            if rep.len() < *max as usize {
                                                                                if !rep.contains(spell_id) {
                                                                                    if ui.button(format!("{} ({})", name, user)).clicked() {
                                                                                        rep.insert(spell_id.clone());
                                                                                        ui.close_menu();
                                                                                    }
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        });
                                                    });
                                                });
                                                ui.horizontal(|ui| {
                                                    ui.label(format!("{} {}{}", spell.magic_type, spell.spell_level + 1, if spell.reversed.is_some() {" (Reversible)"} else {""}));
                                                    if let Some(reversed) = &spell.reversed {
                                                        if link_button(ui) {
                                                            *maybe_spell = Some(reversed.clone());
                                                        }
                                                    }
                                                });
                                                ui.label(format!("Range: {}", spell.range));
                                                ui.label(format!("Duration: {}", spell.duration));
                                                ui.separator();
                                                ui.label(RichText::new(&spell.description).weak().italics());
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
                                                ui.heading(&spell.name);
                                                ui.horizontal(|ui| {
                                                    ui.label(format!("{} {}{}", spell.magic_type, spell.spell_level + 1, if spell.reversed.is_some() {" (Reversible)"} else {""}));
                                                    if let Some(reversed) = &spell.reversed {
                                                        if link_button(ui) {
                                                            *maybe_spell = Some(reversed.clone());
                                                        }
                                                    }
                                                });
                                                ui.label(format!("Range: {}", spell.range));
                                                ui.label(format!("Duration: {}", spell.duration));
                                                ui.separator();
                                                ui.label(RichText::new(&spell.description).weak().italics());
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
    fn class_viewer(&mut self, ui: &mut Ui) {
        let data = &mut *self.data;
        let (viewed, _, _) = registry_viewer(
            ui,
            &data.class_registry,
            data.temp_state.viewed_class.clone(),
            |c| format!("View: {}", c.name).into(),
            |_| None,
            || None,
            |ui, path, class| {
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
                    (self.callback)(DMTab::ClassCreator, true);
                }
            },
            |ui, _, class| {
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
        );
        data.temp_state.viewed_class = viewed;
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
    fn combat(&mut self, ui: &mut Ui) {
        let data = &mut *self.data;
        let fight_cloned: Option<Fight>;
        match &mut data.loaded_map {
            Some((_, map)) => {
                let map_name = map.name.clone();
                let mut maybe_fight = map.fight.clone();
                let mut end_combat = false;
                match &mut maybe_fight {
                    Some(fight) => {
                        if fight.started {
                            if let Some((turn, turn_type)) = &mut fight.current_turn {
                                ui.label("Turn order:");
                                for (i, (_, comb, _)) in fight.turn_order.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        if i == *turn {
                                            ui.label(RichText::new(format!("- {} ({})", comb, turn_type)).strong());
                                        } else {
                                            ui.label(format!("- {}", comb));
                                        }
                                        if link_button_frameless(ui) {
                                            match comb {
                                                Combatant::Enemy { room, .. } => {
                                                    (self.callback)(DMTab::MapViewer, true);
                                                    tree_callback(&mut self.map_tree, MapTab::RoomEnemies(room.clone()), true);
                                                },
                                                Combatant::PC { user, name } => {
                                                    (self.callback)(DMTab::PlayerCharacter(user.clone(), name.clone()), true);
                                                },
                                            }
                                        }
                                    });
                                }
                                ui.separator();
                                if let Some((owner, comb, declaration)) = fight.turn_order.get(*turn) {
                                    ui.label(format!("It is {}'s turn.", comb));
                                    match declaration {
                                        PreRoundAction::FightingWithdrawal => {
                                            ui.label("Remember, they declared a fighting withdrawal.");
                                        },
                                        PreRoundAction::FullRetreat => {
                                            ui.label("Remember, they declared a full retreat.");
                                        },
                                        PreRoundAction::CastSpell(id, lvl, mt) => {
                                            ui.label(format!("Remember, they declared that they are casting {}.", data.spell_registry.get_spell_name_or(id, *lvl, *mt, "Nonexistent Spell")));
                                        },
                                        PreRoundAction::None => {},
                                    }
                                    match turn_type {
                                        TurnType::Movement { action, player_action } => {
                                            let mut act = false;
                                            let mut deny: Option<String> = None;
                                            if let Owner::Player(player) = owner {
                                                if let Some(player_action) = player_action {
                                                    ui.horizontal(|ui| {
                                                        ui.label(format!("{} wants to: {}. Is this allowed? You can act for them if it isn't.", player, player_action));
                                                        if ui.small_button(RichText::new(format!("{}", ep::CHECK)).color(Color32::GREEN)).on_hover_text("Approve").clicked() {
                                                            *action = player_action.clone();
                                                            act = true;
                                                        }
                                                        if ui.small_button(RichText::new(format!("{}", ep::COPY)).color(Color32::YELLOW)).on_hover_text("Modify").clicked() {
                                                            *action = player_action.clone();
                                                        }
                                                        if ui.small_button(RichText::new(format!("{}", ep::X)).color(Color32::RED)).on_hover_text("Deny").clicked() {
                                                            deny = Some(player.clone());
                                                        }
                                                    });
                                                } else {
                                                    ui.label(format!("{} has not chosen an action yet.", player));
                                                }
                                                ui.separator();
                                            }
                                            egui::ComboBox::from_label("Movement Action")
                                                .selected_text(action.to_string())
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(action, MovementAction::None, "None");
                                                    ui.selectable_value(action, MovementAction::Move, "Move");
                                                    ui.selectable_value(action, MovementAction::Run, "Run");
                                                    ui.selectable_value(action, MovementAction::Charge, "Charge");
                                                    ui.selectable_value(action, MovementAction::FightingWithdrawal, "Fighting Withdrawal");
                                                    ui.selectable_value(action, MovementAction::FullRetreat, "Full Retreat");
                                                    ui.selectable_value(action, MovementAction::SimpleAction, "Simple Action");
                                                });
                                            if let Some(player) = deny {
                                                *player_action = None;
                                                fight.update_specific_client(data, player);
                                            } 
                                            if ui.button("Act").clicked() || act {
                                                fight.resolve_action(data);
                                            }
                                        },
                                        TurnType::Attack { action, player_action } => {
                                            let mut act = false;
                                            let mut deny: Option<String> = None;
                                            if let Owner::Player(player) = owner {
                                                if let Some(player_action) = player_action {
                                                    ui.horizontal(|ui| {
                                                        ui.label(format!("{} wants to: {}. Is this allowed? You can act for them if it isn't.", player, player_action.display_alt()));
                                                        if ui.small_button(RichText::new(format!("{}", ep::CHECK)).color(Color32::GREEN)).on_hover_text("Approve").clicked() {
                                                            *action = player_action.clone();
                                                            act = true;
                                                        }
                                                        if ui.small_button(RichText::new(format!("{}", ep::COPY)).color(Color32::YELLOW)).on_hover_text("Modify").clicked() {
                                                            *action = player_action.clone();
                                                        }
                                                        if ui.small_button(RichText::new(format!("{}", ep::X)).color(Color32::RED)).on_hover_text("Deny").clicked() {
                                                            deny = Some(player.clone());
                                                        }
                                                    });
                                                } else {
                                                    ui.label(format!("{} has not chosen an action yet.", player));
                                                }
                                                ui.separator();
                                            }
                                            egui::ComboBox::from_label("Attack Action")
                                                .selected_text(action.to_string())
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(action, AttackAction::None, "None");
                                                    ui.selectable_value(action, AttackAction::Attack(comb.clone(), 0), "Attack");
                                                    ui.selectable_value(action, AttackAction::SpecialManeuver(comb.clone(), SpecialManeuver::Disarm, 0), "Special Maneuver");
                                                    ui.selectable_value(action, AttackAction::CastSpell, "Cast Spell");
                                                    ui.selectable_value(action, AttackAction::OtherAction, "Other Action");
                                                });
                                            match action {
                                                AttackAction::Attack(target, modifier) => {
                                                    egui::ComboBox::from_label("Target")
                                                        .selected_text(target.to_string())
                                                        .show_ui(ui, |ui| {
                                                            for (_, t, _) in &fight.turn_order {
                                                                ui.selectable_value(target, t.clone(), t.to_string());
                                                            }
                                                        });
                                                    egui::ComboBox::from_label("Situational Modifier")
                                                        .selected_text(format!("{:+}", modifier))
                                                        .show_ui(ui, |ui| {
                                                            ui.selectable_value(modifier, -4, "Blind");
                                                            ui.add(egui::DragValue::new(modifier).prefix("Custom:"));
                                                        });
                                                },
                                                AttackAction::SpecialManeuver(target, maneuver, _modifier) => {
                                                    egui::ComboBox::from_label("Target")
                                                        .selected_text(target.to_string())
                                                        .show_ui(ui, |ui| {
                                                            for (_, t, _) in &fight.turn_order {
                                                                ui.selectable_value(target, t.clone(), t.to_string());
                                                            }
                                                        });
                                                    egui::ComboBox::from_label("Maneuver")
                                                        .selected_text(maneuver.to_string())
                                                        .show_ui(ui, |ui| {
                                                            ui.selectable_value(maneuver, SpecialManeuver::Disarm, "Disarm");
                                                            ui.selectable_value(maneuver, SpecialManeuver::ForceBack, "Force Back");
                                                            ui.selectable_value(maneuver, SpecialManeuver::Incapacitate, "Incapacitate");
                                                            ui.selectable_value(maneuver, SpecialManeuver::KnockDown, "Knock Down");
                                                            ui.selectable_value(maneuver, SpecialManeuver::Sunder, "Sunder");
                                                            ui.selectable_value(maneuver, SpecialManeuver::Wrestle, "Wrestle");
                                                        });
                                                },
                                                _ => {},
                                            }
                                            if let Some(player) = deny {
                                                *player_action = None;
                                                fight.update_specific_client(data, player);
                                            } 
                                            if ui.button("Act").clicked() || act {
                                                fight.resolve_action(data);
                                            }
                                        },
                                    }
                                } else {
                                    ui.colored_label(Color32::RED, "Something has gone horribly wrong.");
                                }
                            } else {
                                ui.vertical(|ui| {
                                    ui.label("Initiative has not been calculated yet. Any pre-round declarations are listed (right-click to deny):");
                                    ui.add_space(4.0);
                                    let mut maybe_remove = None;
                                    for (comb, action) in &fight.declarations {
                                        let res = if let PreRoundAction::CastSpell(id, lvl, magic_type) = action {
                                            let name = match magic_type {
                                                MagicType::Arcane => {
                                                    data.spell_registry.arcane.get(*lvl as usize).and_then(|reg| reg.get(id).map(|s| s.name.as_str()))
                                                },
                                                MagicType::Divine => {
                                                    data.spell_registry.divine.get(*lvl as usize).and_then(|reg| reg.get(id).map(|s| s.name.as_str()))
                                                },
                                            };
                                            ui.horizontal(|ui| {
                                                let res = ui.add(Label::new(format!("- {}: Cast Spell ({})", comb, name.unwrap_or("Nonexistent spell"))).sense(Sense::click()));
                                                if ui.small_button(format!("{}", ep::ARROW_SQUARE_OUT)).clicked() {
                                                    data.temp_state.viewed_spell = Some((*magic_type, Some((*lvl, Some(id.clone())))));
                                                    (self.callback)(DMTab::SpellViewer, true);
                                                }
                                                res
                                            }).inner
                                        } else {
                                            ui.add(Label::new(format!("- {}: {}", comb, action)).sense(Sense::click()))
                                        };
                                        res.context_menu(|ui| {
                                            if ui.button("Deny").clicked() {
                                                maybe_remove = Some(comb.clone());
                                                ui.close_menu();
                                            }
                                        });
                                    }
                                    if let Some(typ) = maybe_remove {
                                        fight.declarations.remove(&typ);
                                    }
                                    ui.separator();
                                    if ui.button("Calculate initiative").clicked() {
                                        fight.start_round(data);
                                    }
                                });
                            }
                            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                                if ui.button("End combat").clicked() {
                                    data.log(ChatMessage::no_sender(format!("Combat has concluded in {}!", map_name)).combat());
                                    data.send_to_all_players(ClientBoundPacket::UpdateCombatState(None));
                                    end_combat = true;
                                }
                            });
                        } else {
                            ui.label("The fight has not started.");
                            ui.menu_button("Add combatants...", |ui| {
                                ui.menu_button("Enemies", |ui| {
                                    for (room_id, room) in &map.rooms {
                                        if ui.button(format!("Room {}", room_id)).clicked() {
                                            for (type_id, (typ, group)) in &room.enemies {
                                                for (i, _) in group.iter().enumerate() {
                                                    fight.combatants.insert((Owner::DM, Combatant::enemy_auto_name(room_id.clone(), type_id.clone(), i, typ.name.clone())));
                                                }
                                            }
                                            ui.close_menu();
                                        }
                                    }
                                });
                                ui.menu_button("Parties", |ui| {
                                    for (name, party) in &data.parties {
                                        if ui.button(RichText::new(name).color(party.color)).clicked() {
                                            for (user, name) in &party.members {
                                                fight.combatants.insert((Owner::Player(user.clone()), Combatant::pc(user.clone(), name.clone())));
                                            }
                                            ui.close_menu();
                                        }
                                    }
                                });
                            });
                            ui.separator();
                            ui.label(RichText::new("Combatants").size(15.0));
                            ui.indent("combatants", |ui| {
                                let mut remove = None;
                                for (owner, combatant) in &fight.combatants {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("- {}", combatant));
                                        if trash_button_frameless(ui) {
                                            remove = Some((owner, combatant));
                                        }
                                    });
                                }
                                if let Some((o, c)) = remove {
                                    fight.combatants.remove(&(o.clone(), c.clone()));
                                }
                                if fight.combatants.is_empty() {
                                    ui.label(RichText::new("There's nothing here...").weak().italics());
                                }
                            });
                            ui.separator();
                            if ui.button("Start!").clicked() {
                                data.log(ChatMessage::no_sender(format!("Combat has broken out in {}!", map_name)).combat());
                                fight.started = true;
                                fight.update_clients(data);
                            }
                            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                                if ui.button("End combat").clicked() {
                                    end_combat = true;
                                }
                            });
                        }
                    },
                    None => {
                        ui.label(format!("There is currently no fight in {}.", map.name));
                        if ui.button("Create fight").clicked() {
                            maybe_fight = Some(Fight::new());
                        }
                    },
                }
                if end_combat {
                    maybe_fight = None;
                }
                fight_cloned = maybe_fight;
            },
            None => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("There is not currently any map loaded. A fight requires a map to take place in. Use the ");
                    if ui.link("map viewer").clicked() {
                        (self.callback)(DMTab::MapViewer, true);
                    }
                    ui.label(" to load a map.");
                });
                fight_cloned = None;
            },
        }
        if let Some((_, map)) = &mut data.loaded_map {
            map.fight = fight_cloned;
        }
    }
    fn parties(&mut self, ui: &mut Ui) {
        let data = &mut *self.data;
        let mut packets = Vec::new();
        let mut changed = false;
        let mut msg = None;
        for (name, party) in &mut data.parties {
            ui.label(RichText::new(name).color(party.color).size(16.0));
            ui.indent(name, |ui| {
                ui.label(format!("XP: {}", party.temporary_xp)).on_hover_text("This is the amount of XP that the party has gained since they were adventuring.");
                ui.label("Members:");
                let mut remove = None;
                for (user, character) in &party.members {
                    ui.horizontal(|ui| {
                        ui.label(format!("- {} ({})", character, user));
                        if link_button(ui) {
                            (self.callback)(DMTab::PlayerCharacter(user.clone(), character.clone()), true);
                        }
                        if x_button(ui) {
                            remove = Some((user.clone(), character.clone()));
                        }
                    });
                }
                if let Some((user, character)) = remove {
                    if let Some(user_data) = data.user_data.get_mut(&user) {
                        if let Some(sheet) = user_data.characters.get_mut(&character) {
                            sheet.party = None;
                            packets.push((user.clone(), ClientBoundPacket::UpdateCharacter(character.clone(), sheet.clone())));
                            msg = Some(ChatMessage::no_sender(format!("{} has left {}!", character, name)).parties().color(party.color));
                        }
                    }
                    party.members.remove(&(user, character));
                    changed = true;
                }
            });
            ui.separator();
        }
        if let Some(msg) = msg {
            data.log(msg);
        }
        if let Some((name, party)) = &mut data.temp_state.temp_party {
            ui.horizontal(|ui| {
                ui.label("Party name:");
                ui.text_edit_singleline(name);
            });
            ui.horizontal(|ui| {
                ui.label("Party color:");
                egui::color_picker::color_edit_button_srgba(ui, &mut party.color, egui::color_picker::Alpha::Opaque);
            });
            if ui.button("Create").clicked() {
                if !data.parties.contains_key(name) {
                    data.parties.insert(name.clone(), party.clone());
                    let name = name.clone();
                    let color = party.color;
                    data.log(ChatMessage::no_sender(format!("{} has been created!", name)).parties().color(color));
                    data.temp_state.temp_party = None;
                    changed = true;
                }
            }
        } else {
            if ui.button("Create new").clicked() {
                data.temp_state.temp_party = Some((String::new(), Party::new()));
            }
        }
        for (user, packet) in packets {
            data.send_to_user(packet, user);
        }
        if changed {
            data.send_to_all_players(ClientBoundPacket::UpdateParties(data.parties.clone()));
        }
    }
    fn map_creator(&mut self, ui: &mut Ui) {
        let data = &mut *self.data;
        ui.horizontal(|ui| {
            ui.label("Map name:");
            ui.text_edit_singleline(&mut data.temp_state.temp_map_name);
        });
        ui.horizontal(|ui| {
            ui.label("Map filename:");
            ui.text_edit_singleline(&mut data.temp_state.temp_map_filename);
        });
        if ui.button("Create").clicked() {
            let mut map = Map::new();
            map.name = data.temp_state.temp_map_name.clone();
            let _ = save_ron(&map, "maps", &data.temp_state.temp_map_filename);
            data.temp_state.temp_map_filename = "map".to_owned();
            data.temp_state.temp_map_name.clear();
            data.register_maps();
        }
        ui.label(RichText::new("Edit your newly created map in the map viewer.").weak().italics());
    }
    fn map_viewer(&mut self, ui: &mut Ui) {
        let data = &mut *self.data;
        if data.loaded_map.is_some() {
            egui::Frame::group(ui.style())
                .stroke(egui::Stroke::new(7.0, ui.visuals().extreme_bg_color))
                .inner_margin(egui::Margin::same(0.0))
                .outer_margin(egui::Margin::same(0.0))
                .show(ui, |ui| {
                    if self.map_tree.is_empty() {
                        self.map_tree.push_to_first_leaf(MapTab::Main);
                    }
                    let mut new_tab = None;
                    let mut remove_tab = None;
                    DockArea::new(self.map_tree)
                        .id(Id::new("map_dock_area"))
                        .show_inside(ui, &mut MapTabViewer {
                            callback_outer: |tab, open| {
                                (self.callback)(tab, open);
                            },
                            callback_inner: |tab, open| {
                                if open {
                                    new_tab = Some(tab);
                                } else {
                                    remove_tab = Some(tab);
                                }
                            },
                            data,
                        });
                    if let Some(tab) = new_tab {
                        if let Some((node_i, tab_i)) = self.map_tree.find_tab(&tab) {
                            self.map_tree.set_focused_node(node_i);
                            self.map_tree.set_active_tab(node_i, tab_i);
                        } else {
                            self.map_tree.push_to_focused_leaf(tab);
                        }
                    }
                    if let Some(tab) = remove_tab {
                        if let Some(i) = self.map_tree.find_tab(&tab) {
                            self.map_tree.remove_tab(i);
                        }
                    }
                });
        } else {
            for (id, name) in &data.map_registry {
                if ui.button(format!("Load: {}", name)).clicked() {
                    if let Ok(s) = std::fs::read_to_string(format!("maps/{}.ron", id)) {
                        if let Ok(map) = ron::from_str::<Map>(&s) {
                            data.loaded_map = Some((id.clone(), map));
                        }
                    }
                }
            }
        }
    }
}

impl<F: FnMut(DMTab, bool)> TabViewer for DMTabViewer<'_, F> {
    type Tab = DMTab;

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
                self.class_viewer(ui);
            },
            DMTab::ClassCreator => {
                Self::class_creator(ui, self.data);
            },
            DMTab::Combat => {
                self.combat(ui);
            },
            DMTab::Parties => {
                self.parties(ui);
            },
            DMTab::MapViewer => {
                self.map_viewer(ui);
            },
            DMTab::MapCreator => {
                self.map_creator(ui);
            },
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        if *tab == DMTab::Chat {
            self.data.get_chat_title()
        } else {
            tab.to_string().into()
        }
    }
}

#[simple_enum(no_copy)]
pub enum DMTab {
    DiceRoller,
    PlayerList,
    Player(String),
    PlayerCharacter(String, String),
    Chat,
    EnemyViewer,
    EnemyCreator,
    ItemViewer,
    ItemCreator,
    ProficiencyViewer,
    ProficiencyCreator,
    SpellViewer,
    SpellCreator,
    ClassViewer,
    ClassCreator,
    Combat,
    Parties,
    MapViewer,
    MapCreator,
}

impl std::fmt::Display for DMTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::DiceRoller => "Dice Roller".to_owned(),
            Self::PlayerList => "Player List".to_owned(),
            Self::EnemyViewer => "Enemy Viewer".to_owned(),
            Self::EnemyCreator => "Enemy Creator".to_owned(),
            Self::ItemViewer => "Item Viewer".to_owned(),
            Self::ItemCreator => "Item Creator".to_owned(),
            Self::ProficiencyViewer => "Proficiency Viewer".to_owned(),
            Self::ProficiencyCreator => "Proficiency Creator".to_owned(),
            Self::SpellViewer => "Spell Viewer".to_owned(),
            Self::SpellCreator => "Spell Creator".to_owned(),
            Self::ClassViewer => "Class Viewer".to_owned(),
            Self::ClassCreator => "Class Creator".to_owned(),
            Self::Parties => "Parties".to_owned(),
            Self::Combat => ep::SWORD.to_owned(),
            Self::Chat => ep::CHAT_TEXT.to_owned(),
            Self::MapViewer => "Map Viewer".to_owned(),
            Self::MapCreator => "Map Creator".to_owned(),
            Self::PlayerCharacter(player, name) => format!("{} ({})", name, player),
            Self::Player(player) => format!("Player ({})", player),
        })
    }
}

#[simple_enum(no_copy)]
pub enum MapTab {
    Main,
    Room(String),
    RoomEnemies(String),
    RoomItems(String),
    RoomConnections(String),
}

impl std::fmt::Display for MapTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Main => ep::MAP_TRIFOLD.to_owned(),
            Self::Room(room) => format!("Room {}", room),
            Self::RoomEnemies(room) => format!("{} (Room {})", ep::SWORD, room),
            Self::RoomItems(room) => format!("{} (Room {})", ep::COINS, room),
            Self::RoomConnections(room) => format!("{} (Room {})", ep::DOOR_OPEN, room),
        })
    }
}

pub struct MapTabViewer<'a, O, I> 
where 
    O: FnMut(DMTab, bool),
    I: FnMut(MapTab, bool),
{
    pub callback_outer: O,
    pub callback_inner: I,
    pub data: &'a mut DMAppData,
}

impl<O, I> TabViewer for MapTabViewer<'_, O, I> 
where 
    O: FnMut(DMTab, bool),
    I: FnMut(MapTab, bool),
{
    type Tab = MapTab;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.to_string().into()
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> bool {
        if *tab == MapTab::Main {
            false
        } else {
            true
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            MapTab::Main => {
                self.main_tab(ui);
            },
            MapTab::Room(room) => {
                self.room(ui, room);
            },
            MapTab::RoomEnemies(room) => {
                self.room_enemies(ui, room);
            },
            MapTab::RoomItems(room) => {
                self.room_items(ui, room);
            },
            MapTab::RoomConnections(room) => {
                self.room_connections(ui, room);
            },
        }
    }
}

impl<O, I> MapTabViewer<'_, O, I> 
where 
    O: FnMut(DMTab, bool),
    I: FnMut(MapTab, bool),
{
    fn edit_mode(data: &mut AppTempState, ui: &mut Ui) {
        ui.checkbox(&mut data.map_editing_mode, format!("{}", ep::NOTE_PENCIL)).on_hover_text("Edit mode");
    }

    fn main_tab(&mut self, ui: &mut Ui) {
        let data = &mut *self.data;
        let mut unload = false;
        if let Some((_, map)) = &mut data.loaded_map {
            ui.horizontal(|ui| {
                ui.heading(&map.name);
                ui.menu_button("...", |ui| {
                    if ui.button("Save and Unload").clicked() {
                        unload = true;
                        ui.close_menu();
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    Self::edit_mode(&mut data.temp_state, ui);
                });
            });
            let edit = data.temp_state.map_editing_mode;
            if edit {
                ui.add(TextEdit::multiline(&mut map.summary).hint_text("Summary..."));
            } else {
                if !map.summary.is_empty() {
                    ui.label(RichText::new(&map.summary).weak().italics());
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Rooms").size(16.0));
                ui.add_space(4.0);
                if edit {
                    ui.menu_button(RichText::new(ep::PLUS).color(Color32::LIGHT_GREEN).small(), |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Room ID:");
                            ui.text_edit_singleline(&mut data.temp_state.temp_map_room_id);
                        });
                        let blank = data.temp_state.temp_map_room_id.is_empty();
                        let taken = map.rooms.contains_key(&data.temp_state.temp_map_room_id);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            if blank {
                                ui.colored_label(Color32::YELLOW, "Room ID must not be blank.");
                            } else if taken {
                                ui.colored_label(Color32::RED, "A room with that ID already exists.");
                            } 
                        });
                        ui.add_enabled_ui(!(blank || taken), |ui| {
                            if ui.button("Create").clicked() {
                                map.rooms.insert(data.temp_state.temp_map_room_id.clone(), Room::new());
                                data.temp_state.temp_map_room_id.clear();
                                ui.close_menu();
                            }
                        });
                    });
                }
            });
            ui.add_space(4.0);
            for (room_id, room) in &map.rooms {
                let s = if room.name.is_empty() {
                    format!("{}", room_id)
                } else {
                    format!("{} ({})", room_id, room.name)
                };
                if ui.button(s).clicked() {
                    (self.callback_inner)(MapTab::Room(room_id.clone()), true);
                }
            }
        }
        if unload {
            data.save_and_unload_map();
        }
    }

    fn room(&mut self, ui: &mut Ui, room_id: &String) {
        let data = &mut *self.data;
        if let Some((_, map)) = &mut data.loaded_map {
            if let Some(room) = map.rooms.get_mut(room_id) {
                ui.horizontal(|ui| {
                    ui.heading(format!("Room {}{}", room_id, if room.name.is_empty() {"".to_owned()} else {format!(" ({})", room.name)}));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        Self::edit_mode(&mut data.temp_state, ui);
                    });
                });
                let edit = data.temp_state.map_editing_mode;
                if edit {
                    ui.add(TextEdit::singleline(&mut room.name).hint_text("Room name..."));                    
                }
                if edit {
                    ui.add(TextEdit::multiline(&mut room.description).hint_text("Room description...")); 
                } else {
                    if !room.description.is_empty() {
                        ui.label(RichText::new(&room.description).weak().italics());
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} Enemies", ep::SWORD)).size(16.0));
                    if link_button_frameless(ui) {
                        (self.callback_inner)(MapTab::RoomEnemies(room_id.clone()), true);
                    }
                });
                ui.indent("enemies", |ui| {
                    if room.enemies.is_empty() {
                        ui.label(RichText::new("None...").weak().italics());
                    } else {
                        for (_, (typ, group)) in &room.enemies {
                            ui.label(format!("{} ({})", typ.name, group.len()));
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} Items", ep::COINS)).size(16.0));
                    if link_button_frameless(ui) {
                        (self.callback_inner)(MapTab::RoomItems(room_id.clone()), true);
                    }
                });
                ui.indent("items", |ui| {
                    ui.label(format!("Loose: {}", room.items.loose_items.len()));
                    ui.label(format!("Containers: {}", room.items.containers.len()));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} Connections", ep::DOOR_OPEN)).size(16.0));
                    if link_button_frameless(ui) {
                        (self.callback_inner)(MapTab::RoomConnections(room_id.clone()), true);
                    }
                });
                ui.indent("connections", |ui| {
                    for (uuid, from) in &room.connections {
                        if let Some(connection) = map.connections.get(uuid) {
                            ui.horizontal(|ui| {
                                ui.label(format!("{} {}", 
                                    if connection.one_way {
                                        ep::ARROW_RIGHT
                                    } else {
                                        ep::ARROWS_LEFT_RIGHT
                                    },
                                    if *from {
                                        &connection.to
                                    } else {
                                        &connection.from
                                    }
                                ));
                                if ui.add(egui::Button::new(RichText::new("\u{e972}").size(10.0)).small().frame(false)).clicked() {
                                    (self.callback_inner)(MapTab::Room(if *from {connection.to.clone()} else {connection.from.clone()}), true);
                                }
                            });
                        }
                    }
                });
            } else {
                (self.callback_inner)(MapTab::Room(room_id.clone()), false);
            }
        }
    }

    fn room_enemies(&mut self, ui: &mut Ui, room_id: &String) {
        let data = &mut *self.data;
        if let Some((_, map)) = &mut data.loaded_map {
            if let Some(room) = map.rooms.get_mut(room_id) {
                ui.horizontal(|ui| {
                    ui.heading(format!("Enemies ({}{})", room_id, if room.name.is_empty() {"".to_owned()} else {format!("/{}", room.name)}));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        Self::edit_mode(&mut data.temp_state, ui);
                    });
                });
                let edit = data.temp_state.map_editing_mode;
                ui.separator();
                if edit {
                    ui.menu_button("Add...", |ui| {
                        ui.set_max_width(180.0);
                        if let Some((path, enemy)) = enemy_viewer_callback(ui, &data.enemy_type_registry, room_id) {
                            let e = Enemy::from_type(&enemy);
                            room.enemies.entry(path).or_insert((enemy, Vec::new())).1.push(e);
                        }
                    });
                    ui.add_space(5.0);
                }
                for (enemy_id, (typ, group)) in &mut room.enemies {
                    let mut remove = None;
                    for (i, enemy) in group.iter_mut().enumerate() {
                        CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id((enemy_id, i)), false)
                            .show_header(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(format!("{} {} ({}/{})", typ.name, i + 1, enemy.combat_stats.health.current_hp, enemy.combat_stats.health.max_hp));
                                    ui.menu_button("...", |ui| {
                                        if ui.button("View Enemy Type").clicked() {
                                            data.temp_state.viewed_enemy = Some(enemy_id.clone());
                                            (self.callback_outer)(DMTab::EnemyViewer, true);
                                            ui.close_menu();
                                        }
                                        if edit {
                                            if ui.button("Remove").clicked() {
                                                remove = Some(i);
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                });
                            })
                            .body(|ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    ui.label(format!("HP: {}/{}", enemy.combat_stats.health.current_hp, enemy.combat_stats.health.max_hp));
                                    ui.add_space(4.0);
                                    if ui.add(egui::Button::new("+").small().frame(false)).on_hover_text("Increase current HP").clicked() {
                                        if enemy.combat_stats.health.current_hp < enemy.combat_stats.health.max_hp as i32 {
                                            enemy.combat_stats.health.current_hp += 1;
                                        }
                                    }
                                    if ui.add(egui::Button::new("-").small().frame(false)).on_hover_text("Decrease current HP").clicked() {
                                        if enemy.combat_stats.health.current_hp > i32::MIN {
                                            enemy.combat_stats.health.current_hp -= 1;
                                        }
                                    }
                                    ui.add_space(6.0);
                                    if edit {
                                        if ui.add(egui::Button::new("+").small().frame(false)).on_hover_text("Increase max HP").clicked() {
                                            if enemy.combat_stats.health.max_hp < u32::MAX {
                                                enemy.combat_stats.health.max_hp += 1;
                                            }
                                        }
                                        if ui.add(egui::Button::new("-").small().frame(false)).on_hover_text("Decrease max HP").clicked() {
                                            if enemy.combat_stats.health.max_hp > 1 {
                                                enemy.combat_stats.health.max_hp -= 1;
                                                if enemy.combat_stats.health.current_hp > enemy.combat_stats.health.max_hp as i32 {
                                                    enemy.combat_stats.health.current_hp = enemy.combat_stats.health.max_hp as i32;
                                                }
                                            }
                                        }
                                        ui.add_space(6.0);
                                    }
                                    if ui.add(egui::Button::new(ep::HEART).small().frame(false)).on_hover_text("Restore HP to max").clicked() {
                                        enemy.combat_stats.health.current_hp = enemy.combat_stats.health.max_hp as i32;
                                    }
                                    if edit {
                                        if ui.add(egui::Button::new(ep::DICE_SIX).small().frame(false)).on_hover_text("Reroll max HP").clicked() {
                                            enemy.combat_stats.health.max_hp = typ.hit_dice.roll();
                                            enemy.combat_stats.health.current_hp = enemy.combat_stats.health.max_hp as i32;
                                        }
                                    }
                                });
                                ui.label(format!("AC: {}", enemy.combat_stats.armor_class + enemy.combat_stats.modifiers.armor_class.total()));
                                ui.horizontal(|ui| {
                                    ui.label("Status Effects:");
                                    if edit {
                                        plus_menu_button(ui, |ui| {
                                            for effect in StatusEffect::iterate() {
                                                ui.add_enabled_ui(!enemy.combat_stats.status_effects.is(effect), |ui| {
                                                    if ui.button(format!("{}", effect)).clicked() {
                                                        enemy.combat_stats.status_effects.effects.insert(effect);
                                                    }
                                                });
                                            }
                                        });
                                    }
                                });
                                ui.indent("status_effects", |ui| {
                                    let mut remove_effect = None;
                                    for effect in &enemy.combat_stats.status_effects.effects {
                                        ui.horizontal(|ui| {
                                            ui.label(format!("- {}", effect));
                                            if edit {
                                                if trash_button_frameless(ui) {
                                                    remove_effect = Some(*effect);
                                                }
                                            }
                                        });
                                    }
                                    if enemy.combat_stats.status_effects.effects.is_empty() {
                                        ui.label("None");
                                    }
                                    if let Some(effect) = remove_effect {
                                        enemy.combat_stats.status_effects.effects.remove(&effect);
                                    }
                                });
                                ui.label("Saves:");
                                ui.indent("saves", |ui| {
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.label("P&P");
                                            ui.label(format!("{:+}", enemy.combat_stats.saving_throws.petrification_paralysis + enemy.combat_stats.modifiers.save_petrification_paralysis.total()));
                                        });
                                        ui.vertical(|ui| {
                                            ui.label("P&D");
                                            ui.label(format!("{:+}", enemy.combat_stats.saving_throws.poison_death + enemy.combat_stats.modifiers.save_poison_death.total()));
                                        });
                                        ui.vertical(|ui| {
                                            ui.label("B&B");
                                            ui.label(format!("{:+}", enemy.combat_stats.saving_throws.blast_breath + enemy.combat_stats.modifiers.save_blast_breath.total()));
                                        });
                                        ui.vertical(|ui| {
                                            ui.label("S&W");
                                            ui.label(format!("{:+}", enemy.combat_stats.saving_throws.staffs_wands + enemy.combat_stats.modifiers.save_staffs_wands.total()));
                                        });
                                        ui.vertical(|ui| {
                                            ui.label("Spells");
                                            ui.label(format!("{:+}", enemy.combat_stats.saving_throws.spells + enemy.combat_stats.modifiers.save_spells.total()));
                                        });
                                    });
                                });
                            });
                    }
                    if let Some(index) = remove {
                        group.remove(index);
                    }
                }
            } else {
                (self.callback_inner)(MapTab::RoomEnemies(room_id.clone()), false);
            }
        }
    }

    fn room_items(&mut self, ui: &mut Ui, room_id: &String) {
        let data = &mut *self.data;
        let mut packets = Vec::new();
        if let Some((_, map)) = &mut data.loaded_map {
            if let Some(room) = map.rooms.get_mut(room_id) {
                ui.horizontal(|ui| {
                    ui.heading(format!("Items ({}{})", room_id, if room.name.is_empty() {"".to_owned()} else {format!("/{}", room.name)}));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        Self::edit_mode(&mut data.temp_state, ui);
                    });
                });
                let edit = data.temp_state.map_editing_mode;
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Loose Items").size(18.0))
                        .on_hover_text("These are all the items that aren't in a container. This could be lying on the floor or otherwise not contained.");
                    if edit {
                        plus_menu_button(ui, |ui| {
                            if let Some((_, item)) = item_viewer_callback(ui, &data.item_type_registry, room_id) {
                                room.items.loose_items.push(item);
                            }
                        });
                    }
                });
                let mut remove = None;
                for (i, item) in room.items.loose_items.iter_mut().enumerate() {
                    CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id((i, "loose_items")), false)
                        .show_header(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(format!("{} x{}", item.item_type.name, item.count));
                                ui.menu_button("...", |ui| {
                                    if edit {
                                        ui.add(egui::DragValue::new(&mut item.count).prefix("Count: ").clamp_range(1..=u32::MAX));
                                    }
                                    ui.menu_button("Give to...", |ui| {
                                        for (username, user_data) in &mut data.user_data {
                                            ui.menu_button(username, |ui| {
                                                for (name, sheet) in &mut user_data.characters {
                                                    if ui.button(name).clicked() {
                                                        sheet.inventory.add(item.clone());
                                                        packets.push((ClientBoundPacket::UpdateCharacter(name.clone(), sheet.clone()), username.clone()));
                                                        remove = Some(i);
                                                        ui.close_menu();
                                                    }
                                                }
                                            });
                                        }
                                    });
                                    ui.menu_button("Move to container...", |ui| {
                                        for container in &mut room.items.containers {
                                            ui.menu_button(if container.name.is_empty() {"(Unnamed Container)"} else {container.name.as_str()}, |ui| {
                                                for (section, items) in &mut container.sections {
                                                    if ui.button(format!("Section: {}", section)).clicked() {
                                                        items.push(item.clone());
                                                        remove = Some(i);
                                                        ui.close_menu();
                                                    }
                                                }
                                            });
                                        }
                                    });
                                    if edit {
                                        if ui.button("Remove").clicked() {
                                            remove = Some(i);
                                            ui.close_menu();
                                        }
                                    }
                                });
                            });
                        })
                        .body(|ui| {
                            if edit {
                                ui.add(TextEdit::singleline(&mut item.item_type.name).hint_text("Item name..."));
                            }
                            if edit {
                                ui.add(TextEdit::multiline(&mut item.item_type.description).hint_text("Item description..."));
                            } else if !item.item_type.description.is_empty() {
                                ui.label(RichText::new(&item.item_type.description).weak().italics());
                            }
                            if edit {
                                ui.add(egui::DragValue::new(&mut item.count).prefix("Count: ").clamp_range(1..=u32::MAX));
                            }
                            if edit {
                                egui::ComboBox::from_label("Encumbrance")
                                    .selected_text(item.item_type.encumbrance.display())
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut item.item_type.encumbrance, Encumbrance::Negligible, Encumbrance::Negligible.display());
                                        ui.selectable_value(&mut item.item_type.encumbrance, Encumbrance::Treasure, Encumbrance::Treasure.display());
                                        ui.selectable_value(&mut item.item_type.encumbrance, Encumbrance::OneSixth, Encumbrance::OneSixth.display());
                                        ui.selectable_value(&mut item.item_type.encumbrance, Encumbrance::OneHalf, Encumbrance::OneHalf.display());
                                        ui.selectable_value(&mut item.item_type.encumbrance, Encumbrance::OneStone, Encumbrance::OneStone.display());
                                        ui.selectable_value(&mut item.item_type.encumbrance, Encumbrance::VeryHeavy(1), "Very Heavy");
                                    });
                                if let Encumbrance::VeryHeavy(stone) = &mut item.item_type.encumbrance {
                                    ui.add(egui::DragValue::new(stone).clamp_range(1..=u32::MAX).prefix("Weight (stone): "));
                                }
                            } else {
                                ui.label(format!("Encumbrance: {}", item.item_type.encumbrance.display()));
                            }
                            ui.label(format!("Value: {:.1} sp", item.item_type.value.0))
                                .on_hover_text(
                                    RichText::new(format!("{:.1} cp\n{:.1} sp\n{:.1} ep\n{:.1} gp\n{:.1} pp", 
                                        item.item_type.value.as_copper(),
                                        item.item_type.value.as_silver(),
                                        item.item_type.value.as_electrum(),
                                        item.item_type.value.as_gold(),
                                        item.item_type.value.as_platinum(),
                                )).weak().italics());
                        });
                }
                if let Some(index) = remove {
                    room.items.loose_items.remove(index);
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Containers").size(18.0))
                        .on_hover_text("Items that aren't loose must be in a container. This could be a chest, dresser, corpse, or even something like the surface of a table.");
                    if edit {
                        if plus_button(ui) {
                            room.items.containers.push(RoomContainer::new());
                        }
                    }
                });
                for (i, container) in room.items.containers.iter_mut().enumerate() {
                    ui.label(RichText::new(if container.name.is_empty() {"(Unnamed Container)"} else {container.name.as_str()}).size(16.0));
                    ui.indent(i, |ui| {
                        if edit {
                            ui.add(TextEdit::singleline(&mut container.name).hint_text("Container name..."));
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                            ui.label("Locked?")
                                .on_hover_text("A container is locked if its contents cannot be accessed trivially.");
                            if container.locked {
                                ui.colored_label(Color32::LIGHT_RED, " Yes  ");
                                if ui.add(egui::Button::new(ep::LOCK_OPEN).frame(false)).on_hover_text("Unlock").clicked() {
                                    container.locked = false;
                                }
                            } else {
                                ui.colored_label(Color32::LIGHT_GREEN, " No  ");
                                if ui.add(egui::Button::new(ep::LOCK).frame(false)).on_hover_text("Lock").clicked() {
                                    container.locked = true;
                                }
                            }
                        });
                        let mut remove_trap = false;
                        if let Some(trapped) = &mut container.trapped {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                                ui.label("Trapped?");
                                ui.colored_label(Color32::LIGHT_RED, " Yes  ");
                                if edit {
                                    if ui.button(RichText::new(ep::X).color(Color32::LIGHT_RED).small()).clicked() {
                                        remove_trap = true;
                                    }
                                }
                            });
                            ui.indent((i, "trapped"), |ui| {
                                if edit {
                                    ui.add(TextEdit::multiline(&mut trapped.description).hint_text("Trap description..."));
                                } else if !trapped.description.is_empty() {
                                    ui.label(RichText::new(&trapped.description).weak().italics());
                                }
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                                    ui.label("Active?")
                                        .on_hover_text("A trap is active if it will trigger the next time its conditions are met.");
                                    if trapped.active {
                                        ui.colored_label(Color32::LIGHT_RED, " Yes  ");
                                    } else {
                                        ui.colored_label(Color32::LIGHT_GREEN, " No  ");
                                    }
                                    if ui.add(egui::Button::new(RichText::new(ep::ARROWS_CLOCKWISE).small()).frame(false)).clicked() {
                                        trapped.active = !trapped.active;
                                    }
                                });
                            });
                        } else {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                                ui.label("Trapped?");
                                ui.colored_label(Color32::LIGHT_GREEN, " No  ");
                                if edit {
                                    if plus_button(ui) {
                                        container.trapped = Some(RoomTrap::new());
                                    }
                                }
                            });
                        }
                        if remove_trap {
                            container.trapped = None;
                        }
                        ui.add_space(3.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Sections").size(15.0))
                                .on_hover_text("Sections are a way to divide a container into multiple parts. For example, a chest could have a \"Main\" section as well as a \"Hidden Compartment\" section.");
                            if edit {
                                plus_menu_button(ui, |ui| {
                                    ui.add(TextEdit::singleline(&mut data.temp_state.temp_container_section).hint_text("Section name..."));
                                    if ui.button("Add section").clicked() {
                                        container.sections.insert(data.temp_state.temp_container_section.clone(), Vec::new());
                                        data.temp_state.temp_container_section.clear();
                                        ui.close_menu();
                                    }
                                });
                            }
                        });
                        for (section, items) in &mut container.sections {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!(" Section: {}", section)).size(14.0));
                                if edit {
                                    plus_menu_button(ui, |ui| {
                                        if let Some((_, item)) = item_viewer_callback(ui, &data.item_type_registry, (room_id, i, section)) {
                                            items.push(item);
                                        }
                                    });
                                }
                            });
                            ui.indent((i, section), |ui| {
                                for item in items {
                                    ui.label(format!("- {} x{}", item.item_type.name, item.count));
                                }
                            });
                        }
                    });
                }
            } else {
                (self.callback_inner)(MapTab::RoomItems(room_id.clone()), false);
            }
            for (packet, user) in packets {
                data.send_to_user(packet, user);
            }
        }
    }

    fn room_connections(&mut self, ui: &mut Ui, room_id: &String) {
        let data = &mut *self.data;
        if let Some((_, map)) = &mut data.loaded_map {
            let room_list: Vec<(String, String)> = map.rooms.iter().filter_map(|(id, room)| {
                if id == room_id {
                    None
                } else {
                    Some((id.clone(), room.name.clone()))
                }
            }).collect();
            let mut new_connection = None;
            if let Some(room) = map.rooms.get_mut(room_id) {
                ui.horizontal(|ui| {
                    ui.heading(format!("Connections ({}{})", room_id, if room.name.is_empty() {"".to_owned()} else {format!("/{}", room.name)}));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        Self::edit_mode(&mut data.temp_state, ui);
                    });
                });
                let edit = data.temp_state.map_editing_mode;
                ui.separator();
                if edit {
                    ui.menu_button("Add...", |ui| {
                        ui.checkbox(&mut data.temp_state.temp_room_connect_one_way, "One-Way")
                            .on_hover_text("A one-way connection cannot be traversed backwards, like a teleporter trap or a fall downwards.");
                        ui.menu_button("Leads to...", |ui| {
                            for (id, name) in room_list {
                                if ui.button(format!("{} ({})", id, name)).clicked() {
                                    new_connection = Some((id, data.temp_state.temp_room_connect_one_way));
                                    data.temp_state.temp_room_connect_one_way = false;
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                    ui.add_space(5.0);
                }
                for (uuid, &from) in &room.connections {
                    if let Some(connection) = map.connections.get_mut(uuid) {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(
                                format!("{} {}", 
                                if connection.one_way {ep::ARROW_RIGHT} else {ep::ARROWS_LEFT_RIGHT},
                                if from {&connection.to} else {&connection.from}
                            )).size(18.0));
                            if link_button_frameless(ui) {
                                (self.callback_inner)(MapTab::Room(if from {connection.to.clone()} else {connection.from.clone()}), true);
                            }
                        });
                        ui.indent(uuid, |ui| {
                            if edit {
                                ui.add(TextEdit::multiline(&mut connection.description).hint_text("Description..."));
                            } else if !connection.description.is_empty() {
                                ui.label(RichText::new(&connection.description).weak().italics());
                            }
                            if connection.one_way {
                                ui.label("One-Way")
                                    .on_hover_text(format!("This connection cannot be traversed backwards (going from room {} to {}).", connection.to, connection.from));
                            } else {
                                ui.label("Two-Way")
                                    .on_hover_text("This connection can be traversed from both directions.");
                            }
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                                ui.label("Passable?")
                                    .on_hover_text("A connection is passable if someone can walk through it without needing to interact with it (e.g. an open door). Connections cannot be both passable and locked.");
                                if connection.passable {
                                    ui.colored_label(Color32::LIGHT_GREEN, " Yes  ");
                                    if ui.add(egui::Button::new(ep::DOOR).frame(false)).on_hover_text("Close").clicked() {
                                        connection.passable = false;
                                    }
                                } else {
                                    ui.colored_label(Color32::LIGHT_RED, " No  ");
                                    ui.add_enabled_ui(!connection.locked, |ui| {
                                        if ui.add(egui::Button::new(ep::DOOR_OPEN).frame(false)).on_hover_text("Open").on_disabled_hover_text("Connection is locked").clicked() {
                                            connection.passable = true;
                                        }
                                    });
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                                ui.label("Locked?")
                                    .on_hover_text("A connection is locked if it cannot trivially be made passable (i.e. it requires a key, dice roll, etc). Connections cannot be both locked and passable.");
                                if connection.locked {
                                    ui.colored_label(Color32::LIGHT_RED, " Yes  ");
                                    if ui.add(egui::Button::new(ep::LOCK_OPEN).frame(false)).on_hover_text("Unlock").clicked() {
                                        connection.locked = false;
                                    }
                                } else {
                                    ui.colored_label(Color32::LIGHT_GREEN, " No  ");
                                    ui.add_enabled_ui(!connection.passable, |ui| {
                                        if ui.add(egui::Button::new(ep::LOCK).frame(false)).on_hover_text("Lock").on_disabled_hover_text("Connection is passable").clicked() {
                                            connection.locked = true;
                                        }
                                    });
                                }
                            });
                            let mut remove_trap = false;
                            if let Some(trapped) = &mut connection.trapped {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                                    ui.label("Trapped?");
                                    ui.colored_label(Color32::LIGHT_RED, " Yes  ");
                                    if edit {
                                        if ui.button(RichText::new(ep::X).color(Color32::LIGHT_RED).small()).clicked() {
                                            remove_trap = true;
                                        }
                                    }
                                });
                                ui.indent((uuid, "trapped"), |ui| {
                                    if edit {
                                        ui.add(TextEdit::multiline(&mut trapped.description).hint_text("Trap description..."));
                                    } else if !trapped.description.is_empty() {
                                        ui.label(RichText::new(&trapped.description).weak().italics());
                                    }
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                                        ui.label("Active?")
                                            .on_hover_text("A trap is active if it will trigger the next time its conditions are met.");
                                        if trapped.active {
                                            ui.colored_label(Color32::LIGHT_RED, " Yes  ");
                                        } else {
                                            ui.colored_label(Color32::LIGHT_GREEN, " No  ");
                                        }
                                        if ui.add(egui::Button::new(RichText::new(ep::ARROWS_CLOCKWISE).small()).frame(false)).clicked() {
                                            trapped.active = !trapped.active;
                                        }
                                    });
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                                    ui.label("Trapped?");
                                    ui.colored_label(Color32::LIGHT_GREEN, " No  ");
                                    if edit {
                                        if plus_button(ui) {
                                            connection.trapped = Some(RoomTrap::new());
                                        }
                                    }
                                });
                            }
                            if remove_trap {
                                connection.trapped = None;
                            }
                        });
                    }
                }
            } else {
                (self.callback_inner)(MapTab::RoomConnections(room_id.clone()), false);
            }
            if let Some((to, one_way)) = new_connection {
                let uuid = uuid::Uuid::new_v4().to_string();
                map.connections.insert(uuid.clone(), {
                    let mut c = RoomConnection::new();
                    c.from = room_id.clone();
                    c.to = to.clone();
                    c.one_way = one_way;
                    c
                });
                if let Some(room) = map.rooms.get_mut(room_id) {
                    room.connections.insert(uuid.clone(), true);
                }
                if !one_way {
                    if let Some(room) = map.rooms.get_mut(&to) {
                        room.connections.insert(uuid, false);
                    }
                }
            }
        }
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
                ui.add_space(3.0);
                Self::top_bar(ctx, ui, data, &mut self.tree);
                ui.add_space(2.0);
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.tree.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() / 2.0) - 10.0);
                    ui.label(RichText::new("There's nothing here...").weak().italics());
                });
            } else {
                let mut new_tab = None;
                let mut remove_tab = None;
                DockArea::new(&mut self.tree)
                    .show_inside(ui, &mut DMTabViewer {
                        callback: &mut |tab, add| {
                            if add {
                                new_tab = Some(tab);
                            } else {
                                remove_tab = Some(tab);
                            }
                        },
                        data,
                        map_tree: &mut self.map_tree,
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