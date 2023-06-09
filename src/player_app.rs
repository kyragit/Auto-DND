use crate::character::{PlayerCharacter, Attr, PlayerEquipSlot};
use crate::class::{Class, ClassDamageBonus, Cleaves};
use crate::combat::{CombatantType, SavingThrowType};
use crate::common_ui::{CommonApp, CharacterSheetTab, self};
use crate::dm_app::{Registry, RegistryNode};
use crate::item::{WeaponDamage, MeleeDamage, ContainerStats};
use crate::proficiency::Proficiency;
use eframe::egui::{self, RichText};
use eframe::epaint::{Rgba, Color32};
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
    pub class_registry: Registry<Class>,
    pub proficiency_registry: HashMap<String, Proficiency>,
    pub sorted_prof_list: Vec<(String, String)>,
    pub viewed_class: Option<String>,
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
            class_registry: Registry::new(),
            proficiency_registry: HashMap::new(),
            sorted_prof_list: Vec::new(),
            viewed_class: None,
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
                                            if ui.small_button("⏷").clicked() {
                                                packets.push(ServerBoundPacket::MoveInventoryItem(name.clone(), i, false));
                                            }
                                            if ui.small_button("⏶").clicked() {
                                                packets.push(ServerBoundPacket::MoveInventoryItem(name.clone(), i, true));
                                            }
                                        });
                                    });
                                });
                                ui.separator();
                                ui.horizontal(|ui| {
                                    ui.label(format!("Off hand: {}", sheet.inventory.get_equip_slot(PlayerEquipSlot::LeftHand).map_or("None", |i| &i.item_type.name)));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("Unequip").clicked() {
                                            packets.push(ServerBoundPacket::UnequipInventoryItem(name.clone(), PlayerEquipSlot::LeftHand));
                                        }
                                    });
                                });
                                ui.horizontal(|ui| {
                                    ui.label(format!("Main hand: {}", sheet.inventory.get_equip_slot(PlayerEquipSlot::RightHand).map_or("None", |i| &i.item_type.name)));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("Unequip").clicked() {
                                            packets.push(ServerBoundPacket::UnequipInventoryItem(name.clone(), PlayerEquipSlot::RightHand));
                                        }
                                    });
                                });
                                ui.horizontal(|ui| {
                                    ui.label(format!("Armor: {}", sheet.inventory.get_equip_slot(PlayerEquipSlot::Armor).map_or("None", |i| &i.item_type.name)));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("Unequip").clicked() {
                                            packets.push(ServerBoundPacket::UnequipInventoryItem(name.clone(), PlayerEquipSlot::Armor));
                                        }
                                    });
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
                                            if ui.small_button("↪").clicked() {
                                                data.viewed_prof = Some(id.clone());
                                                data.window_states.insert("prof_viewer".to_owned(), true);
                                            }
                                        });
                                    }
                                }
                            },
                            CharacterSheetTab::Spells => {

                            },
                            CharacterSheetTab::Notes => {
                                egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                                    if ui.text_edit_multiline(&mut sheet.notes).clicked_elsewhere() {
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
        let open = data.window_states.entry("character_generator".to_owned()).or_insert(false);
        let mut temp_open = open.clone();
        egui::Window::new("Character Generator")
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .resizable(false)
            .open(&mut temp_open)
            .show(ctx, |ui| {
                ui.style_mut().wrap = Some(false);
                if data.new_characters.is_empty() {
                    if ui.button("Generate!").clicked() {
                        for _ in 0..5 {
                            data.new_characters.push(PlayerCharacter::random());
                        }
                    }
                } else if let Some(i) = data.picked_character {
                    ui.vertical_centered(|ui| {
                        if let Some(name) = &mut data.new_char_name {
                            if (ui.add(egui::TextEdit::singleline(name).hint_text("Give your new character a name...")).lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))) || ui.button("Ok").clicked() {
                                if let Some(sheet) = data.new_characters.get(i) {
                                    maybe_packet = Some(ServerBoundPacket::CreateNewCharacter(name.clone(), sheet.clone()));
                                }
                            }
                            if let Some(e) = data.new_char_name_error {
                                ui.colored_label(ctx.style().visuals.error_fg_color, format!("{}", e));
                            } else {
                                ui.colored_label(ctx.style().visuals.error_fg_color, "This cannot be changed.");
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
        data.window_states.insert("character_generator".to_owned(), temp_open);
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

    pub fn class_viewer_window(ctx: &egui::Context, data: &mut PlayerAppData) {
        let open = &mut data.window_states.entry("class_viewer".to_owned()).or_insert(false);
        let mut temp_open = open.clone();
        let mut force_close = false;
        egui::Window::new("Class Viewer")
            .collapsible(true)
            .resizable(true)
            .vscroll(true)
            .open(&mut temp_open)
            .show(ctx, |ui| {
                let mut go_back = false;
                match &mut data.viewed_class {
                    Some(path) => {
                        match data.class_registry.get(path) {
                            Some(node) => {
                                match node {
                                    RegistryNode::Value(class) => {
                                        ui.horizontal(|ui| {
                                            if ui.small_button("⬅").clicked() {
                                                go_back = true;
                                            }
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if data.new_char_name.is_none() && data.picked_character.is_some() {
                                                    if ui.button("Pick!").clicked() {
                                                        data.new_char_class = Some(class.clone());
                                                        force_close = true;
                                                    }
                                                }
                                            });
                                        });
                                        ui.separator();
                                        ui.heading(&class.name);
                                        ui.label(RichText::new(&class.description).weak().italics());
                                        ui.separator();
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
                        if data.class_registry.tree.is_empty() {
                            ui.label(RichText::new("There\'s nothing here...").weak().italics());
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
                    },
                }
                if go_back {
                    if let Some(path) = &mut data.viewed_class {
                        data.viewed_class = path.rsplit_once("/").map(|(s, _)| s.to_owned());
                    }
                }
            });
        if force_close {
            temp_open = false;
        }
        data.window_states.insert("class_viewer".to_owned(), temp_open);
    }
    pub fn prof_viewer_window(ctx: &egui::Context, data: &mut PlayerAppData) {
        let open = data.window_states.entry("prof_viewer".to_owned()).or_insert(false);
        let mut temp_open = open.clone();
        let mut force_close = false;
        egui::Window::new("Proficiency Viewer")
            .resizable(true)
            .collapsible(true)
            .vscroll(true)
            .open(&mut temp_open)
            .show(ctx, |ui| {
                let mut go_back = false;
                let mut packets = Vec::new();
                if let Some(id) = &data.viewed_prof {
                    if let Some(prof) = data.proficiency_registry.get(id) {
                        ui.horizontal(|ui| {
                            if ui.small_button("⬅").clicked() {
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
                                            force_close = true;
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
            });
        if force_close {
            temp_open = false;
        }
        data.window_states.insert("prof_viewer".to_owned(), temp_open);
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
        Self::class_viewer_window(ctx, data);
        Self::prof_viewer_window(ctx, data);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                if ui.button("Open Player Notes").clicked() {
                    data.toggle_window_state("notes_open");
                }
                if ui.button("Class Viewer").clicked() {
                    data.toggle_window_state("class_viewer");
                }
                if ui.button("Proficiency Viewer").clicked() {
                    data.toggle_window_state("prof_viewer");
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
