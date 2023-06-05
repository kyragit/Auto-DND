use crate::character::{PlayerCharacter, Attr};
use crate::combat::CombatantType;
use crate::common_ui::{CommonApp, CharacterSheetTab, self, display_i32, display_percent};
use eframe::egui::{self, RichText};
use eframe::epaint::Rgba;
use crate::packets::{ClientBoundPacket, ServerBoundPacket, CombatAction, ClientFacingError};
use std::collections::HashMap;
use std::net::TcpStream;
use std::io::{prelude::*, ErrorKind};
use std::sync::{Arc, Mutex};

/// How often to check for incoming packets, in milliseconds. Setting this too low may cause 
/// performance problems due to acquiring a lock on a mutex.
pub const CLIENT_UPDATE_CLOCK: u64 = 50;

pub fn run() -> Result<(), eframe::Error> {
    eframe::run_native(
        "Player Tool", 
        eframe::NativeOptions {
            centered: true,
            initial_window_size: Some(egui::vec2(1280.0, 720.0)),
            ..Default::default()
        }, 
        Box::new(|_ctx| {
            Box::new(PlayerApp::new())
        })
    )
}
pub struct PlayerAppData {
    pub window_states: HashMap<String, bool>,
    pub ip_address: String,
    pub show_error: bool,
    pub stream: Option<TcpStream>,
    pub logged_in: bool,
    pub logs: Vec<String>,
    pub chat_box: String,
    pub username: String,
    pub password: String,
    pub characters: HashMap<String, PlayerCharacter>,
    pub character_window_tab_state: HashMap<String, CharacterSheetTab>,
    pub new_char_name: String,
    pub notes: String,
    pub new_characters: Vec<PlayerCharacter>,
    pub picked_character: Option<usize>,
    pub new_char_name_error: Option<ClientFacingError>,
    pub character_awaiting_action: Option<CombatantType>,
    pub combatant_list: Vec<CombatantType>,
    pub selected_target: usize,
}

impl PlayerAppData {
    pub fn new() -> Self {
        Self {
            window_states: HashMap::new(),
            ip_address: String::new(),
            show_error: false,
            stream: None,
            logged_in: false,
            logs: Vec::new(),
            chat_box: String::new(),
            username: String::new(),
            password: String::new(),
            characters: HashMap::new(),
            character_window_tab_state: HashMap::new(),
            new_char_name: String::new(),
            notes: String::new(),
            new_characters: Vec::new(),
            picked_character: None,
            new_char_name_error: None,
            character_awaiting_action: None,
            combatant_list: Vec::new(),
            selected_target: 0,
        }
    }

    pub fn write_to_stream(&mut self, msg: String) {
        let func = |stream: &mut TcpStream, msg: String| -> std::io::Result<()> {
            stream.write_all(msg.as_bytes())?;
            stream.write_all(&[255])?;
            stream.flush()?;
            Ok(())
        };
        if let Some(stream) = self.stream.as_mut() {
            match func(stream, msg) {
                Ok(_) => {},
                Err(_) => {
                    self.stream = None;
                },
            }
        }
    }

    pub fn send_to_server(&mut self, packet: ServerBoundPacket) {
        if let Ok(msg) = ron::to_string(&packet) {
            self.write_to_stream(msg);
        }
    }


}

pub struct PlayerApp {
    data: Arc<Mutex<PlayerAppData>>,
}

