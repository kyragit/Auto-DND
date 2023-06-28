use crate::{AppPreferences, WindowPreferences};
use crate::character::{PlayerCharacter, Attr, PlayerEquipSlot};
use crate::class::{Class, ClassDamageBonus, Cleaves, DivineValue, ArcaneValue};
use crate::combat::{CombatantType, SavingThrowType};
use crate::common_ui::{CharacterSheetTab, self, back_arrow};
use crate::dm_app::{Registry, RegistryNode};
use crate::item::{WeaponDamage, MeleeDamage, ContainerStats};
use crate::proficiency::Proficiency;
use crate::spell::{Spell, SpellRegistry, MagicType};
use displaydoc::Display;
use eframe::egui::{self, RichText, Ui, WidgetText};
use eframe::epaint::{Rgba, Color32};
use egui_dock::{TabViewer, Tree, DockArea};
use egui_extras::{StripBuilder, Size};
use simple_enum_macro::simple_enum;
use crate::packets::{ClientBoundPacket, ServerBoundPacket, CombatAction, ClientFacingError, Request};
use std::collections::HashMap;
use std::net::TcpStream;
use std::io::{prelude::*, ErrorKind};
use std::sync::{Arc, Mutex};
use egui_phosphor as ep;

/// How often to check for incoming packets, in milliseconds. Setting this too low may cause 
/// performance problems due to acquiring a lock on a mutex.
pub const CLIENT_UPDATE_CLOCK: u64 = 50;

