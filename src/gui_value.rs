use std::ops::RangeInclusive;

use eframe::egui;

use crate::relativity::Relativity;

use crate::{
    base_num::BaseNumT,
    gui_common::{DrawEgui, GuiInKinds},
    gui_mapping::ValueRefChoiceContext,
    hid_device::HID_AXIS_MAX_RANGE,
    hid_manager::WithDeviceClassification,
    num_interval::NumInterval,
    schemas_cfg::{DevicesCfgNew, VariablesCfg},
    schemas_control_matcher::ControlMatchers,
    schemas_hid::HidDeviceCfg,
    schemas_transform::AutoOrManual,
    schemas_value::{
        DeviceControlMatcherRef, DynValueRefs, ValueDsts, ValueSrcs, ValuesRt, VariableState, WithLastKnownIO,
        WithNumInterval, WithNumIntervalMut, WithNumericValue, WithRelativity,
    },
};

#[derive(Clone, Copy)]
pub(crate) struct GuiInValueEditParams<'s> {
    pub(crate) allow_interval_edit: bool,
    pub(crate) slider_log_scale: bool,
    pub(crate) cfg_variables: &'s VariablesCfg,
    pub(crate) cfg_devices: &'s DevicesCfgNew,
}

#[derive(Clone, Copy)]
pub(crate) enum GuiInValue<'s> {
    Edit(GuiInValueEditParams<'s>),
    Display,
}

// -------------------------------------

impl<'s> DrawEgui<'s> for DynValueRefs {
    type In = (&'s str, &'s str, ValueRefChoiceContext, GuiInValue<'s>);
    type Out = bool;

    fn egui(&mut self, _gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        match self {
            DynValueRefs::DeviceControlMatcher(d) => {
                ui.label(
                    egui::RichText::new(format!(
                        "dev: {} / ctl: {} (range: {}, {:?}, {:08.2})",
                        d.device_matcher_key,
                        d.control_key,
                        d.control_matcher.get_interval(),
                        d.control_matcher.get_relativity(),
                        d.control_matcher.get_last_known_io(),
                    ))
                    .size(14.0)
                    .monospace()
                    .strong(),
                );
                false
            }
            DynValueRefs::Variable(v) => {
                ui.label(
                    egui::RichText::new(format!(
                        "var: {} (range: {}, {:?}, {:08.2})",
                        v.variable_key,
                        v.variable.get_interval(),
                        v.variable.get_relativity(),
                        v.variable.get_numeric_value()
                    ))
                    .size(14.0)
                    .monospace()
                    .strong(),
                );
                false
            }
        }
    }
}

impl<'s> DrawEgui<'s> for ValueDsts {
    type In = (&'s str, &'s str, ValueRefChoiceContext, GuiInValue<'s>);
    type Out = bool;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = false;
        match gui_in.3 {
            GuiInValue::Edit(params) => {
                if let Some(ValuesRt::Dst(dst)) = draw_value_choice_iface(
                    gui_in.2,
                    ui,
                    gui_in.0,
                    gui_in.1,
                    params.cfg_devices,
                    params.cfg_variables,
                ) {
                    *self = dst;
                    changed |= true;
                }
                ui.separator();
            }
            GuiInValue::Display => {}
        }
        match self {
            ValueDsts::Void => {
                ui.label(egui::RichText::new("Void").size(14.0).monospace().strong());
            }
            ValueDsts::Dynamic(dynamic_value_refs_rt) => {
                dynamic_value_refs_rt.egui(gui_in, ui);
            }
        }
        changed
    }
}

// ----------------

pub(crate) enum GuiInInterval<'s> {
    Edit {
        max_range: RangeInclusive<BaseNumT>,
        from_label: &'s str,
        to_label: &'s str,
        sanitize_and_sort: bool,
        truncate: bool,
    },
    Display,
}

impl<'s> DrawEgui<'s> for NumInterval<BaseNumT> {
    type In = GuiInInterval<'s>;
    type Out = bool;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = false;
        match gui_in {
            GuiInInterval::Edit {
                max_range,
                from_label,
                to_label,
                sanitize_and_sort,
                truncate,
            } => {
                ui.label(from_label);
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut self.from)
                            .range(max_range.clone())
                            .fixed_decimals(2),
                    )
                    .changed();
                ui.separator();
                ui.label(to_label);
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut self.to)
                            .range(max_range.clone())
                            .fixed_decimals(2),
                    )
                    .changed();

                if sanitize_and_sort {
                    *self = self.sanitize_and_sort();
                }

                if truncate {
                    *self = self.trunc();
                }
            }
            GuiInInterval::Display => {
                ui.label(format!("Range: {}", self));
            }
        };
        changed
    }
}

