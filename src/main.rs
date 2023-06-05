use std::{rc::Rc, cell::RefCell};

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
pub mod common_ui;
pub mod combat;
pub mod enemy;
pub mod item;

fn main() -> Result<(), eframe::Error> {
    // have to do some fuckery with interior mutability to store the button press between applications
    let is_dm: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));
    let is_dm_clone = Rc::clone(&is_dm);
    let _ = eframe::run_native(
        "TTRPG Tool", 
        eframe::NativeOptions {
            centered: true,
            initial_window_size: Some(eframe::egui::vec2(400.0, 300.0)),
            ..Default::default()
        },
        Box::new(|_ctx| {
            Box::new(StartupApp { is_dm: is_dm_clone })
        })
    );

    if let Some(dm) = *is_dm.borrow() {
        if dm {
            return dm_app::run();
        } else {
            return player_app::run();
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