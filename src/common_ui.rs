use std::hash::Hash;

use crate::{dice::{DiceRoll, ModifierType, Drop}, combat::{DamageRoll, StatMod}, dm_app::{Registry, RegistryNode}, enemy::EnemyType};
use eframe::{egui::{self, Ui, RichText, Button, TextFormat}, epaint::{text::LayoutJob, Color32}, emath::Align};
use egui::{FontId, Stroke, Id, WidgetText};
use serde::{Serialize, Deserialize};
use simple_enum_macro::simple_enum;

/// Adds a tab bar to a `Ui`. 
/// 
/// ## Parameters
/// - `state`: the stored state of the tab. It will be mutated when the tabs are clicked.
/// - `unique_id`: a string representing this tab bar uniquely. For technical reasons, it is 
/// important that this is globally unique.
/// - `ui`: the `Ui` to add the tab bar to.
/// - `on_change`: called when the tab changes, passing in the old tab and the new tab (in that order).
/// - `func`: a function that contains your ui code. The selected tab is passed as an argument 
/// to react to.
/// 
/// ## Example
/// ```rust
/// // in some ui
/// ui.vertical(|ui| {
///     common_ui::tabs(&mut data.tab_state, "my_tab".to_owned(), ui, |ui, tab| {
///         match tab {
///             // ...
///         }
///     });
/// });
/// ```
pub fn tabs<R, T: TabValue, F: FnOnce(&mut Ui, T) -> R, C: FnOnce(T, T)>(state: &mut T, unique_id: String, ui: &mut Ui, on_change: C, func: F) -> R {
    egui::TopBottomPanel::top(unique_id).show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            for tab in T::iterate() {
                let old = *state;
                if ui.selectable_value(state, tab, RichText::new(tab.display()).background_color(ui.style().visuals.faint_bg_color)).clicked() {
                    on_change(old, tab);
                    break;
                }
            }
        });
    });
    ui.add_space(4.0);
    func(ui, *state)
}

/// A value (probably an enum) that represents a tab.
pub trait TabValue: PartialEq + Sized + Copy {
    /// Returns a list of all tabs, in order.
    fn iterate() -> Vec<Self>;
    /// The display name of the tab.
    fn display(&self) -> String;
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum CharacterSheetTab {
    #[default]
    Stats,
    Class,
    Inventory,
    Proficiencies,
    Spells,
    Notes,
}

impl TabValue for CharacterSheetTab {
    fn iterate() -> Vec<Self> {
        vec![
            Self::Stats,
            Self::Class,
            Self::Inventory,
            Self::Proficiencies,
            Self::Spells,
            Self::Notes,
        ]
    }
    fn display(&self) -> String {
        match self {
            Self::Stats => "Stats",
            Self::Class => "Class",
            Self::Inventory => "Inventory",
            Self::Proficiencies => "Proficiencies",
            Self::Spells => "Spells",
            Self::Notes => "Notes",
        }.to_owned()
    }
}

/// Creates a dice roll editor in this `Ui`. The passed in `DiceRoll` will be modified in-place.
pub fn dice_roll_editor(ui: &mut Ui, roll: &mut DiceRoll) {
    ui.add(egui::Slider::new(&mut roll.amount, 1..=10).text("Amount").clamp_to_range(false));
    ui.add(egui::Slider::new(&mut roll.sides, 1..=20).text("Sides").clamp_to_range(false));
    ui.add(egui::Slider::new(&mut roll.modifier, -10..=10).text("Modifier").clamp_to_range(false));
    egui::ComboBox::from_label("Modifier type")
        .selected_text(roll.modifier_type.to_string())
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut roll.modifier_type,
                ModifierType::Add, ModifierType::Add.to_string());
            ui.selectable_value(&mut roll.modifier_type,
                ModifierType::Multiply, ModifierType::Multiply.to_string());
            ui.selectable_value(&mut roll.modifier_type,
                ModifierType::DivideFloor, ModifierType::DivideFloor.to_string());
            ui.selectable_value(&mut roll.modifier_type,
                ModifierType::DivideCeil, ModifierType::DivideCeil.to_string());
            ui.selectable_value(&mut roll.modifier_type,
                ModifierType::DivideRound, ModifierType::DivideRound.to_string());
    });
    ui.checkbox(
        &mut roll.apply_modifier_to_all, 
        "Apply modifier to each die"
    );
    egui::ComboBox::from_label("Drop")
        .selected_text(roll.drop.to_string())
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut roll.drop,
                Drop::None, Drop::None.to_string());
            ui.selectable_value(&mut roll.drop,
                Drop::DropHighest(0), Drop::DropHighest(0).to_string());
            ui.selectable_value(&mut roll.drop,
                Drop::DropLowest(0),  Drop::DropLowest(0).to_string());
        });
    match &mut roll.drop {
        Drop::None => {},
        Drop::DropHighest(amount) => {
            ui.add(egui::Slider::new(amount, 0..=10).text("Drop amount").clamp_to_range(false));
        },
        Drop::DropLowest(amount) => {
            ui.add(egui::Slider::new(amount, 0..=10).text("Drop amount").clamp_to_range(false));
        },
    }
    ui.add(egui::Slider::new(&mut roll.min_value, -10..=10).text("Minimum value").clamp_to_range(false));
}