// -----------------------------
impl<'s> DrawEgui<'s> for ValueSrcs {
    type In = (&'s str, &'s str, ValueRefChoiceContext, GuiInValue<'s>);
    type Out = bool;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let gui_type = gui_in.3;
        match &gui_type {
            GuiInValue::Edit(params) => {
                let mut changed = false;
                if let Some(ValuesRt::Src(new_value_src)) = draw_value_choice_iface(
                    gui_in.2,
                    ui,
                    gui_in.0,
                    gui_in.1,
                    params.cfg_devices,
                    params.cfg_variables,
                ) {
                    *self = new_value_src;
                    changed |= true;
                }
                ui.separator();
                match self {
                    Self::Static(s) => {
                        ui.label("Value:");
                        changed |= ui
                            .add(
                                egui::Slider::new(&mut s.value, s.interval.make_range_inclusive())
                                    .logarithmic(params.slider_log_scale)
                                    .fixed_decimals(4)
                                    .step_by(0.0001),
                            )
                            .changed();
                        // --
                        ui.separator();
                        ui.label("Range: ");
                        if params.allow_interval_edit {
                            changed |= s.interval.egui(
                                GuiInInterval::Edit {
                                    max_range: HID_AXIS_MAX_RANGE,
                                    from_label: "From: ",
                                    to_label: "To: ",
                                    sanitize_and_sort: true,
                                    truncate: false,
                                },
                                ui,
                            );
                            ui.label("");
                        } else {
                            ui.label(format!("{}", s.interval));
                        }
                        if changed {
                            s.value = s.interval.clamp(s.value);
                        }
                        changed
                    }
                    Self::Dynamic(d) => d.egui(gui_in, ui),
                };
                changed
            }
            GuiInValue::Display => {
                ui.separator();
                match self {
                    Self::Static(v) => {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("Value: {}, Range: {}", v.value, v.interval))
                                    .size(14.0)
                                    .monospace()
                                    .strong(),
                            );
                        });
                    }
                    Self::Dynamic(dynamic_value_ref_rt) => {
                        dynamic_value_ref_rt.egui(gui_in, ui);
                    }
                };
                false
            }
        }
    }
}

// -------------------------------------
impl<'s> DrawEgui<'s> for Relativity {
    type In = GuiInKinds;
    type Out = bool;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = false;
        match gui_in {
            GuiInKinds::Edit => {
                changed |= ui.selectable_value(self, Self::Abs, "Abs").changed();
                changed |= ui.selectable_value(self, Self::Rel, "Rel").changed();
            }
            GuiInKinds::Display => {
                ui.label(format!("{self:?}"));
            }
        };
        changed
    }
}

impl<'s> DrawEgui<'s> for VariableState {
    type In = GuiInKinds;
    type Out = bool;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = false;
        match gui_in {
            GuiInKinds::Edit => {
                ui.separator();
                self.get_relativity().egui(GuiInKinds::Display, ui);
                ui.separator();
                ui.label("Range: ");
                changed |= self.interval_mut().egui(
                    GuiInInterval::Edit {
                        max_range: HID_AXIS_MAX_RANGE,
                        from_label: "From: ",
                        to_label: "To: ",
                        sanitize_and_sort: true,
                        truncate: false,
                    },
                    ui,
                );

                ui.separator();
                ui.label(
                    egui::RichText::new(format!(" Value: {}", self.get_numeric_value(),))
                        .monospace()
                        .strong(),
                );
                // ui.group(|ui| {
                // });
            }
            GuiInKinds::Display => {
                ui.horizontal(|ui| {
                    self.get_interval().egui(GuiInInterval::Display, ui);
                    ui.separator();
                    self.get_relativity().egui(GuiInKinds::Display, ui);
                    ui.separator();
                    let mut value = self.get_numeric_value();
                    match self.value {
                        crate::schemas_transform::AutoOrManual::Manual(_) => {
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut value, self.get_interval().make_range_inclusive())
                                        .logarithmic(false) // TODO: make configurable
                                        .fixed_decimals(4)
                                        .step_by(0.0001),
                                )
                                .changed();
                            if changed {
                                self.value.store(value, std::sync::atomic::Ordering::Relaxed);
                                // TODO: WithNumericValueSet
                            }
                            ui.separator();
                            if ui
                                .button(egui_phosphor::bold::ROBOT.to_string())
                                .on_hover_text(
                                    "Convert to runtime variable (if clicked, will NOT be saved or restored from config)",
                                )
                                .clicked()
                            {
                                self.value = AutoOrManual::Auto((*self.value).clone());
                                changed = true;
                            }
                        }
                        crate::schemas_transform::AutoOrManual::Auto(_) => {
                            ui.label(egui::RichText::new(format!("{value:+11.4}")).monospace().strong());
                            ui.separator();
                            if ui
                                .button(egui_phosphor::bold::HAND_TAP.to_string())
                                .on_hover_text(
                                    "Convert to manually-set param (if clicked, WILL be saved and restored from config) \
                                    (WARNING: it can still be dynamically overridden from mappings)",
                                )
                                .clicked()
                            {
                                self.value = AutoOrManual::Manual((*self.value).clone());
                                changed = true;
                            }
                        }
                    };
                });
            }
        }
        changed
    }
}

