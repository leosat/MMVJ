use crate::gui_common::{DrawEgui, GuiCmd, GuiCmdControlMatcherChange, GuiCmdControlMatcherRemove, draw_collapsing_ui};
use crate::gui_device::{GuiInDeviceCfg, draw_create_control_matcher_gui};
use crate::gui_value::GuiInInterval;
use crate::hid_device::HID_AXIS_MAX_RANGE;
use crate::hid_manager::{AvailableHIDDeviceInfo, WithDeviceClassification};
use crate::hid_owned_and_ffb::{X_AXIS_IDX, Y_AXIS_IDX};
use crate::mapped_device::MappedDeviceClassification;
use crate::schemas_control_matcher::ControlMatchers;
use crate::schemas_hid::{HIDDeviceForceFeedbackCfg, HidControlMatcherCfg, HidDeviceCfg};
use crate::schemas_value::WithLastKnownIO;
use regex::Regex;
use strum::IntoEnumIterator;

impl<'s> DrawEgui<'s> for HIDDeviceForceFeedbackCfg {
    type In = ();
    type Out = bool;
    fn egui(&mut self, _state: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = false;

        ui.separator();
        changed |= ui.checkbox(&mut self.enabled, "Enabled").changed();
        ui.separator();

        // changed |= ui.checkbox(&mut self.autocenter,"Autocenter").changed();
        // changed |= ui
        //     .add(egui::Slider::new(&mut self.gain, 0.0..=1.0))
        //     .changed();
        ui.label("Max effects:");
        changed |= ui.add(egui::Slider::new(&mut self.max_effects, 1..=96)).changed();

        ui.separator();
        ui.collapsing("Effects", |ui| {
            let mut selected_effect: Option<crate::schemas_hid::HidFfEffect> = None;
            egui::ComboBox::from_label(egui_phosphor::bold::MAGIC_WAND.to_string())
                .selected_text("Add effect...")
                .show_ui(ui, |ui| {
                    for effect in crate::schemas_hid::HidFfEffect::iter() {
                        let is_already_added = self.effects.contains(&effect);
                        if ui.selectable_label(is_already_added, effect.to_string()).clicked() {
                            selected_effect = Some(effect);
                        }
                    }
                });

            if let Some(effect) = selected_effect
                && !self.effects.contains(&effect)
            {
                self.effects.push(effect);
                changed = true;
            }

            ui.separator();
            let mut idx_to_remove = None;
            for (i, effect) in self.effects.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(effect.to_string());
                    ui.add_space(4.0);
                    if ui
                        .button(egui_phosphor::bold::TRASH.to_string())
                        .on_hover_text("Remove effect")
                        .clicked()
                    {
                        idx_to_remove = Some(i);
                    }
                });
            }

            if let Some(idx) = idx_to_remove {
                self.effects.remove(idx);
                changed = true;
            }
        });

        ui.separator();
        changed
    }
}