pub fn dice_roll_editor_simple(ui: &mut Ui, roll: &mut DiceRoll) {
    ui.add(egui::Slider::new(&mut roll.amount, 1..=10).text("Amount").clamp_to_range(false));
    ui.add(egui::Slider::new(&mut roll.sides, 1..=20).text("Sides").clamp_to_range(false));
    ui.add(egui::Slider::new(&mut roll.modifier, -10..=10).text("Modifier").clamp_to_range(false));
}

pub fn damage_roll_editor(ui: &mut Ui, roll: &mut DamageRoll) {
    ui.add(egui::Slider::new(&mut roll.amount, 1..=10).text("Amount").clamp_to_range(false));
    ui.add(egui::Slider::new(&mut roll.sides, 1..=20).text("Sides").clamp_to_range(false));
    ui.add(egui::Slider::new(&mut roll.modifier, -10..=10).text("Modifier").clamp_to_range(false));
}

pub fn back_arrow(ui: &mut Ui) -> bool {
    ui.add(Button::new("\u{e953}").frame(false)).clicked()
}

pub fn link_button(ui: &mut Ui) -> bool {
    ui.small_button("\u{e972}").clicked()
}

pub fn link_button_frameless(ui: &mut Ui) -> bool {
    ui.add(egui::Button::new("\u{e972}").small().frame(false)).clicked()
}

pub fn check_button(ui: &mut Ui) -> bool {
    ui.small_button(RichText::new("\u{ea30}").color(Color32::GREEN)).clicked()
}

pub fn x_button(ui: &mut Ui) -> bool {
    ui.small_button(RichText::new("\u{eddb}").color(Color32::RED)).clicked()
}

pub fn stat_mod_i32_button(ui: &mut Ui, stat_mod: &mut StatMod<i32>) {
    ui.menu_button("...", |ui| {
        ui.horizontal(|ui| {
            ui.strong("Modifiers");
            ui.label(RichText::new(format!("Total: {:+}", stat_mod.total())));
        });
        ui.separator();
        ui.horizontal(|ui| {
            let (id, amount) = stat_mod.new_mod_state();
            ui.add(egui::TextEdit::singleline(id).hint_text("Modifier ID..."));
            ui.add(egui::DragValue::new(amount).speed(0.1).custom_formatter(|n, _| {
                format!("{:+}", n)
            }));
            if ui.small_button(RichText::new(format!("{}", egui_phosphor::PLUS)).color(Color32::LIGHT_GREEN)).clicked() {
                stat_mod.apply_new_mod(0);
            }
        });
        ui.separator();
        let mut remove = None;
        for (id, amount) in stat_mod.view_all() {
            ui.horizontal(|ui| {
                ui.label(format!("{}: {:+}", id, amount));
                if x_button(ui) {
                    remove = Some(id.clone());
                }
            });
        }
        if let Some(id) = remove {
            stat_mod.remove(id);
        }
        ui.separator();
        ui.colored_label(Color32::LIGHT_RED, "Warning! Deleting modifiers you did not add may cause problems! Don't do it unless you know what you're doing!");
    });
}