impl PlayerApp {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(PlayerAppData::new())),
        }
    }

    pub fn chat_window(ctx: &egui::Context, data: &mut PlayerAppData) {
        egui::Window::new("Chat").collapsible(true).vscroll(true).resizable(true).show(ctx, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                let response = ui.text_edit_singleline(&mut data.chat_box);
                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    if !data.chat_box.trim().is_empty() {
                        data.send_to_server(ServerBoundPacket::ChatMessage(data.chat_box.clone()));  
                    }
                    data.chat_box.clear();
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

    pub fn connect_screen(ctx: &egui::Context, data: &mut PlayerAppData) -> bool {
        let response = egui::CentralPanel::default().show(ctx, |ui| {
            ui.text_edit_singleline(&mut data.ip_address);
            if ui.button("Connect").clicked() {
                if let Ok(stream) = TcpStream::connect(data.ip_address.trim()) {
                    stream.set_nonblocking(true).unwrap();
                    data.stream = Some(stream);
                    return true;
                } else {
                    data.show_error = true;
                }
            }
            if data.show_error {
                ui.colored_label(Rgba::RED, "Could not connect to server.");
            }
            false
        });
        response.inner
    }

    pub fn log_in_screen(ctx: &egui::Context, data: &mut PlayerAppData) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.label("Username:");
                ui.text_edit_singleline(&mut data.username);
                ui.label("Password:");
                ui.text_edit_singleline(&mut data.password);
                ui.colored_label(Rgba::RED, "Warning! Username and password are NOT stored securely!");
                if ui.button("Log in").clicked() {
                    data.send_to_server(ServerBoundPacket::AttemptLogIn(data.username.clone(), data.password.clone()));
                }
                if ui.button("Create account").clicked() {
                    data.send_to_server(ServerBoundPacket::CreateAccount(data.username.clone(), data.password.clone()));
                }
            });
        });
    }

    pub fn notes_window(ctx: &egui::Context, data: &mut PlayerAppData) {
        data.create_window(ctx, "Player Notes", "notes_open".to_owned(), |window| {
            window.collapsible(true)
            .vscroll(true)
            .resizable(true)
        }, |ui, data| {
            if ui.text_edit_multiline(&mut data.notes).lost_focus() {
                data.send_to_server(ServerBoundPacket::UpdatePlayerNotes(data.notes.clone()));
            }
        });
    }

    pub fn character_sheet_windows(ctx: &egui::Context, data: &mut PlayerAppData) {
        // I do not see a way around cloning here, as much as I hate it
        let mut characters = data.characters.clone();
        let mut packets = Vec::new();
        for (name, sheet) in characters.iter_mut() {
            data.create_window(
                ctx, 
                format!("Character: {}", name), 
                format!("character_sheet_<{}>", name), 
                |window| {
                    window.collapsible(true).resizable(false)
                }, 
                |ui, data| {
                    let tab_state = data.character_window_tab_state.entry(name.clone()).or_insert(CharacterSheetTab::default());
                    common_ui::tabs(tab_state, format!("char_sheet_tabs_<{}>", name), ui, |ui, tab| {
                        match tab {
                            CharacterSheetTab::Stats => {
                                let attrs = sheet.combat_stats.attributes;
                                ui.label(format!("STR: {} ({})", attrs.strength, display_i32(attrs.modifier(Attr::STR))));
                                ui.label(format!("DEX: {} ({})", attrs.dexterity, display_i32(attrs.modifier(Attr::DEX))));
                                ui.label(format!("CON: {} ({})", attrs.constitution, display_i32(attrs.modifier(Attr::CON))));
                                ui.label(format!("INT: {} ({})", attrs.intelligence, display_i32(attrs.modifier(Attr::INT))));
                                ui.label(format!("WIS: {} ({})", attrs.wisdom, display_i32(attrs.modifier(Attr::WIS))));
                                ui.label(format!("CHA: {} ({})", attrs.charisma, display_i32(attrs.modifier(Attr::CHA))));
                                ui.label(format!("HP: {}/{}", sheet.combat_stats.health.current_hp, sheet.combat_stats.health.max_hp));
                                ui.label(format!("AC: {}", sheet.combat_stats.armor_class + sheet.combat_stats.modifiers.armor_class.total()));
                                ui.label(format!("Initiative: {}", display_i32(sheet.combat_stats.modifiers.initiative.total())));
                                ui.label(format!("Surprise: {}", display_i32(sheet.combat_stats.modifiers.surprise.total())));
                                ui.label(format!("ATK: +{}", sheet.combat_stats.attack_throw));
                                ui.label(format!("Melee ATK bonus: {}", display_i32(sheet.combat_stats.modifiers.melee_attack.total())));
                                ui.label(format!("Missile ATK bonus: {}", display_i32(sheet.combat_stats.modifiers.missile_attack.total())));
                                ui.label(format!("Melee DMG bonus: {}", display_i32(sheet.combat_stats.modifiers.melee_damage.total())));
                                ui.label(format!("Missile DMG bonus: {}", display_i32(sheet.combat_stats.modifiers.missile_damage.total())));
                                ui.separator();
                                let saves = sheet.combat_stats.saving_throws;
                                ui.label("Saving throws:");
                                ui.label(format!("Petrification & Paralysis: {}", display_i32(saves.petrification_paralysis + sheet.combat_stats.modifiers.save_petrification_paralysis.total())));
                                ui.label(format!("Poison & Death: {}", display_i32(saves.poison_death + sheet.combat_stats.modifiers.save_poison_death.total())));
                                ui.label(format!("Blast & Breath: {}", display_i32(saves.blast_breath + sheet.combat_stats.modifiers.save_blast_breath.total())));
                                ui.label(format!("Staffs & Wands: {}", display_i32(saves.staffs_wands + sheet.combat_stats.modifiers.save_staffs_wands.total())));
                                ui.label(format!("Spells: {}", display_i32(saves.spells + sheet.combat_stats.modifiers.save_spells.total())));
                            },
                            CharacterSheetTab::Class => {
                                ui.label(format!("Class: {}", sheet.class.name));
                                ui.label(RichText::new(&sheet.class.description).italics().weak());
                                ui.label(format!("Race: {}", sheet.race));
                                ui.label(format!("Level: {}", sheet.level));
                                ui.label(format!("XP: {}/{} ({})", sheet.xp, sheet.xp_to_level, display_percent(sheet.combat_stats.modifiers.xp_gain.total())));
                            },
                            CharacterSheetTab::Inventory => {
                                sheet.inventory.foreach(|item| {
                                    ui.label(format!("{} x{}", item.item_type.name, item.count)).on_hover_text(&item.item_type.description);
                                });
                                ui.separator();
                                ui.label(format!("Encumbrance: {}", sheet.inventory.total_weight()));
                            },
                            CharacterSheetTab::Spells => {

                            },
                            CharacterSheetTab::Notes => {
                                egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                                    if ui.text_edit_multiline(&mut sheet.notes).lost_focus() {
                                        packets.push(ServerBoundPacket::RequestCharacterUpdate(name.clone(), Some(sheet.clone())));
                                    }
                                });
                            },
                        }
                    });
                },
            );
        }
        data.characters = characters;
        for packet in packets {
            data.send_to_server(packet);
        }
    }

    pub fn character_generator(ctx: &egui::Context, data: &mut PlayerAppData) {
        let mut maybe_packet = None;
        egui::Window::new("Character Generator")
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .auto_sized()
            .open(data.window_states.entry("character_generator".to_owned()).or_insert(false))
            .show(ctx, |ui| {
                if data.new_characters.is_empty() {
                    if ui.button("Generate!").clicked() {
                        for _ in 0..5 {
                            data.new_characters.push(PlayerCharacter::random());
                        }
                    }
                } else if let Some(i) = data.picked_character {
                    ui.vertical_centered(|ui| {
                        if (ui.add(egui::TextEdit::singleline(&mut data.new_char_name).hint_text("Give your new character a name...")).lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))) || ui.button("Ok").clicked() {
                            if let Some(sheet) = data.new_characters.get(i) {
                                maybe_packet = Some(ServerBoundPacket::CreateNewCharacter(data.new_char_name.clone(), sheet.clone()));
                            }
                        }
                        if let Some(e) = data.new_char_name_error {
                            ui.colored_label(ctx.style().visuals.error_fg_color, format!("{}", e));
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        if data.new_characters.len() <= 3 {
                            data.new_characters.clear();
                            return;
                        }
                        let mut picked = None;
                        for (i, sheet) in data.new_characters.iter().enumerate() {
                            ui.vertical(|ui| {
                                ui.label(format!("Race: {}", sheet.race));
                                ui.label(format!("STR: {}", sheet.combat_stats.attributes.strength));
                                ui.label(format!("DEX: {}", sheet.combat_stats.attributes.dexterity));
                                ui.label(format!("CON: {}", sheet.combat_stats.attributes.constitution));
                                ui.label(format!("INT: {}", sheet.combat_stats.attributes.intelligence));
                                ui.label(format!("WIS: {}", sheet.combat_stats.attributes.wisdom));
                                ui.label(format!("CHA: {}", sheet.combat_stats.attributes.charisma));
                                if ui.button("Pick").clicked() {
                                    picked = Some(i);
                                }
                            });
                            if i < data.new_characters.len() - 1 {
                                ui.separator();
                            }
                        }
                        if picked.is_some() {
                            data.picked_character = picked;
                        }
                    });
                }
            });
        if let Some(packet) = maybe_packet {
            data.send_to_server(packet);
        }
    }

    pub fn combat_action_window(ctx: &egui::Context, data: &mut PlayerAppData) {
        if *data.window_states.entry("combat_action".to_owned()).or_insert(false) {
            egui::Window::new("Combat Action")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(format!("Character: {}", data.character_awaiting_action.as_ref().map_or("error".to_owned(), |n| n.name())));
                    egui::ComboBox::from_label("Target")
                    .show_index(ui, &mut data.selected_target, data.combatant_list.len(), |i| data.combatant_list[i].name());
                if ui.button("Attack").clicked() {
                    if let Some(target) = data.combatant_list.get(data.selected_target) {
                        data.send_to_server(ServerBoundPacket::DecideCombatAction(CombatAction::Attack(target.clone())));
                    } else {
                        data.send_to_server(ServerBoundPacket::DecideCombatAction(CombatAction::RelinquishControl));
                    }
                    data.window_states.insert("combat_action".to_owned(), false);
                }
                });
            });
        }
    }
}