pub fn run(prefs: AppPreferences) -> Result<(), eframe::Error> {
    eframe::run_native(
        "Player Tool",
        if let Some(p) = prefs.player_window {
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
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts);
            ctx.egui_ctx.set_fonts(fonts);
            Box::new({
                let app = PlayerApp::new();
                {
                    let data = &mut *app.data.lock().unwrap();
                    if let Some(ip) = prefs.player_last_ip {
                        data.ip_address = ip;
                    }
                    if let Some((username, password)) = prefs.player_login {
                        data.remember_me = true;
                        data.username = username;
                        data.password = password;
                    }
                }
                app
            })
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
    pub unread_messages: u32,
    pub unread_msg_buffer: bool,
    pub username: String,
    pub password: String,
    pub remember_me: bool,
    pub class_registry: Registry<Class>,
    pub spell_registry: SpellRegistry,
    pub proficiency_registry: HashMap<String, Proficiency>,
    pub sorted_prof_list: Vec<(String, String)>,
    pub viewed_class: Option<String>,
    pub viewed_spell: Option<(MagicType, Option<(u8, Option<String>)>)>,
    pub viewed_prof: Option<String>,
    pub viewed_prof_spec: Option<String>,
    pub picking_prof: Option<(bool, String)>,
    pub characters: HashMap<String, PlayerCharacter>,
    pub character_window_tab_state: HashMap<String, CharacterSheetTab>,
    pub new_char_class: Option<Class>,
    pub new_char_name: Option<String>,
    pub notes: String,
    pub new_characters: Vec<PlayerCharacter>,
    pub picked_character: Option<usize>,
    pub new_char_name_error: Option<ClientFacingError>,
    pub character_awaiting_action: Option<CombatantType>,
    pub combatant_list: Vec<CombatantType>,
    pub selected_target: usize,
    pub prefs: WindowPreferences,
    pub requests: Requests,
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
            unread_messages: 0,
            unread_msg_buffer: false,
            username: String::new(),
            password: String::new(),
            remember_me: false,
            class_registry: Registry::new(),
            spell_registry: SpellRegistry::new(),
            proficiency_registry: HashMap::new(),
            sorted_prof_list: Vec::new(),
            viewed_class: None,
            viewed_spell: None,
            viewed_prof: None,
            viewed_prof_spec: None,
            picking_prof: None,
            characters: HashMap::new(),
            character_window_tab_state: HashMap::new(),
            new_char_class: None,
            new_char_name: None,
            notes: String::new(),
            new_characters: Vec::new(),
            picked_character: None,
            new_char_name_error: None,
            character_awaiting_action: None,
            combatant_list: Vec::new(),
            selected_target: 0,
            prefs: WindowPreferences::new(),
            requests: Requests::new(),
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

    pub fn get_chat_title(&self) -> WidgetText {
        if self.unread_messages == 0 || self.unread_msg_buffer {
            format!("{}", ep::CHAT_TEXT).into()
        } else {
            RichText::new(format!("{}({})", ep::CHAT_TEXT, self.unread_messages)).color(Color32::RED).into()
        }
    }
}

pub struct PlayerApp {
    pub data: Arc<Mutex<PlayerAppData>>,
    pub tree: Tree<PlayerTab>,
}

impl PlayerApp {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(PlayerAppData::new())),
            tree: Tree::new(vec![PlayerTab::Chat]),
        }
    }

    pub fn chat_window(ctx: &egui::Context, data: &mut PlayerAppData, tree: &mut Tree<PlayerTab>) {
        let open = data.window_states.entry("chat_window".to_owned()).or_insert(false);
        let prev_open = open.clone();
        let mut temp_open = open.clone();
        egui::Window::new(data.get_chat_title())
            .id("chat_window".into())
            .collapsible(true)
            .vscroll(true)
            .resizable(true)
            .open(&mut temp_open)
            .show(ctx, |ui| {
                data.unread_messages = 0;
                chat(ui, data);
            });
        if prev_open && !temp_open {
            if tree.find_tab(&PlayerTab::Chat).is_none() {
                tree.push_to_focused_leaf(PlayerTab::Chat);
            }
        }
        data.window_states.insert("chat_window".to_owned(), temp_open);
    }

    pub fn connect_screen(ctx: &egui::Context, data: &mut PlayerAppData) -> bool {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() / 2.0) - 65.0);
                ui.label(RichText::new("ACKS Player Tool").strong().size(30.0));
                ui.add_space(10.0);
                let res = ui.add(egui::TextEdit::singleline(&mut data.ip_address).hint_text(RichText::new("Enter IP...").weak().italics()));
                if ui.button("Connect").clicked() || (res.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))) {
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
            }).inner
        }).inner
    }

    pub fn log_in_screen(ctx: &egui::Context, data: &mut PlayerAppData) {
        egui::CentralPanel::default().show(ctx, |ui| {
            StripBuilder::new(ui)
                .size(Size::remainder())
                .size(Size::exact(160.0))
                .size(Size::remainder())
                .cell_layout(egui::Layout::top_down(egui::Align::Center).with_main_align(egui::Align::Center).with_main_justify(false))
                .vertical(|mut strip| {
                    strip.empty();
                    strip.cell(|ui| {
                        ui.label(RichText::new("ACKS Player Tool").strong().size(30.0));
                        ui.label("Username:");
                        let res1 = ui.text_edit_singleline(&mut data.username);
                        ui.label("Password:");
                        let res2 = ui.text_edit_singleline(&mut data.password);
                        ui.colored_label(Rgba::RED, "Warning! Username and password are NOT stored securely!");
                        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter)) && (res1.lost_focus() || res2.lost_focus());
                        StripBuilder::new(ui)
                            .size(Size::remainder())
                            .size(Size::exact(60.0))
                            .size(Size::remainder())
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center).with_main_justify(true))
                            .horizontal(|mut strip| {
                                strip.empty();
                                strip.cell(|ui| {
                                    if ui.button("Log in").clicked() || enter_pressed {
                                        data.send_to_server(ServerBoundPacket::AttemptLogIn(data.username.clone(), data.password.clone()));     
                                    }
                                    ui.checkbox(&mut data.remember_me, "Remember me");
                                });
                                strip.empty();
                            });
                        if ui.button("Create account").clicked() {
                            data.send_to_server(ServerBoundPacket::CreateAccount(data.username.clone(), data.password.clone()));
                        }
                    });
                    strip.empty();
                });
        });
    }

    fn combat_action_window(ctx: &egui::Context, data: &mut PlayerAppData) {
        if *data.window_states.entry("combat_action".to_owned()).or_insert(false) {
            egui::Window::new("Combat Action")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(format!("Character: {}", data.character_awaiting_action.as_ref().map_or("error".to_owned(), |n| n.name())));
                    egui::ComboBox::from_label("Target")
                        .show_index(ui, &mut data.selected_target, data.combatant_list.len(), |i| data.combatant_list.get(i).map_or("error".to_owned(), |c| c.name()));
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

pub fn chat(ui: &mut Ui, data: &mut PlayerAppData) {
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
}

pub struct PlayerTabViewer<'a, F: FnMut(PlayerTab, bool)> {
    pub callback: &'a mut F,
    pub data: &'a mut PlayerAppData,
}

impl<'a, F: FnMut(PlayerTab, bool) + 'a> PlayerTabViewer<'a, F> {
    fn character_sheet(&mut self, ui: &mut Ui, name: &String) {
        let data = &mut *self.data;
        let mut packets = Vec::new();
        if let Some(sheet) = data.characters.get_mut(name) {
            let tab_state = data.character_window_tab_state.entry(name.clone()).or_insert(CharacterSheetTab::default());
            let mut update = false;
            common_ui::tabs(tab_state, format!("char_sheet_tabs_<{}>", name), ui, |old, _| {
                if old == CharacterSheetTab::Notes {
                    update = true;
                }
            }, |ui, tab| {
                match tab {
                    CharacterSheetTab::Stats => {
                        let attrs = sheet.combat_stats.attributes;
                        ui.label(format!("STR: {} ({:+})", attrs.strength, attrs.modifier(Attr::STR)))
                            .on_hover_text("Strength represents brute force and muscle mass. It modifies your melee attack and damage rolls, as well as acts of brute force (such as breaking open a door).");
                        ui.label(format!("DEX: {} ({:+})", attrs.dexterity, attrs.modifier(Attr::DEX)))
                            .on_hover_text("Dexterity represents agility, gracefulness, and hand-eye coordination. It modifies your missile (ranged) attack rolls (NOT damage rolls), armor class, and initiative.");
                        ui.label(format!("CON: {} ({:+})", attrs.constitution, attrs.modifier(Attr::CON)))
                            .on_hover_text("Constitution represents health and general hardiness. It modifies your health roll whenever you level up, and rolls on the Mortal Wounds table.");
                        ui.label(format!("INT: {} ({:+})", attrs.intelligence, attrs.modifier(Attr::INT)))
                            .on_hover_text("Intelligence represents knowledge and academic aptitude. It modifies your spell repertoire size, number of languages spoken, and number of general proficiencies.");
                        ui.label(format!("WIS: {} ({:+})", attrs.wisdom, attrs.modifier(Attr::WIS)))
                            .on_hover_text("Wisdom represents intuition, willpower, and common sense. It modifies all of your saving throws.");
                        ui.label(format!("CHA: {} ({:+})", attrs.charisma, attrs.modifier(Attr::CHA)))
                            .on_hover_text("Charisma represents sociability, charm, and leadership. It modifies your reaction rolls, maximum number of henchmen, and henchmen morale.");
                        ui.label(format!("HP: {}/{}", sheet.combat_stats.health.current_hp, sheet.combat_stats.health.max_hp))
                            .on_hover_text("HP, or hit points, is the amount of damage you can take before being defeated and possibly dead.");
                        ui.label(format!("AC: {}", sheet.combat_stats.armor_class + sheet.combat_stats.modifiers.armor_class.total()))
                            .on_hover_text("Armor Class is increased by being highly armored or highly dextrous, and is subtracted from your attacker\'s attack roll.");
                        ui.label(format!("Initiative: {:+}", sheet.combat_stats.modifiers.initiative.total()))
                            .on_hover_text("Initiative determines turn order during combat.");
                        ui.label(format!("Surprise: {:+}", sheet.combat_stats.modifiers.surprise.total()))
                            .on_hover_text("A surprise roll is made whenever you might be surprised by an attacker. A high surprise modifier increases the likelyhood that you won\'t be caught off guard.");
                        ui.label(format!("ATK: {:+}", sheet.combat_stats.attack_throw))
                            .on_hover_text("Your base modifier for all attack rolls. This increases as you level up.");
                        ui.label(format!("Base damage: {}", sheet.combat_stats.damage.display()))
                            .on_hover_text("Your damage before any modifiers, given the weapon that you are holding.");
                        ui.label(format!("Melee ATK bonus: {:+}", sheet.combat_stats.modifiers.melee_attack.total()))
                            .on_hover_text("All your bonuses to melee attack rolls.");
                        ui.label(format!("Missile ATK bonus: {:+}", sheet.combat_stats.modifiers.missile_attack.total()))
                            .on_hover_text("All your bonuses to missile (ranged) attack rolls.");
                        ui.label(format!("Melee DMG bonus: {:+}", sheet.combat_stats.modifiers.melee_damage.total()))
                            .on_hover_text("All your bonuses to melee damage rolls.");
                        ui.label(format!("Missile DMG bonus: {:+}", sheet.combat_stats.modifiers.missile_damage.total()))
                            .on_hover_text("All your bonuses to missile (ranged) damage rolls (note that these are fairly rare).");
                        ui.separator();
                        let saves = sheet.combat_stats.saving_throws;
                        ui.label("Saving throws:")
                            .on_hover_text("A saving throw is made whenever your character must act quickly to save themselves. The appropriate modifier is added to the 1d20 roll (20 or higher is a success).");
                        ui.horizontal(|ui| {
                            ui.label(format!("Petrification & Paralysis: {:+}", saves.petrification_paralysis + sheet.combat_stats.modifiers.save_petrification_paralysis.total()))
                                .on_hover_text("Made to resist being rendered immobile, such as being turned to stone.");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Roll").clicked() {
                                    packets.push(ServerBoundPacket::SavingThrow(name.clone(), SavingThrowType::PetrificationParalysis));
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label(format!("Poison & Death: {:+}", saves.poison_death + sheet.combat_stats.modifiers.save_poison_death.total()))
                                .on_hover_text("Made to resist instant death or other life-threatening ailments.");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Roll").clicked() {
                                    packets.push(ServerBoundPacket::SavingThrow(name.clone(), SavingThrowType::PoisonDeath));
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label(format!("Blast & Breath: {:+}", saves.blast_breath + sheet.combat_stats.modifiers.save_blast_breath.total()))
                                .on_hover_text("Made to resist large area attacks, such as explosions, fireballs, or a collapsing building.");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Roll").clicked() {
                                    packets.push(ServerBoundPacket::SavingThrow(name.clone(), SavingThrowType::BlastBreath));
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label(format!("Staffs & Wands: {:+}", saves.staffs_wands + sheet.combat_stats.modifiers.save_staffs_wands.total()))
                                .on_hover_text("Made to resist other attacks from a magical item.");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Roll").clicked() {
                                    packets.push(ServerBoundPacket::SavingThrow(name.clone(), SavingThrowType::StaffsWands));
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label(format!("Spells: {:+}", saves.spells + sheet.combat_stats.modifiers.save_spells.total()))
                                .on_hover_text("Made to resist a magical attack not covered by any other category.");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Roll").clicked() {
                                    packets.push(ServerBoundPacket::SavingThrow(name.clone(), SavingThrowType::Spells));
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
                        ui.label(format!("XP: {}/{} ({:+.1}%)", sheet.xp, sheet.xp_to_level, sheet.combat_stats.modifiers.xp_gain.total() * 100.0));
                        ui.label(format!("Hit Die: {}", sheet.class.hit_die))
                            .on_hover_text("Your hit die is rolled whenever you level up to determine the amount of HP you gain.");
                    },
                    CharacterSheetTab::Inventory => {
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
                                            packets.push(ServerBoundPacket::EquipInventoryItem(name.clone(), PlayerEquipSlot::LeftHand, i));
                                            ui.close_menu();
                                        }
                                    }
                                    if let Some(weapon) = &item.item_type.weapon_stats {
                                        match &weapon.damage {
                                            WeaponDamage::Melee(melee) => {
                                                match melee  {
                                                    MeleeDamage::OneHanded(_) => {
                                                        if ui.button("Equip: Main Hand").clicked() {
                                                            packets.push(ServerBoundPacket::EquipInventoryItem(name.clone(), PlayerEquipSlot::RightHand, i));
                                                            ui.close_menu();
                                                        }
                                                        if ui.button("Equip: Off Hand").clicked() {
                                                            packets.push(ServerBoundPacket::EquipInventoryItem(name.clone(), PlayerEquipSlot::LeftHand, i));
                                                            ui.close_menu();
                                                        }
                                                    },
                                                    MeleeDamage::Versatile(_, _) => {
                                                        if ui.button("Equip: Main Hand").clicked() {
                                                            packets.push(ServerBoundPacket::EquipInventoryItem(name.clone(), PlayerEquipSlot::RightHand, i));
                                                            ui.close_menu();
                                                        }
                                                        if ui.button("Equip: Both Hands").clicked() {
                                                            packets.push(ServerBoundPacket::EquipInventoryItem(name.clone(), PlayerEquipSlot::BothHands, i));
                                                            ui.close_menu();
                                                        }
                                                        if ui.button("Equip: Off Hand").clicked() {
                                                            packets.push(ServerBoundPacket::EquipInventoryItem(name.clone(), PlayerEquipSlot::LeftHand, i));
                                                            ui.close_menu();
                                                        }
                                                    },
                                                    MeleeDamage::TwoHanded(_) => {
                                                        if ui.button("Equip: Both Hands").clicked() {
                                                            packets.push(ServerBoundPacket::EquipInventoryItem(name.clone(), PlayerEquipSlot::BothHands, i));
                                                            ui.close_menu();
                                                        }
                                                    },
                                                }
                                            },
                                            WeaponDamage::Missile(_, _) => {
                                                if ui.button("Equip: Both Hands").clicked() {
                                                    packets.push(ServerBoundPacket::EquipInventoryItem(name.clone(), PlayerEquipSlot::BothHands, i));
                                                    ui.close_menu();
                                                }
                                            },
                                        }
                                    }
                                    if item.item_type.armor_stats.is_some() {
                                        if ui.button("Equip: Armor").clicked() {
                                            packets.push(ServerBoundPacket::EquipInventoryItem(name.clone(), PlayerEquipSlot::Armor, i));
                                            ui.close_menu();
                                        }
                                    }
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button(format!("{}", egui_phosphor::CARET_DOWN)).clicked() {
                                        packets.push(ServerBoundPacket::MoveInventoryItem(name.clone(), i, false));
                                    }
                                    if ui.small_button(format!("{}", egui_phosphor::CARET_UP)).clicked() {
                                        packets.push(ServerBoundPacket::MoveInventoryItem(name.clone(), i, true));
                                    }
                                });
                            });
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(format!("Off hand: {}", sheet.inventory.get_equip_slot(PlayerEquipSlot::LeftHand).map_or("None", |i| &i.item_type.name)));
                            if sheet.inventory.left_hand.is_some() {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("Unequip").clicked() {
                                        packets.push(ServerBoundPacket::UnequipInventoryItem(name.clone(), PlayerEquipSlot::LeftHand));
                                    }
                                });
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(format!("Main hand: {}", sheet.inventory.get_equip_slot(PlayerEquipSlot::RightHand).map_or("None", |i| &i.item_type.name)));
                            if sheet.inventory.right_hand.is_some() {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("Unequip").clicked() {
                                        packets.push(ServerBoundPacket::UnequipInventoryItem(name.clone(), PlayerEquipSlot::RightHand));
                                    }
                                });
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(format!("Armor: {}", sheet.inventory.get_equip_slot(PlayerEquipSlot::Armor).map_or("None", |i| &i.item_type.name)));
                            if sheet.inventory.armor.is_some() {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("Unequip").clicked() {
                                        packets.push(ServerBoundPacket::UnequipInventoryItem(name.clone(), PlayerEquipSlot::Armor));
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
                        'inner: {
                            if ui.vertical_centered(|ui| {
                                if let Some((general, character)) = &data.picking_prof {
                                    if character == name {
                                        ui.label(format!("Open the proficiency viewer to select a new {} proficiency!", if *general {"general"} else {"class"}));
                                        ui.add_space(3.0);
                                        if ui.button("Actually wait, go back").clicked() {
                                            data.picking_prof = None;
                                        }
                                        return true;
                                    }
                                }
                                false  
                            }).inner {
                                break 'inner;
                            }
                            let g = sheet.proficiencies.general_slots;
                            let c = sheet.proficiencies.class_slots;
                            if g + c > 0 {
                                ui.horizontal(|ui| {
                                    if g > 0 {
                                        if ui.button(RichText::new(format!("Pick general proficiency ({})", g)).underline().color(Color32::GREEN)).clicked() {
                                            data.picking_prof = Some((true, name.clone()));
                                            data.viewed_prof = None;
                                        }
                                    }
                                    if c > 0 {
                                        if ui.button(RichText::new(format!("Pick class proficiency ({})", c)).underline().color(Color32::YELLOW)).clicked() {
                                            data.picking_prof = Some((false, name.clone()));
                                            data.viewed_prof = None;
                                        }
                                    }
                                });
                                ui.separator();
                            }
                            for ((id, _), prof) in &sheet.proficiencies.profs {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(prof.display()).strong());
                                    ui.add_space(5.0);
                                    if ui.small_button(format!("{}", egui_phosphor::ARROW_SQUARE_OUT)).clicked() {
                                        data.viewed_prof = Some(id.clone());
                                        (self.callback)(PlayerTab::ProficiencyViewer, true);
                                    }
                                });
                            }
                        }
                    },
                    CharacterSheetTab::Spells => {
                        if let Some(divine) = &sheet.divine_spells {
                            ui.vertical_centered(|ui| {
                                ui.heading("Divine Spellcaster");
                                ui.add_space(5.0);
                                for (i, &(curr, max)) in divine.spell_slots.iter().enumerate() {
                                    if max > 0 {
                                        ui.horizontal(|ui| {
                                            let show_slots = |ui: &mut egui::Ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(format!("Level {}:", i + 1));
                                                    for j in 0..max {
                                                        if curr > j {
                                                            ui.colored_label(Color32::GREEN, format!("{}", egui_phosphor::STAR));
                                                        } else {
                                                            ui.colored_label(Color32::RED, format!("{}", egui_phosphor::STAR_HALF));
                                                        }
                                                    }
                                                    ui.label(RichText::new(format!("{}/{}", curr, max)).weak());
                                                });
                                            };
                                            if egui::CollapsingHeader::new(RichText::new("").size(1.0))
                                                .id_source(format!("{}_divine_spells_{}", name, i))
                                                .show_unindented(ui, |ui| {
                                                    show_slots(ui);
                                                    ui.separator();
                                                    for spell_id in &divine.spell_repertoire[i] {
                                                        if let Some(spell) = data.spell_registry.divine[i].get(spell_id) {
                                                            ui.horizontal(|ui| {
                                                                ui.label(&spell.name);
                                                                if ui.small_button(format!("{}", egui_phosphor::ARROW_SQUARE_OUT)).clicked() {
                                                                    data.viewed_spell = Some((spell.magic_type, Some((spell.spell_level, Some(spell_id.clone())))));
                                                                    (self.callback)(PlayerTab::SpellViewer, true);
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
                        if let Some(arcane) = &sheet.arcane_spells {
                            ui.vertical_centered(|ui| {
                                ui.heading("Arcane Spellcaster");
                                ui.add_space(5.0);
                                for (i, &(curr, max)) in arcane.spell_slots.iter().enumerate() {
                                    if max > 0 {
                                        ui.horizontal(|ui| {
                                            let show_slots = |ui: &mut egui::Ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(format!("Level {}:", i + 1));
                                                    for j in 0..max {
                                                        if curr > j {
                                                            ui.colored_label(Color32::GREEN, format!("{}", egui_phosphor::STAR));
                                                        } else {
                                                            ui.colored_label(Color32::RED, format!("{}", egui_phosphor::STAR_HALF));
                                                        }
                                                    }
                                                    ui.label(RichText::new(format!("{}/{}", curr, max)).weak());
                                                });
                                            };
                                            if egui::CollapsingHeader::new(RichText::new("").size(1.0))
                                                .id_source(format!("{}_arcane_spells_{}", name, i))
                                                .show_unindented(ui, |ui| {
                                                    show_slots(ui);
                                                    ui.separator();
                                                    ui.label(format!("Repertoire size: {}/{}", arcane.spell_repertoire[i].0.len(),  arcane.spell_repertoire[i].1));
                                                    for spell_id in &arcane.spell_repertoire[i].0 {
                                                        if let Some(spell) = data.spell_registry.arcane[i].get(spell_id) {
                                                            ui.horizontal(|ui| {
                                                                ui.label(&spell.name);
                                                                if ui.small_button(format!("{}", egui_phosphor::ARROW_SQUARE_OUT)).clicked() {
                                                                    data.viewed_spell = Some((spell.magic_type, Some((spell.spell_level, Some(spell_id.clone())))));
                                                                    (self.callback)(PlayerTab::SpellViewer, true);
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
                                ui.label("This character isn't a spellcaster. Sorry!")
                            });
                        }
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
            if update {
                packets.push(ServerBoundPacket::RequestCharacterUpdate(name.clone(), Some(sheet.clone())));
            }
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "This character doesn't appear to exist!");
        }
        for packet in packets {
            data.send_to_server(packet);
        }
    }
    fn class_viewer(&mut self, ui: &mut Ui) {
        let data = &mut *self.data;
        let mut go_back = false;
        match &mut data.viewed_class {
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
                                        if data.new_char_name.is_none() && data.picked_character.is_some() {
                                            if ui.button("Pick!").clicked() {
                                                data.new_char_class = Some(class.clone());
                                                go_back = true;
                                                (self.callback)(PlayerTab::ClassViewer, false);
                                            }
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
                                match class.divine_value {
                                    DivineValue::None => {},
                                    DivineValue::One(turn) => {
                                        ui.label("Divine value: 1");
                                        if turn {
                                            ui.label(format!("The {} class can turn undead.", class.name));
                                        }
                                    },
                                    DivineValue::Two(turn) => {
                                        ui.label("Divine value: 2");
                                        if turn {
                                            ui.label(format!("The {} class can turn undead.", class.name));
                                        }
                                    },
                                    DivineValue::Three(turn) => {
                                        ui.label("Divine value: 3");
                                        if turn {
                                            ui.label(format!("The {} class can turn undead.", class.name));
                                        }
                                    },
                                    DivineValue::Four(turn) => {
                                        ui.label("Divine value: 4");
                                        if turn {
                                            ui.label(format!("The {} class can turn undead.", class.name));
                                        }
                                    },
                                }
                                match class.arcane_value {
                                    ArcaneValue::None => {},
                                    ArcaneValue::One(_) => {
                                        ui.label("Arcane value: 1");
                                    },
                                    ArcaneValue::Two(_) => {
                                        ui.label("Arcane value: 2");
                                    },
                                    ArcaneValue::Three(_) => {
                                        ui.label("Arcane value: 3");
                                    },
                                    ArcaneValue::Four => {
                                        ui.label("Arcane value: 4");
                                    },
                                }
                            },
                            RegistryNode::SubRegistry(map) => {
                                if data.picked_character.is_some() && data.new_char_name.is_none() {
                                    go_back = true;
                                }
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
                        data.viewed_class = None;
                    },
                }
            },
            None => {
                'inner: {
                    if data.class_registry.tree.is_empty() {
                        ui.label(RichText::new("There\'s nothing here...").weak().italics());
                    }
                    if data.new_char_name.is_none() {
                        if let Some(i) = data.picked_character {
                            if let Some(sheet) = data.new_characters.get(i) {
                                fn recurse(reg: &HashMap<String, RegistryNode<Class>>, ui: &mut egui::Ui, sheet: &PlayerCharacter) -> Option<String> {
                                    for (path, node) in reg {
                                        match node {
                                            RegistryNode::Value(class) => {
                                                if class.race == sheet.race {
                                                    let mut allowed = true;
                                                    for req in &class.prime_reqs {
                                                        if match *req {
                                                            Attr::STR => sheet.combat_stats.attributes.strength,
                                                            Attr::DEX => sheet.combat_stats.attributes.dexterity,
                                                            Attr::CON => sheet.combat_stats.attributes.constitution,
                                                            Attr::INT => sheet.combat_stats.attributes.intelligence,
                                                            Attr::WIS => sheet.combat_stats.attributes.wisdom,
                                                            Attr::CHA => sheet.combat_stats.attributes.charisma,
                                                        } < 9 {
                                                            allowed = false;
                                                            break;
                                                        }
                                                    }
                                                    if allowed {
                                                        if ui.button(format!("View: {}", class.name)).clicked() {
                                                            return Some(path.clone());
                                                        } 
                                                    }
                                                }
                                            },
                                            RegistryNode::SubRegistry(sub) => {
                                                if let Some(value) = recurse(sub, ui, sheet) {
                                                    return Some(format!("{}/{}", path, value));
                                                }
                                            },
                                        }
                                    }
                                    None
                                }
                                if let Some(path) = recurse(&data.class_registry.tree, ui, sheet) {
                                    data.viewed_class = Some(path);
                                }
                                break 'inner;
                            }
                        }
                    }
                    for (path, node) in &data.class_registry.tree {
                        match node {
                            RegistryNode::Value(class) => {
                                if ui.button(format!("View: {}", class.name)).clicked() {
                                    data.viewed_class = Some(path.clone());
                                }
                            },
                            RegistryNode::SubRegistry(_) => {
                                if ui.button(format!("Folder: {}", path)).clicked() {
                                    data.viewed_class = Some(path.clone());
                                }
                            },
                        }
                    }
                }
            },
        }
        if go_back {
            if data.picked_character.is_some() && data.new_char_name.is_none() {
                data.viewed_class = None;
            }
            if let Some(path) = &mut data.viewed_class {
                data.viewed_class = path.rsplit_once("/").map(|(s, _)| s.to_owned());
            }
        }
    }
    fn prof_viewer(&mut self, ui: &mut Ui) {
        let data = &mut *self.data;
        let mut go_back = false;
        let mut packets = Vec::new();
        if let Some(id) = &data.viewed_prof {
            if let Some(prof) = data.proficiency_registry.get(id) {
                ui.horizontal(|ui| {
                    if back_arrow(ui) {
                        go_back = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some((general, character)) = &data.picking_prof {
                            if prof.requires_specification {
                                if let Some(sheet) = data.characters.get(character) {
                                    if let Some(class_valid) = sheet.class.class_proficiencies.get(id).map_or(&None, |m| if *general {&None} else {m}) {
                                        if data.viewed_prof_spec.is_none() {
                                            let mut temp = None;
                                            for v in class_valid {
                                                temp = Some(v.clone());
                                                break;
                                            }
                                            if temp.is_none() {
                                                temp = Some("".to_owned());
                                            }
                                            data.viewed_prof_spec = temp;
                                        }
                                        if let Some(spec) = &mut data.viewed_prof_spec {
                                            egui::ComboBox::from_label("Type")
                                                .selected_text(spec.as_str())
                                                .show_ui(ui, |ui| {
                                                    for v in class_valid {
                                                        ui.selectable_value(spec, v.to_string(), v);
                                                    }
                                                });
                                        }
                                    } else {
                                        if let Some(valid) = &prof.valid_specifications {
                                            if data.viewed_prof_spec.is_none() {
                                                let mut temp = None;
                                                for v in valid {
                                                    temp = Some(v.clone());
                                                    break;
                                                }
                                                if temp.is_none() {
                                                    temp = Some("".to_owned());
                                                }
                                                data.viewed_prof_spec = temp;
                                            }
                                            if let Some(spec) = &mut data.viewed_prof_spec {
                                                egui::ComboBox::from_label("Type")
                                                    .selected_text(spec.as_str())
                                                    .show_ui(ui, |ui| {
                                                        for v in valid {
                                                            ui.selectable_value(spec, v.to_string(), v);
                                                        }
                                                    });
                                            }
                                        } else {
                                            if data.viewed_prof_spec.is_none() {
                                                data.viewed_prof_spec = Some("".to_owned());
                                            }
                                            if let Some(spec) = &mut data.viewed_prof_spec {
                                                ui.add(egui::TextEdit::singleline(spec).clip_text(true).desired_width(150.0).hint_text("Specify type..."));
                                            }
                                        }
                                    }
                                } else {
                                    go_back = true;
                                }
                            }
                            if ui.button("Pick!").clicked() {
                                if data.viewed_prof_spec.as_ref().map_or(true, |s| !s.is_empty()) {
                                    packets.push(ServerBoundPacket::PickNewProficiency(character.clone(), *general, id.clone(), data.viewed_prof_spec.clone()));
                                    data.picking_prof = None;
                                    data.viewed_prof_spec = None;
                                    (self.callback)(PlayerTab::ProficiencyViewer, false);
                                }
                            }
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
                data.viewed_prof = None;
            }
        } else {
            if let Some((general, character)) = &data.picking_prof {
                if let Some(sheet) = data.characters.get(character) {
                    data.viewed_prof_spec = None;
                    for (id, prof) in &data.proficiency_registry {
                        // if it's a general prof or a class prof we have
                        if (*general && prof.is_general) || (!general && sheet.class.class_proficiencies.contains_key(id)) {
                            // if we don't already have the prof or we could take it more than once
                            if sheet.proficiencies.profs.get(&(id.to_string(), None)).map_or(true, |p| p.prof_level < prof.max_level)  {
                                if ui.button(&prof.name).clicked() {
                                    data.viewed_prof = Some(id.clone());
                                }
                            }
                        }
                    }
                }
            } else {
                for (id, name) in &data.sorted_prof_list {
                    if ui.button(name).clicked() {
                        data.viewed_prof = Some(id.clone());
                    }
                }
            }
        }
        if go_back {
            data.viewed_prof = None;
        }
        for packet in packets {
            data.send_to_server(packet);
        }
    }
    fn spell_viewer(ui: &mut Ui, data: &mut PlayerAppData) {
        let mut go_back = false;
        match &mut data.viewed_spell {
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
                    data.viewed_spell = Some((MagicType::Arcane, None));
                }
                if ui.button("Divine").clicked() {
                    data.viewed_spell = Some((MagicType::Divine, None));
                }
            },
        }
        if go_back {
            if let Some((_, maybe_lvl)) = &mut data.viewed_spell {
                if let Some((_, maybe_spell)) = maybe_lvl {
                    if maybe_spell.is_some() {
                        *maybe_spell = None;
                    } else {
                        *maybe_lvl = None;
                    }
                } else {
                    data.viewed_spell = None;
                }
            }
        }
    }
    fn display_spell(ui: &mut egui::Ui, spell: &Spell) {
        ui.heading(&spell.name);
        ui.label(format!("{} {}{}", spell.magic_type, spell.spell_level + 1, if spell.reversed.is_some() {" (Reversible)"} else {""}));
        ui.label(format!("Range: {}", spell.range));
        ui.label(format!("Duration: {}", spell.duration));
        ui.separator();
        ui.label(RichText::new(&spell.description).weak().italics());
    }
    fn character_generator(&mut self, ui: &mut Ui) {
        let data = &mut *self.data;
        ui.style_mut().wrap = Some(false);
        if data.new_characters.is_empty() {
            if data.requests.get_status(Request::GenerateCharacters).is_none() {
                if ui.button("Request to generate new characters").clicked() {
                    data.requests.make_request(Request::GenerateCharacters);
                }
            } else {
                match data.requests.consume(Request::GenerateCharacters) {
                    Some(approved) => {
                        if approved {
                            for _ in 0..5 {
                                data.new_characters.push(PlayerCharacter::random());
                            }
                        }
                    },
                    None => {
                        ui.label("Waiting for the DM to answer your request...");
                    },
                }
            }
        } else if let Some(i) = data.picked_character {
            ui.vertical_centered(|ui| {
                if let Some(name) = &mut data.new_char_name {
                    if (ui.add(egui::TextEdit::singleline(name).hint_text("Give your new character a name...")).lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))) || ui.button("Ok").clicked() {
                        if let Some(sheet) = data.new_characters.get(i) {
                            let name = name.clone();
                            data.send_to_server(ServerBoundPacket::CreateNewCharacter(name, sheet.clone()));
                        }
                    }
                    if let Some(e) = data.new_char_name_error {
                        ui.colored_label(ui.visuals().error_fg_color, format!("{}", e));
                    } else {
                        ui.colored_label(ui.visuals().error_fg_color, "This cannot be changed.");
                    }
                } else {
                    match &data.new_char_class {
                        None => {
                            if let Some(sheet) = data.new_characters.get(i) {
                                if sheet.class.name.is_empty() {
                                    ui.label("You must pick a class for your new character. Open the class viewer and press the \"pick\" button once you have decided.");
                                } else {
                                    ui.label(format!("Are you sure you want your character to be a {}? This cannot be changed later.", sheet.class.name));
                                    ui.label(RichText::new("Until you click yes, you can still pick a different class.").weak().italics());
                                    if ui.button("Yes").clicked() {
                                        data.new_char_name = Some("".to_owned());
                                    }
                                }
                            }
                        },
                        Some(class) => {
                            if let Some(sheet) = data.new_characters.get_mut(i) {
                                sheet.class = class.clone();
                                data.new_char_class = None;
                            }
                        },
                    }
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
                            (self.callback)(PlayerTab::ClassViewer, true);
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
    }
}

impl<'a, F: FnMut(PlayerTab, bool) + 'a> TabViewer for PlayerTabViewer<'a, F> {
    type Tab = PlayerTab;

    fn add_popup(&mut self, ui: &mut Ui, _node: egui_dock::NodeIndex) {
        ui.horizontal(|ui| {
            if ui.button("Chat").clicked() {
                (self.callback)(PlayerTab::Chat, true);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Notes").clicked() {
                (self.callback)(PlayerTab::Notes, true);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Characters").clicked() {
                (self.callback)(PlayerTab::Characters, true);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Character Generator").clicked() {
                (self.callback)(PlayerTab::CharacterGenerator, true);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Class Viewer").clicked() {
                (self.callback)(PlayerTab::ClassViewer, true);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Proficiency Viewer").clicked() {
                (self.callback)(PlayerTab::ProficiencyViewer, true);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Spell Viewer").clicked() {
                (self.callback)(PlayerTab::SpellViewer, true);
            }
        });
    }

    fn context_menu(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            PlayerTab::Chat => {
                if ui.button("Detatch").clicked() {
                    self.data.window_states.insert("chat_window".to_owned(), true);
                    (self.callback)(PlayerTab::Chat, false);
                    ui.close_menu();
                }
            },
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            PlayerTab::Chat => {
                self.data.unread_messages = 0;
                chat(ui, self.data);
            },
            PlayerTab::Notes => {
                ui.vertical_centered_justified(|ui| {
                    if ui.text_edit_multiline(&mut self.data.notes).lost_focus() {
                        self.data.send_to_server(ServerBoundPacket::UpdatePlayerNotes(self.data.notes.clone()));
                    }
                });
            },
            PlayerTab::Characters => {
                ui.vertical(|ui| {
                    for (name, _) in &self.data.characters {
                        if ui.button(format!("View: {}", name)).clicked() {
                            (self.callback)(PlayerTab::Character(name.clone()), true);
                        }
                    }
                });
            },
            PlayerTab::Character(name) => {
                self.character_sheet(ui, name);
            },
            PlayerTab::ClassViewer => {
                self.class_viewer(ui);
            },
            PlayerTab::ProficiencyViewer => {
                self.prof_viewer(ui);
            },
            PlayerTab::SpellViewer => {
                Self::spell_viewer(ui, self.data);
            },
            PlayerTab::CharacterGenerator => {
                self.character_generator(ui);
            },
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        if *tab == PlayerTab::Chat {
            self.data.get_chat_title()
        } else {
            tab.to_string().into()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
pub enum PlayerTab {
    /// Chat
    Chat,
    /// Notes
    Notes,
    /// Characters
    Characters,
    /// {0}
    Character(String),
    /// Character Generator
    CharacterGenerator,
    /// Class Viewer
    ClassViewer,
    /// Proficiency Viewer
    ProficiencyViewer,
    /// Spell Viewer
    SpellViewer,
}

impl eframe::App for PlayerApp {
    fn on_close_event(&mut self) -> bool {
        let data = &mut *self.data.lock().unwrap();
        if let Ok(s) = std::fs::read_to_string("preferences.ron") {
            if let Ok(mut prefs) = ron::from_str::<AppPreferences>(&s) {
                prefs.player_window = Some(data.prefs.clone());
                prefs.player_last_ip = Some(data.ip_address.clone());
                if data.remember_me {
                    prefs.player_login = Some((data.username.clone(), data.password.clone()));
                } else {
                    prefs.player_login = None;
                }
                let _ = std::fs::write("preferences.ron", ron::to_string(&prefs).unwrap_or(s));
            }
        }
        true
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        ctx.request_repaint();
        let data = &mut *self.data.lock().unwrap();
        let info = frame.info().window_info;
        let pos = info.position.unwrap_or_default();
        data.prefs.pos = (pos.x, pos.y);
        data.prefs.size = (info.size.x, info.size.y);
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
        Self::chat_window(ctx, data, &mut self.tree);
        Self::combat_action_window(ctx, data);
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.tree.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() / 2.0) - 10.0);
                    if ui.add(egui::Button::new("Oh no, you closed the last tab! Click me to get one back.").frame(false)).clicked() {
                        self.tree.push_to_first_leaf(PlayerTab::Chat);
                    }
                });
            } else {
                let mut new_tab = None;
                let mut remove_tab = None;
                DockArea::new(&mut self.tree)
                    .show_add_buttons(true)
                    .show_add_popup(true)
                    .show_inside(ui, &mut PlayerTabViewer {
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
        data.unread_msg_buffer = false;
        let mut requests = Vec::new();
        for (request, status) in &mut data.requests.map {
            if *status == RequestStatus::Sending {
                *status = RequestStatus::AwaitingResponse;
                requests.push(*request);
            }
        }
        for request in requests {
            data.send_to_server(ServerBoundPacket::MakeRequest(request));
        }
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

pub struct Requests {
    map: HashMap<Request, RequestStatus>,
}

impl Requests {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
    pub fn make_request(&mut self, request: Request) {
        if !self.map.contains_key(&request) {
            self.map.insert(request, RequestStatus::Sending);
        }
    }
    /// Gets the status of a request without removing it.
    pub fn get_status(&self, request: Request) -> Option<RequestStatus> {
        self.map.get(&request).copied()
    }
    /// Tries to read a request result. Returns `None` if the request is unanswered, otherwise
    /// returns whether the request was approved and removes the request.
    pub fn consume(&mut self, request: Request) -> Option<bool> {
        match self.map.get(&request) {
            Some(&status) => {
                match status {
                    RequestStatus::Approved => {
                        self.map.remove(&request);
                        Some(true)
                    },
                    RequestStatus::Denied => {
                        self.map.remove(&request);
                        Some(false)
                    },
                    _ => None,
                }
            },
            None => None,
        }
    }
    pub fn set_approval(&mut self, request: Request, approved: bool) {
        if let Some(status) = self.map.get_mut(&request) {
            *status = if approved {
                RequestStatus::Approved
            } else {
                RequestStatus::Denied
            };
        }
    }
}

#[simple_enum]
pub enum RequestStatus {
    Sending,
    AwaitingResponse,
    Approved,
    Denied,
}