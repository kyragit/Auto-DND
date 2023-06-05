use crate::{dm_app::DMAppData, player_app::PlayerAppData, dice::{DiceRoll, ModifierType, Drop}};
use eframe::egui::{self, WidgetText, InnerResponse, Ui, RichText};

pub trait CommonApp<'d> {
    fn get_data(&'d mut self) -> CommonAppData<'d>;
    fn get_window_state(&mut self, name: String) -> &mut bool;
    fn set_window_state(&mut self, name: String, value: bool);

    fn toggle_window_state(&mut self, name: impl Into<String>) {
        let open = self.get_window_state(name.into());
        *open = !*open;
    }

    /// Creates a window that is opened with the `open_key`. Returns the window's `Response`. 
    /// 
    /// ### Example:
    /// ```rust
    /// // ...
    /// data.create_window(ctx, "My Window", "my_window_open".to_owned(), |window| {
    ///     window.resizable(true).vscroll(true)
    /// }, |ui, data| {
    ///     // some widgets...
    /// });
    /// // ...
    /// ```
    fn create_window<R>(&mut self, ctx: &egui::Context, title: impl Into<WidgetText>, open_key: String, 
        options: impl for<'a> FnOnce(egui::Window<'a>) -> egui::Window<'a>, 
        func: impl FnOnce(&mut Ui, &mut Self) -> R)
        -> Option<InnerResponse<Option<R>>> {
        let open = self.get_window_state(open_key.clone());
        let mut temp_open = open.clone();
        let response = options(egui::Window::new(title)).open(&mut temp_open).show(ctx, |ui| func(ui, self));
        self.set_window_state(open_key, temp_open);
        response
    }
}

impl<'a, 'd: 'a> CommonApp<'d> for DMAppData {
    fn get_data(&'d mut self) -> CommonAppData<'d> {
        CommonAppData::DM(self)
    }
    fn get_window_state(&mut self, name: String) -> &mut bool {
        self.temp_state.window_states.entry(name).or_insert(false)
    }
    fn set_window_state(&mut self, name: String, value: bool) {
        self.temp_state.window_states.insert(name, value);
    }
}

impl<'d> CommonApp<'d> for PlayerAppData {
    fn get_data(&'d mut self) -> CommonAppData<'d> {
        CommonAppData::Player(self)
    }
    fn get_window_state(&mut self, name: String) -> &mut bool {
        self.window_states.entry(name).or_insert(false)
    }
    fn set_window_state(&mut self, name: String, value: bool) {
        self.window_states.insert(name, value);
    }
}

pub enum CommonAppData<'d> {
    DM(&'d mut DMAppData),
    Player(&'d mut PlayerAppData),
}

/// Adds a tab bar to a `Ui`. 
/// 
/// ## Parameters
/// - `state`: the stored state of the tab. It will be mutated when the tabs are clicked.
/// - `unique_id`: a string representing this tab bar uniquely. For technical reasons, it is 
/// important that this is globally unique.
/// - `ui`: the `Ui` to add the tab bar to.
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
pub fn tabs<R, T: TabValue, F: FnOnce(&mut Ui, T) -> R>(state: &mut T, unique_id: String, ui: &mut Ui, func: F) -> R {
    egui::TopBottomPanel::top(unique_id).show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            for tab in T::iterate() {
                ui.selectable_value(state, tab, RichText::new(tab.display()).background_color(ui.style().visuals.faint_bg_color));
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
    Spells,
    Notes,
}

impl TabValue for CharacterSheetTab {
    fn iterate() -> Vec<Self> {
        vec![
            Self::Stats,
            Self::Class,
            Self::Inventory,
            Self::Spells,
            Self::Notes,
        ]
    }
    fn display(&self) -> String {
        match self {
            Self::Stats => "Stats",
            Self::Class => "Class",
            Self::Inventory => "Inventory",
            Self::Spells => "Spells",
            Self::Notes => "Notes",
        }.to_owned()
    }
}

pub fn display_i32(i: i32) -> String {
    if i < 0 {
        format!("{}", i)
    } else {
        format!("+{}", i)
    }
}

pub fn display_percent(x: f32) -> String {
    if x < 0.0 {
        format!("{}%", x * 100.0)
    } else {
        format!("+{}%", x * 100.0)
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