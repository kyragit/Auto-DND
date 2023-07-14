//#![windows_subsystem = "windows"]

use std::{rc::Rc, cell::RefCell};

use eframe::NativeOptions;
use serde::{Serialize, Deserialize};

/// Simulated dice roller.
pub mod dice;
/// Player character data.
pub mod character;
/// Packets sent between the server and client.
pub mod packets;
/// Mortal Wounds Table automation.
pub mod mortal_wounds;
/// The DM (server-side) application.
pub mod dm_app;
/// The player-facing (client-side) application.
pub mod player_app;
/// Anything related to character classes.
pub mod class;
/// Character race information.
pub mod race;
/// Some shared UI and utility code.
pub mod common_ui;
/// Combat automation.
pub mod combat;
/// Monsters and the like.
pub mod enemy;
/// Items, weapons, etc.
pub mod item;
/// Proficiency logic.
pub mod proficiency;
/// Everything related to spells and magic.
pub mod spell;
pub mod party;
pub mod map;

fn main() -> Result<(), eframe::Error> {
    // have to do some fuckery with interior mutability to store the button press between applications
    let is_dm: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));
    let is_dm_clone = Rc::clone(&is_dm);
    let _ = eframe::run_native(
        "TTRPG Tool", 
        eframe::NativeOptions {
            centered: true,
            initial_window_size: Some(eframe::egui::vec2(400.0, 300.0)),
            follow_system_theme: false,
            ..Default::default()
        },
        Box::new(|_ctx| {
            Box::new(StartupApp { is_dm: is_dm_clone })
        })
    );

    if let Some(dm) = *is_dm.borrow() {
        let prefs = if let Ok(s) = std::fs::read_to_string("preferences.ron") {
            if let Ok(p) = ron::from_str::<AppPreferences>(&s) {
                p
            } else {
                let _ = std::fs::write("preferences.ron", ron::to_string(&AppPreferences::default()).unwrap_or(String::new()));
                AppPreferences::default()
            }
        } else {
            let _ = std::fs::write("preferences.ron", ron::to_string(&AppPreferences::default()).unwrap_or(String::new()));
            AppPreferences::default()
        };
        if dm {
            return dm_app::run(prefs);
        } else {
            return player_app::run(prefs);
        }
    }
    Ok(())
}

struct StartupApp {
    is_dm: Rc<RefCell<Option<bool>>>
}

impl eframe::App for StartupApp {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 2.5);
                ui.label("I am a...");
                if ui.button("Player").clicked() {
                    let mut borrow = self.is_dm.borrow_mut();
                    *borrow = Some(false);
                    frame.close();
                }
                if ui.button("DM").clicked() {
                    let mut borrow = self.is_dm.borrow_mut();
                    *borrow = Some(true);
                    frame.close();
                }
            });
        });
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppPreferences {
    pub dm_window: Option<WindowPreferences>,
    pub player_window: Option<WindowPreferences>,
    pub player_last_ip: Option<String>,
    pub player_login: Option<(String, String)>,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            dm_window: None,
            player_window: None,
            player_last_ip: None,
            player_login: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowPreferences {
    pub pos: (f32, f32),
    pub size: (f32, f32),
}

impl WindowPreferences {
    pub fn new() -> Self {
        Self {
            pos: (0.0, 0.0),
            size: (100.0, 100.0),
        }
    }
    pub fn to_native_options(&self) -> NativeOptions {
        NativeOptions {
            initial_window_pos: Some(eframe::egui::pos2(self.pos.0, self.pos.1)),
            initial_window_size: Some(eframe::egui::vec2(self.size.0, self.size.1)),
            follow_system_theme: false,
            ..Default::default()
        }
    }
}