impl<'s> DrawEgui<'s> for HidDeviceCfg {
    type In = GuiInDeviceCfg<'s>;
    type Out = Option<GuiCmd>;
    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        match gui_in {
            GuiInDeviceCfg::Edit { device_key } => {
                let mut changed = false;
                let is_virtual = self.get_classification().is_a_virtual();
                ui.separator();
                changed |= ui.checkbox(&mut self.enabled, "Enabled").changed();
                if is_virtual {
                    ui.separator();
                    changed |= ui
                        .checkbox(self.virtual_device_persistent_mut().unwrap(), "Persistent")
                        .changed();

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Long name:");
                        if ui
                            .text_edit_singleline(
                                self.virtual_device_name_mut()
                                    .expect("Virtual device matchers must have name setting"),
                            )
                            .changed()
                        {
                            changed = true;
                        };
                    });

                    if let Some(bus_cfg) = self.virtual_device_bus_info_mut() {
                        ui.separator();
                        ui.collapsing("Bus parameters", |ui| {
                            ui.separator();

                            egui::ComboBox::from_label("Bus type")
                                .selected_text(format!("{:?}", bus_cfg.r#type))
                                .show_ui(ui, |ui| {
                                    for bus_type in crate::schemas_hid::HidDeviceBusType::iter() {
                                        if ui
                                            .selectable_label(bus_cfg.r#type == bus_type, format!("{:?}", bus_type))
                                            .clicked()
                                        {
                                            bus_cfg.r#type = bus_type;
                                            changed = true;
                                        }
                                    }
                                });

                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.label("Vendor ID:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut bus_cfg.vendor_id)
                                            .range(0..=u16::MAX)
                                            .speed(1.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                };

                                ui.label(format!("(0x{:04X})", bus_cfg.vendor_id));

                                ui.label("Product ID:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut bus_cfg.product_id)
                                            .range(0..=u16::MAX)
                                            .speed(1.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                };

                                ui.label(format!("(0x{:04X})", bus_cfg.product_id));

                                ui.label("Version:");
                                if ui
                                    .add(egui::DragValue::new(&mut bus_cfg.version).range(0..=0xFFFF).speed(1.0))
                                    .changed()
                                {
                                    changed = true;
                                };

                                ui.label(format!("(0x{:04X})", bus_cfg.version));
                            });
                        });
                    }

                    if let Some(ff) = &mut self.virtual_device_force_feedback_info_mut() {
                        ui.separator();
                        ui.collapsing("Force feedback settings:", |ui| {
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.label("Cur. X force sum: ");
                                ui.label(
                                    egui::RichText::from(format!(
                                        "{:+09.4}",
                                        ff.state_xy[X_AXIS_IDX].load(std::sync::atomic::Ordering::Relaxed)
                                    ))
                                    // .color(Color32::CYAN)
                                    .monospace(),
                                );
                                ui.separator();
                                ui.label("Cur. Y force sum: ");
                                ui.label(
                                    egui::RichText::from(format!(
                                        "{:+09.4}",
                                        ff.state_xy[Y_AXIS_IDX].load(std::sync::atomic::Ordering::Relaxed)
                                    ))
                                    // .color(Color32::CYAN)
                                    .monospace(),
                                );
                                ui.separator();
                            });

                            if ff.egui((), ui) {
                                changed = true;
                            }
                        });
                    } else {
                        ui.separator();
                        if ui.button("Enable force feedback support").clicked() {
                            changed = true;
                            self.add_virtual_device_force_feedback_params();
                            self.add_special_force_feedback_controls();
                            if let Some(ff) = &mut self.virtual_device_force_feedback_info_mut() {
                                ff.effects.push(crate::schemas_hid::HidFfEffect::Constant);
                                ff.enabled = true;
                                ff.gain = 1.0;
                                ff.max_effects = 16;
                            } else {
                                unreachable!();
                            }
                        }
                    }
                } else {
                    if let Some(regex_mut) = self.matcher_name_regex_mut() {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Device name matching regex: ");
                            let mut regex_string = regex_mut.to_string();
                            if ui.text_edit_singleline(&mut regex_string).changed() {
                                if let Ok(regex) = Regex::new(&regex_string) {
                                    changed = true;
                                    *regex_mut = regex;
                                    log::info!("Compiled regex from string {regex_string}");
                                } else {
                                    log::error!("Failed to compile regex from string {regex_string}");
                                }
                            }
                        });
                    }
                }

                let mut gui_out = None;
                ui.separator();
                ui.collapsing("Controls", |ui| {
                    let mut update_classification = false;
                    for (cn, c) in &mut self.controls {
                        ui.separator();
                        draw_collapsing_ui(ui, None::<()>, Some(cn), |ui| {
                            ui.separator();
                            let mut val = c.get_last_known_io();
                            ui.label(egui::RichText::new(format!("{:+09.2}", val)).monospace());
                            ui.separator();
                            ui.add(egui::Slider::new(&mut val, c.range.into()).show_value(false));
                            ui.separator();
                            if c.r#type.is_button() || c.r#type.is_key() {
                                if val != 0.0 {
                                    ui.label(egui::RichText::new(egui_phosphor::bold::TRAY_ARROW_DOWN).size(20.0))
                                        .on_hover_text("Pressed (for any inputs values != 0)");
                                } else {
                                    ui.label(egui::RichText::new(egui_phosphor::bold::TRAY_ARROW_UP).size(20.0))
                                        .on_hover_text("Not pressed (for any inputs values != 0)");
                                }
                            } else {
                                if c.r#type.is_absolute() {
                                    ui.label(egui::RichText::new(egui_phosphor::bold::ARROWS_OUT_CARDINAL).size(20.0))
                                        .on_hover_text("Absolute axis");
                                } else {
                                    ui.label(
                                        egui::RichText::new(if val <= 0.0 {
                                            egui_phosphor::bold::ARROWS_COUNTER_CLOCKWISE
                                        } else {
                                            egui_phosphor::bold::ARROWS_CLOCKWISE
                                        })
                                        .size(20.0),
                                    )
                                    .on_hover_text("Relative movement");
                                }
                            }
                            ui.separator();
                            if ui
                                .small_button(egui_phosphor::fill::TRASH.to_string())
                                .on_hover_text("Remove control (will be removed if not referenced)")
                                .clicked()
                            {
                                update_classification = true;
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
                                    update_classification = true;
                                    gui_out = Some(GuiCmd::ControlMatcherChange(GuiCmdControlMatcherChange {
                                        new_cm: ControlMatchers::Hid(c.clone()),
                                    }));
                                }
                            });
                        });
                    }

                    if let Some((n, ControlMatchers::Hid(cm))) =
                        draw_create_control_matcher_gui(ui, MappedDeviceClassification::Hid(self.get_classification()))
                        && self.controls.insert(n, cm).is_none()
                    {
                        update_classification = true;
                        changed = true;
                    }

                    if update_classification {
                        self.update_classification();
                    }
                });

                ui.separator();

                if gui_out.is_some() {
                    gui_out
                } else if changed {
                    Some(GuiCmd::ConfigChangeDriverRestart)
                } else {
                    None
                }
            }
        }
    }
}

