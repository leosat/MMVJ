use crate::schemas_control_matcher::ControlMatchers;
use crate::{mapped_device::MappedDeviceClassification, schemas_predefined::HidControlPredefined};
use eframe::egui;

// --------------------------------------------------------

pub(crate) fn draw_create_control_matcher_gui(
    ui: &mut egui::Ui,
    device_type: MappedDeviceClassification,
) -> Option<(String, ControlMatchers)> {
    let is_win_opened_egui_id = ui.make_persistent_id("new control");
    let is_win_opened = &mut ui.data_mut(|d| d.get_temp::<bool>(is_win_opened_egui_id).unwrap_or(false));
    ui.separator();
    if ui.small_button("add control").clicked() {
        *is_win_opened = true;
    }
    let mut control_to_add_opt: Option<(String, ControlMatchers)> = None;
    egui::Window::new("Select predefined control matcher to add...")
        .id(is_win_opened_egui_id)
        .open(is_win_opened)
        .resizable(true)
        .order(egui::Order::TOP)
        .show(ui.ctx(), |ui| {
            ui.separator();
            match device_type {
                #[cfg(feature = "midi")]
                MappedDeviceClassification::Midi => {
                    for c in &crate::config::PREDEF_CONTROLS.midi_controls {
                        ui.separator();
                        if ui.button(c.0).clicked() {
                            control_to_add_opt = Some((c.0.to_string(), ControlMatchers::Midi(c.1.clone().into())));
                        }
                    }
                }
                MappedDeviceClassification::Hid(classif_filter) => {
                    ui.label(format!("Current device matcher classification is: {}", classif_filter));
                    ui.separator();

                    #[allow(clippy::type_complexity)]
                    let controls: [(&str, fn((&String, &HidControlPredefined)) -> bool); _] = [
                        ("\"Joystick\"", |cm| cm.1.r#type.is_a_joystick_control()),
                        ("\"Gamepad\"", |cm| cm.1.r#type.is_a_gamepad_control()),
                        ("\"Mouse\"", |cm| cm.1.r#type.is_a_mouse_control()),
                        ("\"Keyboard\"", |cm| cm.1.r#type.is_a_keyboard_control()),
                        ("Misc", |cm| cm.1.r#type.is_a_misc_control()),
                    ];

                    for (title, predicate) in controls {
                        ui.separator();
                        ui.collapsing(title, |ui| {
                            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                                crate::config::PREDEF_CONTROLS
                                    .hid_controls
                                    .iter()
                                    .filter(|args| predicate(*args))
                                    .for_each(|(cm_key, cm)| {
                                        ui.separator();
                                        if ui.button(cm_key).clicked() {
                                            control_to_add_opt =
                                                Some((cm_key.to_string(), ControlMatchers::Hid(cm.clone().into())));
                                        }
                                    });
                            });
                        });
                    }
                }
                MappedDeviceClassification::Unsupported => {}
            }
            ui.separator();
        });
    ui.data_mut(|d| d.insert_temp(is_win_opened_egui_id, *is_win_opened));
    control_to_add_opt
}

pub(crate) enum GuiInDeviceCfg<'s> {
    Edit { device_key: &'s str },
}