pub fn stat_mod_percent_button(ui: &mut Ui, stat_mod: &mut StatMod<f64>) {
    ui.menu_button("...", |ui| {
        ui.horizontal(|ui| {
            ui.strong("Modifiers");
            ui.label(RichText::new(format!("Total: {:+.1}%", stat_mod.total() * 100.0)));
        });
        ui.separator();
        ui.horizontal(|ui| {
            let (id, amount) = stat_mod.new_mod_state();
            ui.add(egui::TextEdit::singleline(id).hint_text("Modifier ID..."));
            ui.add(egui::DragValue::new(amount).fixed_decimals(3).speed(0.00025).custom_formatter(|n, _| {
                format!("{:+.1}%", n * 100.0)
            }).custom_parser(|s| {
                if let Ok(n) = s.parse::<f64>() {
                    Some(n / 100.0)
                } else {
                    None
                }
            }));
            if ui.small_button(RichText::new(format!("{}", egui_phosphor::PLUS)).color(Color32::LIGHT_GREEN)).clicked() {
                stat_mod.apply_new_mod(0.0);
            }
        });
        ui.separator();
        let mut remove = None;
        for (id, amount) in stat_mod.view_all() {
            ui.horizontal(|ui| {
                ui.label(format!("{}: {:+.1}%", id, amount * 100.0));
                if x_button(ui) {
                    remove = Some(id.clone());
                }
            });
        }
        if let Some(id) = remove {
            stat_mod.remove(id);
        }
        ui.separator();
        ui.colored_label(Color32::LIGHT_RED, "Warning! Deleting modifiers you did not add may cause problems! Don't do it unless you know what you're doing!");
    });
}

#[simple_enum]
pub enum TabCallbackMode {
    AddOrFocus,
    AddOrMove,
    Remove,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone)]
pub struct ChatFlags {
    pub private: bool,
    pub dice_roll: bool,
    pub combat: bool,
    pub parties: bool,
}

impl Default for ChatFlags {
    fn default() -> Self {
        Self {
            private: false,
            dice_roll: false,
            combat: false,
            parties: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub message: String,
    pub sender: MessageSender,
    pub flags: ChatFlags,
    pub color: Color32,
    pub size: f32,
    pub italics: bool,
    pub strikethrough: bool,
    pub underline: bool,
    pub valign: Align,
}

impl ChatMessage {
    pub fn to_layout_job(&self) -> LayoutJob {
        let mut job = LayoutJob::default();
        job.append(&format!("{}{}{}{}", 
            if self.flags.private {egui_phosphor::EYE_CLOSED} else {""}, 
            if self.flags.parties {egui_phosphor::USERS_THREE} else {""}, 
            if self.flags.dice_roll {egui_phosphor::DICE_SIX} else {""}, 
            if self.flags.combat {egui_phosphor::SWORD} else {""}), 
            0.0, TextFormat {
                font_id: FontId::proportional(11.0),
                valign: Align::Center,
                ..Default::default()
            });
        job.append("|", 2.0, TextFormat {
            font_id: FontId::proportional(18.0),
            valign: Align::Center,
            ..Default::default()
        });
        job.append("", 4.0, TextFormat::default());
        match &self.sender {
            MessageSender::Server => {
                job.append("[server]: ", 0.0, TextFormat {
                    color: Color32::WHITE,
                    font_id: FontId::proportional(13.0),
                    ..Default::default()
                });
            },
            MessageSender::Player(name) => {
                job.append(&format!("<{}>: ", name), 0.0, TextFormat {
                    color: Color32::WHITE,
                    font_id: FontId::proportional(13.0),
                    ..Default::default()
                });
            },
            _ => {},
        }
        job.append(&self.message, 0.0, TextFormat {
            font_id: FontId::proportional(self.size),
            color: self.color,
            italics: self.italics,
            valign: self.valign,
            strikethrough: if self.strikethrough {Stroke::new(1.0, self.color)} else {Stroke::NONE},
            underline: if self.underline {Stroke::new(1.0, self.color)} else {Stroke::NONE},
            ..Default::default()
        });
        job
    }

    pub fn new(sender: MessageSender, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            sender,
            ..Default::default()
        }
    }

    pub fn server(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            sender: MessageSender::Server,
            ..Default::default()
        }
    }

