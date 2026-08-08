use crate::gui_common::GuiDeviceClassifications;
use crate::gui_common::{DrawEgui, GuiCmd, GuiCmdControlMatcherChange, GuiCmdControlMatcherRemove, draw_collapsing_ui};
use crate::gui_device::{GuiInDeviceCfg, draw_create_control_matcher_gui};
use crate::gui_value::GuiInInterval;
use crate::mapped_controls::{MappedCtls, MappedCtlsMidi};
use crate::midi::{AvailableMidiDeviceInfo, MIDIv1_CONTROL_RANGE, MIDIv1_PITCH_WHEEL_RANGE};
use crate::schemas_common::WithRuntimeId;
use crate::schemas_control_matcher::ControlMatchers;
use crate::schemas_midi::{
    MidiChannelCfg, MidiControlMatcherCfg, MidiMatcherCfg, MidiMessageCfg, MidiNumberCfg, MidiNumberSpecial,
};
use crate::schemas_value::WithLastKnownIO;
use strum::IntoEnumIterator;

impl<'s> DrawEgui<'s> for MappedCtlsMidi {
    type In = ();
    type Out = bool;
    fn egui(&mut self, _: Self::In, ui: &mut egui::Ui) -> Self::Out {
        ui.label(format!("MIDI control type: {:#?}", self));
        false
    }
}

impl<'s> DrawEgui<'s> for MidiMessageCfg {
    type In = ();
    type Out = bool;
    fn egui(&mut self, _: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = false;

        ui.horizontal(|ui| {
            ui.label("Type:");
            ui.label(format!("{:#?}", self.r#type));
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Channel:");
            match &mut self.channel {
                MidiChannelCfg::Any => {
                    ui.label("Any");
                    if ui.small_button("Set Number").clicked() {
                        self.channel = MidiChannelCfg::Number(0);
                        changed = true;
                    }
                }
                MidiChannelCfg::Number(n) => {
                    changed |= ui.add(egui::DragValue::new(n).range(0..=15)).changed();
                    if ui.small_button("Any").clicked() {
                        self.channel = MidiChannelCfg::Any;
                        changed = true;
                    }
                }
            }
        });

        if self.r#type != MappedCtlsMidi::PitchWheel {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Number:");
                match self.r#type {
                    MappedCtlsMidi::ControlChange => {
                        if let MidiNumberCfg::Single(n) = &mut self.number {
                            changed |= ui.add(egui::DragValue::new(n).range(0..=i8::MAX)).changed();
                        } else {
                            self.number = MidiNumberCfg::Single(0);
                            changed = true;
                        }
                    }
                    MappedCtlsMidi::Note => {
                        let cur_mode_string = match &self.number {
                            MidiNumberCfg::Special(s) => s.to_string(),
                            _ => self.number.to_string(),
                        };

                        egui::ComboBox::from_label("Mode")
                            .selected_text(&cur_mode_string)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(cur_mode_string == "Single", "Single").clicked() {
                                    self.number = MidiNumberCfg::Single(60);
                                    changed = true;
                                }
                                if ui.selectable_label(cur_mode_string == "Multiple", "Multiple").clicked() {
                                    self.number = MidiNumberCfg::Multiple(vec![60]);
                                    changed = true;
                                }
                                if ui.selectable_label(cur_mode_string == "Any", "Any").clicked() {
                                    self.number = MidiNumberCfg::Special(MidiNumberSpecial::Any);
                                    changed = true;
                                }
                            });

                        match &mut self.number {
                            MidiNumberCfg::Single(n) => {
                                changed |= ui.add(egui::DragValue::new(n).range(0..=i8::MAX)).changed();
                            }
                            MidiNumberCfg::Multiple(ns) => {
                                ui.label("Notes:");
                                let mut remove_idx = None;

                                for (i, n) in ns.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        changed |= ui.add(egui::DragValue::new(n).range(0..=i8::MAX)).changed();
                                        if ui.small_button("-").clicked() {
                                            remove_idx = Some(i);
                                        }
                                    });
                                }

                                if let Some(i) = remove_idx {
                                    ns.remove(i);
                                    changed = true;
                                }

                                ui.separator();
                                if ui.small_button("+ Add Number").clicked() {
                                    ns.push(0);
                                    changed = true;
                                }

                                ui.separator();
                                if ui.small_button("<..> Sort and dedup").clicked() {
                                    changed = true;
                                    ns.sort();
                                    ns.dedup();
                                }
                            }
                            MidiNumberCfg::Special(_) => {
                                ui.label("Matches any note number");
                            }
                        }
                    }
                    _ => {
                        if let MidiNumberCfg::Single(n) = &mut self.number {
                            changed |= ui.add(egui::DragValue::new(n).range(0..=i8::MAX)).changed();
                        }
                    }
                }
            });
        }

        changed
    }
}