// -------------------------------------

pub(crate) fn draw_value_choice_iface_window(
    choice_context: &ValueRefChoiceContext,
    ui: &mut egui::Ui,
    cfg_devices: &DevicesCfgNew,
    cfg_variables: &VariablesCfg,
) -> Option<DynValueRefs> {
    let mut gui_out = None;
    let gui_out_mut = &mut gui_out;
    let (allow_special_ffb, allow_joysticks_or_gamepads, allow_midi, allow_mice_or_kbd, allow_vars) =
        match choice_context {
            ValueRefChoiceContext::MappingSrc => (true, true, true, true, true),
            ValueRefChoiceContext::MappingDst => (false, true, false, true, true),
            ValueRefChoiceContext::TfmStepAuxSrc => (true, true, true, true, true),
            ValueRefChoiceContext::TfmStepAuxDst => (false, true, false, true, true),
        };

    if allow_vars {
        ui.separator();
        egui::CollapsingHeader::new("Variables").show(ui, |ui| {
            for v in cfg_variables {
                ui.separator();
                if ui
                    .add(egui::Button::new(format!(" Variable {}", v.0)))
                    .on_hover_text(serde_saphyr::to_string(&v).expect("Can't serialize variable"))
                    .clicked()
                {
                    *gui_out_mut = Some(DynValueRefs::Variable(crate::schemas_value::VariableRef {
                        variable_key: v.0.clone(),
                        variable: v.1.clone(),
                    }));
                };
            }
        });
    }
    ui.separator();
    egui::CollapsingHeader::new("Device control matchers").show(ui, |ui| {
        ui.separator();
        ui.label("Hint: hover to see control details, click to select.");
        ui.separator();

        fn choose_hid(
            ui: &mut egui::Ui,
            filter: impl FnMut(&(&String, &HidDeviceCfg)) -> bool,
            cfg_devices: &DevicesCfgNew,
        ) -> Option<DynValueRefs> {
            for dm in cfg_devices.hid.iter().filter(filter) {
                if let Some(choice) = egui::CollapsingHeader::new(dm.0)
                    .show(ui, |ui| {
                        for cm in &dm.1.controls {
                            ui.separator();
                            if ui
                                .add(egui::Button::new(format!("{}: ({})", cm.0, cm.0,)))
                                .on_hover_text(serde_saphyr::to_string(&cm.1).expect("Can't serialize device control"))
                                .clicked()
                            {
                                return Some(DynValueRefs::DeviceControlMatcher(DeviceControlMatcherRef {
                                    device_matcher_key: dm.0.to_string(),
                                    control_key: cm.0.to_string(),
                                    control_matcher: ControlMatchers::Hid(cm.1.clone()),
                                }));
                            };
                        }
                        None
                    })
                    .body_returned
                    .unwrap_or_default()
                {
                    return Some(choice);
                };
            }
            None
        }

        if allow_joysticks_or_gamepads || allow_special_ffb {
            ui.separator();
            *gui_out_mut =
                gui_out_mut
                    .clone()
                    .or(egui::CollapsingHeader::new("Joysticks or gamepads control matchers")
                        .show(ui, |ui| {
                            choose_hid(
                                ui,
                                |dm: &(&String, &HidDeviceCfg)| dm.1.is_a_joystick() || dm.1.is_a_gamepad(),
                                cfg_devices,
                            )
                        })
                        .body_returned
                        .unwrap_or_default());
        }

        if allow_mice_or_kbd {
            ui.separator();
            *gui_out_mut = gui_out_mut
                .clone()
                .or(egui::CollapsingHeader::new("Mice or keyboard control matchers")
                    .show(ui, |ui| {
                        choose_hid(
                            ui,
                            |dm: &(&String, &HidDeviceCfg)| dm.1.is_a_mouse() || dm.1.is_a_keyboard(),
                            cfg_devices,
                        )
                    })
                    .body_returned
                    .unwrap_or_default());
        }

        #[cfg(feature = "midi")]
        if allow_midi {
            ui.separator();
            egui::CollapsingHeader::new("MIDI control matchers").show(ui, |ui| {
                ui.separator();
                for d in &cfg_devices.midi {
                    egui::CollapsingHeader::new(d.0).show(ui, |ui| {
                        for c in &d.1.controls {
                            ui.separator();
                            if ui
                                .add(egui::Button::new(format!("{}: ({})", c.0, c.0,)))
                                .on_hover_text(serde_saphyr::to_string(&c.1).expect("Can't serialize device control"))
                                .clicked()
                            {
                                *gui_out_mut = gui_out_mut.clone().or(Some(DynValueRefs::DeviceControlMatcher(
                                    DeviceControlMatcherRef {
                                        device_matcher_key: d.0.to_string(),
                                        control_key: c.0.to_string(),
                                        control_matcher: ControlMatchers::Midi(c.1.clone()),
                                    },
                                )));
                            };
                        }
                    });
                }
            });
        }
    });
    gui_out
}