    pub fn player(player: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            sender: MessageSender::Player(player.into()),
            ..Default::default()
        }
    }

    pub fn no_sender(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Default::default()
        }
    }

    pub fn color(mut self, color: impl Into<Color32>) -> Self {
        self.color = color.into();
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn valign(mut self, valign: Align) -> Self {
        self.valign = valign;
        self
    }

    pub fn italics(mut self) -> Self {
        self.italics = true;
        self
    }

    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    pub fn private(mut self) -> Self {
        self.flags.private = true;
        self
    }

    pub fn dice_roll(mut self) -> Self {
        self.flags.dice_roll = true;
        self
    }

    pub fn combat(mut self) -> Self {
        self.flags.combat = true;
        self
    }

    pub fn parties(mut self) -> Self {
        self.flags.parties = true;
        self
    }

    pub fn strong(self) -> Self {
        self.color(Color32::WHITE)
    }

    pub fn red(self) -> Self {
        self.color(Color32::RED)
    }

    pub fn light_red(self) -> Self {
        self.color(Color32::LIGHT_RED)
    }

    pub fn green(self) -> Self {
        self.color(Color32::GREEN)
    }

    pub fn light_green(self) -> Self {
        self.color(Color32::LIGHT_GREEN)
    }

    pub fn blue(self) -> Self {
        self.color(Color32::BLUE)
    }
}

impl Default for ChatMessage {
    fn default() -> Self {
        Self {
            message: String::new(),
            sender: MessageSender::None,
            flags: ChatFlags::default(),
            color: Color32::GRAY,
            size: 13.0,
            italics: false,
            strikethrough: false,
            underline: false,
            valign: Align::Max,
        }
    }
}

#[simple_enum(no_copy)]
pub enum MessageSender {
    Server,
    Player(String),
    None,
}