impl<'s> DrawEgui<'s> for HidControlMatcherCfg {
    type In = ();
    type Out = bool;
    fn egui(&mut self, _state: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = false;

        ui.horizontal(|ui| {
            ui.label("Control type: ");
            ui.label(self.r#type.to_string());
        });

        if !self.r#type.is_button() {
            ui.separator();
            ui.horizontal(|ui| {
                changed |= self.range.egui(
                    GuiInInterval::Edit {
                        max_range: HID_AXIS_MAX_RANGE,
                        from_label: "From: ",
                        to_label: "To: ",
                        sanitize_and_sort: true,
                        truncate: true,
                    },
                    ui,
                );

                ui.label("Initial value:");
                self.initial_value = self.initial_value.trunc().floor();
                let range = self.get_interval().make_range_inclusive();
                changed |= ui.add(egui::Slider::new(&mut self.initial_value, range)).changed();
            });
            if let Some(p) = self.properties.as_mut() {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.disable(); // TODO: not applied in the engine.
                    ui.label("Flat");
                    changed |= ui.add(egui::Slider::new(&mut p.flat, HID_AXIS_MAX_RANGE)).changed();
                    ui.separator();
                    ui.label("Fuzz");
                    changed |= ui.add(egui::Slider::new(&mut p.fuzz, HID_AXIS_MAX_RANGE)).changed();
                    ui.separator();
                    ui.label("Resolution");
                    changed |= ui
                        .add(egui::Slider::new(&mut p.resolution, HID_AXIS_MAX_RANGE))
                        .changed();
                });
            }
        }
        changed
    }
}

impl<'s> DrawEgui<'s> for AvailableHIDDeviceInfo {
    type In = ();
    type Out = ();
    fn egui(&mut self, _gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        ui.group(|ui| {
            ui.separator();
            ui.label(format!("Device name: {}", self.name));
            ui.separator();
            ui.label(format!("Classification: {}", self.classification));
            ui.separator();
            ui.label(format!("Path: {}", self.path.to_string_lossy()));
        });
    }
}