pub(crate) fn draw_value_choice_iface(
    choice_context: ValueRefChoiceContext,
    ui: &mut egui::Ui,
    egui_id_hashable: &str,
    window_title: &str,
    cfg_devices: &DevicesCfgNew,
    cfg_variables: &VariablesCfg,
) -> Option<ValuesRt> {
    let mut choice = None;
    ui.scope_builder(egui::UiBuilder::default(), |ui| {
        let egui_id_window_open = ui.auto_id_with(egui_id_hashable);
        let mut choose_ctl_window_opened = ui.data_mut(|d| d.get_temp(egui_id_window_open).unwrap_or(false));
        if choose_ctl_window_opened {
            ui.label("selecting...");
        } else if ui
            .button(egui_phosphor::regular::LIST_MAGNIFYING_GLASS.to_string())
            .on_hover_text(match choice_context {
                ValueRefChoiceContext::MappingSrc => "Select main src",
                ValueRefChoiceContext::MappingDst => "Select main dst",
                ValueRefChoiceContext::TfmStepAuxSrc => "Select src",
                ValueRefChoiceContext::TfmStepAuxDst => "Select dst",
            })
            .clicked()
        {
            choose_ctl_window_opened = true;
        }
        if choose_ctl_window_opened {
            choice = egui::Window::new(window_title)
                .open(&mut choose_ctl_window_opened)
                .id(egui_id_window_open)
                .scroll([true, true])
                .show(ui.ctx(), |ui| {
                    let mut static_value = None;
                    if choice_context.is_dst() {
                        ui.separator();
                        ui.collapsing("Void", |ui| {
                            ui.separator();
                            if ui.button("Void").clicked() {
                                static_value = Some(ValuesRt::Dst(ValueDsts::Void));
                            }
                        });
                    } else {
                        ui.separator();
                        ui.collapsing("Static", |ui| {
                            ui.separator();
                            if ui.button("Local static value").clicked() {
                                static_value = Some(ValuesRt::Src(ValueSrcs::Static(Default::default())));
                            }
                        });
                    }

                    if static_value.is_some() {
                        return static_value;
                    }

                    if let Some(dynamic) =
                        draw_value_choice_iface_window(&choice_context, ui, cfg_devices, cfg_variables)
                    {
                        return match choice_context {
                            ValueRefChoiceContext::TfmStepAuxSrc | ValueRefChoiceContext::MappingSrc => {
                                Some(ValuesRt::Src(ValueSrcs::Dynamic(dynamic)))
                            }
                            ValueRefChoiceContext::MappingDst => Some(ValuesRt::Dst(ValueDsts::Dynamic(dynamic))),
                            ValueRefChoiceContext::TfmStepAuxDst => Some(ValuesRt::Dst(ValueDsts::Dynamic(dynamic))),
                        };
                    }
                    None
                })
                .unwrap()
                .inner
                .flatten();
            // dbg!(&choice);
            if choice.is_some() {
                choose_ctl_window_opened = false;
            }
        }
        ui.data_mut(move |d| {
            d.insert_temp(egui_id_window_open, choose_ctl_window_opened);
        });
    });
    choice
}