/// Adds a general-purpose registry viewer to a `Ui`.
/// ### Parameters
/// - `ui`: The `Ui` to add the viewer to.
/// - `registry`: The `Registry` being viewed.
/// - `viewed`: The current state (value/folder being viewed) of the viewer. The caller is 
/// responsible for making sure the state is persistent between frames.
/// - `display_value`: What text to display on the button that views a value (e.g. "View: {value.name}").
/// - `display_folder`: What text to display on the folder buttons. The folder's name is passed.
/// If it returns `None`, it defaults to "Folder: {folder_name}".
/// - `empty_tree_text`: What text to display inside an empty folder/tree. Defaults to "There's nothing here...".
/// - `top_right_value_callback`: When viewing a value, this is called in the top right, on the 
/// same level as the back button. By default it uses a right-to-left layout.
/// - `value_callback`: The place to display your value. Starts with a vertical layout.
/// ### Returns
/// Returns `(viewed, p, r)`, where `viewed` is simply the `viewed` parameter, possibly modified.
/// `p` and `r` are whatever is returned from `top_right_value_callback` and `value_callback`, 
/// respectively.
/// ### Example
/// ```rust
/// // in some ui
/// let (state, picked, _) = registry_viewer(
///     ui,
///     &data.enemy_registry,
///     state,
///     |enemy| format!("View: {}", enemy.name),
///     |_| None,
///     || None,
///     |ui, path, enemy| {
///         // draw a button or whatever...
///         enemy.clone()
///     },
///     |ui, path, enemy| {
///         ui.label(&enemy.name);
///         // etc...
///     },
/// );
/// // make sure to store `state` again
/// ```
pub fn registry_viewer<T, P, R>(
    ui: &mut Ui, 
    registry: &Registry<T>, 
    mut viewed: Option<String>,
    display_value: impl Fn(&T) -> WidgetText,
    display_folder: impl Fn(&String) -> Option<WidgetText> + Copy,
    empty_tree_text: impl Fn() -> Option<WidgetText>,
    top_right_value_callback: impl FnOnce(&mut Ui, &String, &T) -> P,
    value_callback: impl FnOnce(&mut Ui, &String, &T) -> R,
) -> (Option<String>, Option<P>, Option<R>) 
{
    let mut go_back = false;
    let mut callback_return: Option<R> = None;
    let mut top_right_return: Option<P> = None;
    match &mut viewed {
        Some(path) => {
            match registry.get(path) {
                Some(node) => {
                    match node {
                        RegistryNode::Value(value) => {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    if back_arrow(ui) {
                                        go_back = true;
                                    }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        top_right_return = Some(top_right_value_callback(ui, path, value));
                                    });
                                });
                                ui.separator();
                                callback_return = Some(value_callback(ui, path, value));
                            });
                        },
                        RegistryNode::SubRegistry(tree) => {
                            ui.horizontal(|ui| {
                                if back_arrow(ui) {
                                    go_back = true;
                                }
                            });
                            ui.separator();
                            if tree.is_empty() {
                                ui.label(empty_tree_text().unwrap_or(RichText::new("There's nothing here...").weak().italics().into()));
                            }
                            for (subpath, subnode) in tree {
                                match subnode {
                                    RegistryNode::Value(value) => {
                                        if ui.button(display_value(value)).clicked() {
                                            path.push('/');
                                            path.push_str(subpath);
                                        }
                                    },
                                    RegistryNode::SubRegistry(_) => {
                                        if ui.button(display_folder(subpath).unwrap_or(format!("Folder: {}", subpath).into())).clicked() {
                                            path.push('/');
                                            path.push_str(subpath);
                                        }
                                    },
                                }
                            }
                        },
                    }
                },
                None => {
                    viewed = None;
                },
            }
        },
        None => {
            if registry.tree.is_empty() {
                ui.label(empty_tree_text().unwrap_or(RichText::new("There's nothing here...").weak().italics().into()));
            }
            for (path, node) in &registry.tree {
                match node {
                    RegistryNode::Value(value) => {
                        if ui.button(display_value(value)).clicked() {
                            viewed = Some(path.clone());
                        }
                    },
                    RegistryNode::SubRegistry(_) => {
                        if ui.button(display_folder(path).unwrap_or(format!("Folder: {}", path).into())).clicked() {
                            viewed = Some(path.clone());
                        }
                    },
                }
            }
        },
    }
    if go_back {
        if let Some(path) = viewed.take() {
            viewed = path.rsplit_once('/').map(|(s, _)| s.to_owned());
        }
    }
    (viewed, top_right_return, callback_return)
}

/// Creates an enemy viewer in the given `Ui` and returns the picked enemy, or `None`.
/// ### Parameters
/// - `ui`: the `Ui` to paint in.
/// - `registry`: the enemy registry to search.
/// - `id_source`: a semi-unique hashable value, for storing the state of the viewer. It should not
/// change over time, and should be unique to all other enemy viewers.
/// ### Returns
/// `None` if nothing was picked, or `Some((id, enemy))`, where `id` is the enemy's registry path
/// and `enemy` is the `EnemyType`.
pub fn enemy_viewer_callback(ui: &mut Ui, registry: &Registry<EnemyType>, id_source: impl Hash) -> Option<(String, EnemyType)> {
    let id = Id::new(id_source).with("enemy_viewer_callback");
    let viewed = ui.ctx().data_mut(|map| {
        if let Some(viewed) = map.get_temp::<Option<String>>(id) {
            viewed
        } else {
            None
        }
    });
    let (viewed, picked, _) = registry_viewer(
        ui,
        registry,
        viewed,
        |e| format!("View: {}", e.name).into(),
        |_| None,
        || None,
        |ui, path, enemy| {
            if ui.button("Pick").clicked() {
                Some((path.clone(), enemy.clone()))
            } else {
                None
            }
        },
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
    ui.ctx().data_mut(|map| map.insert_temp(id, viewed));
    picked.flatten()
}