impl<'s> DrawEgui<'s> for MidiControlMatcherCfg {
    type In = ();
    type Out = bool;
    fn egui(&mut self, _state: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = false;

        ui.horizontal(|ui| {
            ui.label("Control type:");
            let current_type = MappedCtls::from(self.midi_message.r#type);
            let current_type_str = current_type.to_string();

            egui::ComboBox::from_label("")
                .selected_text(&current_type_str)
                .show_ui(ui, |ui| {
                    for mct in MappedCtlsMidi::iter()
                        .filter(|c| !MappedCtls::from(*c).is_unhandled())
                        .collect::<Vec<_>>()
                    {
                        let ctrl = MappedCtls::from(mct);
                        let label = ctrl.to_string();
                        if ui.selectable_label(current_type_str == label, &label).clicked()
                            && let ControlMatchers::Midi(new_cfg) = ctrl.get_predefined_control_cfg()
                        {
                            let id = self.get_id();
                            *self = new_cfg;
                            self.id = id;
                            changed = true;
                        }
                    }
                });
        });
        ui.separator();

        ui.collapsing("MIDI Message", |ui| {
            ui.group(|ui| {
                changed |= self.midi_message.egui((), ui);
            });
        });
        ui.separator();

        if !MappedCtls::from(self.midi_message.r#type).is_button() {
            ui.horizontal(|ui| {
                changed |= self.range.egui(
                    GuiInInterval::Edit {
                        max_range: if self.midi_message.r#type == MappedCtlsMidi::PitchWheel {
                            MIDIv1_PITCH_WHEEL_RANGE
                        } else {
                            MIDIv1_CONTROL_RANGE
                        },
                        from_label: "From: ",
                        to_label: "To: ",
                        sanitize_and_sort: true,
                        truncate: true,
                    },
                    ui,
                );

                self.range = self.range.trunc().sanitize_and_sort();
            });
        }
        ui.separator();
        changed
    }
}

impl<'s> DrawEgui<'s> for MidiMatcherCfg {
    type In = GuiInDeviceCfg<'s>;
    type Out = Option<GuiCmd>;
    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        match gui_in {
            GuiInDeviceCfg::Edit { device_key } => {
                let mut changed = false;
                ui.separator();
                changed |= ui.checkbox(&mut self.enabled, "Enabled").changed();
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Device name matching regex: ");
                    let mut regex_string = self.match_name_regex.to_string();
                    if ui.text_edit_singleline(&mut regex_string).changed() {
                        if let Ok(regex) = regex::Regex::new(&regex_string) {
                            changed = true;
                            self.match_name_regex = regex;
                        } else {
                            log::error!("Failed to compile regex from string: {}", regex_string);
                        }
                    }
                });
                ui.separator();

                let mut gui_out = None;
                ui.collapsing("Controls", |ui| {
                    for (cn, c) in &mut self.controls {
                        ui.separator();
                        draw_collapsing_ui(ui, None::<()>, Some(cn), |ui| {
                            ui.separator();
                            let mut val = c.get_last_known_io();
                            ui.label(egui::RichText::new(format!("{:+09.2}", val)).monospace());
                            ui.separator();
                            ui.add(egui::Slider::new(&mut val, c.range.into()).show_value(false));
                            ui.separator();
                            if ui
                                .small_button(egui_phosphor::fill::TRASH.to_string())
                                .on_hover_text("Remove (will be removed if not referenced)")
                                .clicked()
                            {
                                gui_out = Some(GuiCmd::ControlMatcherRemove(GuiCmdControlMatcherRemove {
                                    device_key: device_key.to_string(),
                                    control_key: cn.clone(),
                                }));
                            }
                        })
                        .body(|ui| {
                            ui.group(|ui| {
                                ui.separator();
                                if c.egui((), ui) {
                                    gui_out = Some(GuiCmd::ControlMatcherChange(GuiCmdControlMatcherChange {
                                        new_cm: ControlMatchers::Midi(c.clone()),
                                    }))
                                }
                            });
                        });
                    }

                    if let Some((n, ControlMatchers::Midi(cm))) =
                        draw_create_control_matcher_gui(ui, GuiDeviceClassifications::Midi)
                        && self.controls.insert(n, cm).is_none()
                    {
                        changed = true;
                    }
                });

                ui.separator();

                if gui_out.is_some() {
                    return gui_out;
                } else if changed {
                    return Some(GuiCmd::ConfigChangeDriverRestart);
                }
                None
            }
        }
    }
}

impl<'s> DrawEgui<'s> for AvailableMidiDeviceInfo {
    type In = ();
    type Out = ();
    fn egui(&mut self, _: Self::In, ui: &mut egui::Ui) -> Self::Out {
        ui.group(|ui| {
            ui.label(format!("Device name: {}, Port index: {}", self.name, self.port_index));
        });
    }
}