impl eframe::App for PlayerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();
        let data = &mut *self.data.lock().unwrap();
        if data.stream.is_none() {
            if Self::connect_screen(ctx, data) {
                let data_clone = Arc::clone(&self.data);
                std::thread::spawn(move || {
                    handle_packets(data_clone);
                });
            }
            return;
        }
        if !data.logged_in {
            Self::log_in_screen(ctx, data);
            return;
        }
        Self::chat_window(ctx, data);
        Self::notes_window(ctx, data);
        Self::character_generator(ctx, data);
        Self::character_sheet_windows(ctx, data);
        Self::combat_action_window(ctx, data);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                if ui.button("Open player notes").clicked() {
                    data.toggle_window_state("notes_open");
                }
                if ui.button("Character Generator").clicked() {
                    data.toggle_window_state("character_generator");
                }
                for (name, _) in &data.characters {
                    if ui.button(format!("View: {}", name)).clicked() {
                        let open = data.window_states.entry(format!("character_sheet_<{}>", name)).or_insert(false);
                        *open = !*open;
                    }
                }
            });
        });
    }
}

pub fn handle_packets(data: Arc<Mutex<PlayerAppData>>) {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(CLIENT_UPDATE_CLOCK));
        let data = &mut *data.lock().unwrap();
        if let Some(stream) = &mut data.stream {
            let mut reader = std::io::BufReader::new(stream);
            let recieved: Vec<u8>;
            match reader.fill_buf() {
                Ok(buf) => {
                    if buf.is_empty() {
                        continue;
                    }
                    recieved = buf.to_vec();
                },
                Err(e) => {
                    match e.kind() {
                        ErrorKind::ConnectionAborted | ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset => {
                            data.stream = None;
                            continue;
                        },
                        _ => {continue;},
                    }
                },
            }
            reader.consume(recieved.len());
            for split in recieved.split(|byte| *byte == 255) {
                let msg = String::from_utf8(split.to_vec()).unwrap_or(String::new());
                if let Ok(packet) = ron::from_str::<ClientBoundPacket>(msg.as_str()) {
                    packet.handle(data);
                }
            }
        } else {
            break;
        }
    }
}
