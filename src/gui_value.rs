use std::ops::RangeInclusive;

use eframe::egui;

use crate::config::WithSelfSanitize;
use crate::gui_common::{GuiCmd, bool_to_simple_change_gui_cmd, draw_collapsing_ui};
use crate::gui_transform_step::GuiInCommon;
use crate::mapping::MappingEngineCmd;
use crate::relativity::Relativity;

use crate::schemas_value::ValueIface;
use crate::schemas_value::{AutoOrManual, ValueXrcs, WithNumIntervalSettable};

use crate::schemas_value_port::{
    PortRemapPolicy, PortSanPolicy, SanPolicyNone, SanPolicyUseFromPortInner, TfmPolicyDefaultChoice, TfmPolicyIface,
    ValuePort, ValuePortIface, WithNumericValueSanitizerStatic,
};

use crate::{
    base_num::BaseNumT,
    device_and_device_manager::WithDeviceClassification,
    gui_common::{DrawEgui, GuiInKinds},
    gui_mapping::ValueUsageContext,
    hid_device::HID_AXIS_MAX_RANGE,
    num_interval::NumInterval,
    schemas_cfg::{DevicesCfgNew, VariablesCfg},
    schemas_control_matcher::ControlMatchers,
    schemas_hid::HidDeviceCfg,
    schemas_value::{
        DeviceControlMatcherRef, DynValueRefs, ValueDsts, ValueSrcs, ValueTargets, VariableState, WithLastKnownIO,
        WithNumInterval, WithNumIntervalMut, WithNumericValue, WithRelativity,
    },
};

#[derive(Clone, Copy)]
pub(crate) struct GuiInValueEditParams<'s> {
    pub(crate) name: &'s str,
    pub(crate) choice_case: Option<ValueUsageContext>,
    pub(crate) allow_interval_edit: bool,
    pub(crate) slider_log_scale: bool,
    pub(crate) gui_common_ctx: &'s GuiInCommon<'s>,
}

#[derive(Clone, Copy)]
pub(crate) enum GuiInValue<'s> {
    Edit(GuiInValueEditParams<'s>),
    Display {
        #[allow(unused)]
        usage_context: ValueUsageContext,
    },
}

// -------------------------------------

impl<'s> DrawEgui<'s> for DynValueRefs {
    type In = GuiInValue<'s>;
    type Out = bool;

    fn egui(&mut self, _gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        match self {
            DynValueRefs::DeviceControlMatcher(d) => {
                ui.label(
                    egui::RichText::new(format!(
                        "dev: {} / ctl: {} (range: {}, {:?}, {:08.2})",
                        d.device_matcher_key,
                        d.control_matcher_key,
                        d.control_matcher.get_interval(),
                        d.control_matcher.get_relativity(),
                        d.control_matcher.get_last_known_io(),
                    ))
                    .size(12.0)
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
                    .size(12.0)
                    .monospace()
                    .strong(),
                );
                false
            }
        }
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
                if !from_label.is_empty() {
                    ui.label(from_label);
                }
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut self.from)
                            .range(max_range.clone())
                            .fixed_decimals(2),
                    )
                    .changed();
                ui.separator();
                if !to_label.is_empty() {
                    ui.label(to_label);
                }
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

impl<'s, PortInnerT, SanPolicyT, RemapT, TfmT> DrawEgui<'s> for ValuePort<PortInnerT, SanPolicyT, RemapT, TfmT>
where
    <PortInnerT as WithNumericValue>::ValueT: eframe::emath::Numeric,
    RemapT: PortRemapPolicy<PortInnerT::ValueT>,
    TfmT: TfmPolicyIface,
    SanPolicyT: PortSanPolicy<PortInnerT> + GuiSanInfo<PortInnerT>,
    PortInnerT: ValueIface + DrawEgui<'s, In = GuiInValue<'s>, Out = Option<GuiCmd>> + TfmPolicyDefaultChoice,
    NumInterval<<PortInnerT as WithNumericValue>::ValueT>: DrawEgui<'s, In = GuiInInterval<'s>, Out = bool>,
    BaseNumT: From<<PortInnerT as WithNumericValue>::ValueT>,
    <PortInnerT as WithNumericValue>::ValueT: From<BaseNumT>,
{
    type In = GuiInValue<'s>;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let gui_out = draw_egui_for_port::<SanPolicyT, Self, PortInnerT>(self, gui_in, ui);
        if gui_out.is_some() {
            self.sanitize_inplace(());
        }
        gui_out
    }
}

fn get_new_value_target<'s>(gui_in: GuiInValue<'s>, ui: &mut egui::Ui) -> Option<ValueTargets> {
    if let GuiInValue::Edit(params) = gui_in
        && let Some(choice_case) = params.choice_case
        && let Some(target) = draw_value_choice_iface(
            choice_case,
            ui,
            params.name,
            params.name,
            params.gui_common_ctx.cfg_devices(),
            params.gui_common_ctx.cfg_variables(),
        )
    {
        target.into()
    } else {
        None
    }
}

trait GuiSanInfo<T: WithNumericValueSanitizerStatic> {
    fn draw_san_info<PortInnerT: WithNumericValueSanitizerStatic>(ui: &mut egui::Ui);
}

impl<T: WithNumericValueSanitizerStatic> GuiSanInfo<T> for SanPolicyUseFromPortInner {
    fn draw_san_info<PortInnerT: WithNumericValueSanitizerStatic>(ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(egui_phosphor::bold::BROOM).size(14.0))
            .on_hover_text(PortInnerT::get_value_sanitizer_policy_doc_str());
    }
}

impl<T: WithNumericValueSanitizerStatic> GuiSanInfo<T> for SanPolicyNone {
    fn draw_san_info<PortInnerT: WithNumericValueSanitizerStatic>(ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(egui_phosphor::bold::FUNNEL_X).size(14.0))
            .on_hover_text(<Self as PortSanPolicy<T>>::san_policy_doc_str());
    }
}

fn draw_egui_for_port<'s, SanPolicyT, PortT, PortInnerT>(
    port: &mut PortT,
    mut gui_in: GuiInValue<'s>,
    ui: &mut egui::Ui,
) -> Option<GuiCmd>
where
    PortT: ValuePortIface,
    PortInnerT: ValueIface,
    SanPolicyT: GuiSanInfo<PortInnerT>,
    <PortT as ValuePortIface>::InnerT: ValueIface + DrawEgui<'s, In = GuiInValue<'s>, Out = Option<GuiCmd>>,
    NumInterval<<<PortT as ValuePortIface>::InnerT as WithNumericValue>::ValueT>:
        DrawEgui<'s, In = GuiInInterval<'s>, Out = bool>,
    PortInnerT::ValueT: eframe::emath::Numeric,
    BaseNumT: From<<<PortT as ValuePortIface>::InnerT as WithNumericValue>::ValueT>,
    <<PortT as ValuePortIface>::InnerT as WithNumericValue>::ValueT: From<BaseNumT>,
{
    match gui_in {
        GuiInValue::Edit(params) => {
            if let Some(new_target) = get_new_value_target(gui_in, ui) {
                *port.port_inner_mut() = new_target.into();
                return Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter));
            }

            if let GuiInValue::Edit(ref mut params) = gui_in {
                params.choice_case = None
            }

            if port.port_inner_ref().value_is_static() {
                let mut gui_out = ui.horizontal(|ui| port.port_inner_mut().egui(gui_in, ui)).inner;

                let default_interval = port.port_get_default_interval_from_inner().cast().unwrap();

                if default_interval != port.port_inner_ref().get_interval()
                    && ui.button(format!("reset to {default_interval}",)).clicked()
                {
                    port.port_inner_mut().set_interval(default_interval);
                    gui_out = bool_to_simple_change_gui_cmd(true);
                }

                gui_out
            } else {
                let mut gui_out = None;
                let gui_out_mut = &mut gui_out;
                let mut changed_simple = false;

                ui.label(
                    egui::RichText::new(format!(
                        "{:+012.5} {}",
                        port.port_get_numeric_value(None::<&()>),
                        port.port_get_interval()
                    ))
                    .monospace()
                    .size(11.0),
                );

                ui.separator();
                SanPolicyT::draw_san_info::<PortInnerT>(ui);
                ui.separator();

                ui.vertical(|ui| {
                    draw_collapsing_ui(ui, Some(port as *mut PortT), None, |ui| {
                        ui.label(egui::RichText::new(egui_phosphor::bold::PLUGS).size(14.0))
                            .on_hover_text("Value port: allows sanitization and optional range remapping");
                        // ui.label(format!("PORT({})", port.port_get_identity_str()));
                        ui.label("...");
                    })
                    .body(|ui| {
                        ui.separator();

                        ui.horizontal(|ui| port.port_inner_mut().egui(gui_in, ui))
                            .inner
                            .inspect(|out| *gui_out_mut = Some(out.clone()));

                        if port.port_is_transformable() {
                            ui.separator();

                            if let Some(tfm) = port.port_transformation_mut() {
                                let mut remove_tfm = false;
                                ui.collapsing("Transformation", |ui| {
                                    tfm.egui(*params.gui_common_ctx, ui)
                                        .inspect(|out| *gui_out_mut = Some(out.clone()));
                                    ui.separator();
                                    if ui.button("remove transformation").clicked() {
                                        remove_tfm = true;
                                        *gui_out_mut = bool_to_simple_change_gui_cmd(true);
                                    }
                                });
                                if remove_tfm {
                                    port.port_transformation_off();
                                }
                            } else {
                                if ui.small_button("add transformation").clicked() {
                                    port.port_transformation_on();
                                    *gui_out_mut = bool_to_simple_change_gui_cmd(true);
                                }
                            }
                        }

                        ui.separator();
                        ui.horizontal(|ui| {
                            if let Some(mut remap_to) = port.port_get_remap_interval() {
                                ui.label("remap: ");

                                if remap_to.egui(
                                    GuiInInterval::Edit {
                                        max_range: BaseNumT::MIN..=BaseNumT::MAX, // port.get_max_interval().cast().unwrap().make_range_inclusive(),
                                        from_label: "",
                                        to_label: "",
                                        sanitize_and_sort: true,
                                        truncate: false,
                                    },
                                    ui,
                                ) {
                                    port.port_set_remap_interval(remap_to);
                                    changed_simple = true;
                                };

                                if remap_to == port.port_get_default_interval_from_inner() {
                                    ui.label("(=default range)");
                                } else {
                                    ui.label("(overridden range)");
                                }

                                if ui
                                    .button(egui::RichText::new(egui_phosphor::bold::TRASH).size(14.0))
                                    .on_hover_text("Turn remapping Off")
                                    .clicked()
                                {
                                    port.port_set_remap_off();
                                    changed_simple = true;
                                }
                            } else {
                                let policy_enforced_remap_range = PortT::RemapT::get_remap_range();
                                if policy_enforced_remap_range.is_none()
                                    && ui
                                        .button("enable remapping")
                                        .on_hover_text("Turn remapping On")
                                        .clicked()
                                {
                                    port.port_set_remap_from_inner_default();
                                    changed_simple = true;
                                } else if let Some(policy_enforced_remap_range) = policy_enforced_remap_range {
                                    ui.label(format!("Port-enforced remapping range: {policy_enforced_remap_range}",));
                                }
                            }
                        });
                    })
                });

                gui_out.or(bool_to_simple_change_gui_cmd(changed_simple))
            }
        }
        GuiInValue::Display { .. } => None,
    }
}

pub(crate) fn draw_egui_for_a_value<'s, ValueT>(
    this: &mut ValueT,
    gui_in: GuiInValue<'s>,
    ui: &mut egui::Ui,
) -> Option<GuiCmd>
where
    ValueT: ValueIface + From<ValueTargets>,
    NumInterval<<ValueT as WithNumericValue>::ValueT>: DrawEgui<'s, Out = bool, In = GuiInInterval<'s>>,
    <ValueT as WithNumericValue>::ValueT: eframe::emath::Numeric,
{
    match &gui_in {
        GuiInValue::Edit(params) => {
            let mut changed = false;

            if let Some(new_target) = get_new_value_target(gui_in, ui) {
                *this = new_target.into();
                return Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter));
            }

            ui.label(this.value_identity());

            ui.label("Value:");
            let mut value = this.get_numeric_value();

            let value_slider = ui.add(
                egui::Slider::new(&mut value, this.get_interval().make_range_inclusive())
                    .logarithmic(params.slider_log_scale)
                    .custom_formatter(|value, _| format!("{value:+011.4}",))
                    .step_by(0.0001),
            );

            if value_slider.changed() && (value_slider.drag_stopped() || value_slider.dragged()) {
                if !this.get_device_control_matcher_ref().is_some() {
                    this.set_numeric_value(value);
                }
                if this.value_is_static() {
                    changed = true;
                }
            }

            // --
            ui.separator();

            ui.label("Range: ");
            if params.allow_interval_edit && this.value_is_static() {
                let mut interval = this.get_interval();
                if interval.egui(
                    GuiInInterval::Edit {
                        max_range: HID_AXIS_MAX_RANGE,
                        from_label: "From: ",
                        to_label: "To: ",
                        sanitize_and_sort: true,
                        truncate: false,
                    },
                    ui,
                ) {
                    this.set_interval(interval);
                    changed = true;
                }
                ui.label("");
            } else {
                ui.label(format!("{}", this.get_interval()));
            }
            bool_to_simple_change_gui_cmd(changed)
        }
        GuiInValue::Display { .. } => {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "Value: {}, Range: {}",
                        // self.get_port_identity_str(),
                        this.get_numeric_value(),
                        this.get_interval()
                    ))
                    .size(14.0)
                    .monospace()
                    .strong(),
                );
            });
            None
        }
    }
}

// -----------------------------
impl<'s> DrawEgui<'s> for ValueDsts {
    type In = GuiInValue<'s>;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        draw_egui_for_a_value(self, gui_in, ui)
    }
}

// -----------------------------
impl<'s> DrawEgui<'s> for ValueSrcs {
    type In = GuiInValue<'s>;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        draw_egui_for_a_value(self, gui_in, ui)
    }
}

// -----------------------------
impl<'s> DrawEgui<'s> for ValueXrcs {
    type In = GuiInValue<'s>;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        draw_egui_for_a_value(self, gui_in, ui)
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
                        AutoOrManual::Manual(_) => {
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
                        AutoOrManual::Auto(_) => {
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
    choice_context: &ValueUsageContext,
    ui: &mut egui::Ui,
    cfg_devices: &DevicesCfgNew,
    cfg_variables: &VariablesCfg,
) -> Option<DynValueRefs> {
    let mut gui_out = None;
    let gui_out_mut = &mut gui_out;
    let (allow_special_ffb, allow_joysticks_or_gamepads, allow_midi, allow_mice_or_kbd, allow_vars) =
        match choice_context {
            ValueUsageContext::MappingSrc => (true, true, true, true, true),
            ValueUsageContext::MappingDst => (false, true, false, true, true),
            ValueUsageContext::TfmStepAuxSrc => (true, true, true, true, true),
            ValueUsageContext::TfmStepAuxDst => (false, true, false, true, true),
            ValueUsageContext::TfmStepAuxXrc => (false, true, false, true, true),
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
                                    control_matcher_key: cm.0.to_string(),
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
            egui::CollapsingHeader::new("Joysticks or gamepads control matchers")
                .show(ui, |ui| {
                    choose_hid(
                        ui,
                        |dm: &(&String, &HidDeviceCfg)| dm.1.is_a_joystick() || dm.1.is_a_gamepad(),
                        cfg_devices,
                    )
                })
                .body_returned
                .unwrap_or_default()
                .inspect(|out| *gui_out_mut = Some(out.clone()));
        }

        if allow_mice_or_kbd {
            ui.separator();
            egui::CollapsingHeader::new("Mice or keyboard control matchers")
                .show(ui, |ui| {
                    choose_hid(
                        ui,
                        |dm: &(&String, &HidDeviceCfg)| dm.1.is_a_mouse() || dm.1.is_a_keyboard(),
                        cfg_devices,
                    )
                })
                .body_returned
                .unwrap_or_default()
                .inspect(|out| *gui_out_mut = Some(out.clone()));
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
                                *gui_out_mut = Some(DynValueRefs::DeviceControlMatcher(DeviceControlMatcherRef {
                                    device_matcher_key: d.0.to_string(),
                                    control_matcher_key: c.0.to_string(),
                                    control_matcher: ControlMatchers::Midi(c.1.clone()),
                                }));
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
    choice_context: ValueUsageContext,
    ui: &mut egui::Ui,
    egui_id_hashable: &str,
    window_title: &str,
    cfg_devices: &DevicesCfgNew,
    cfg_variables: &VariablesCfg,
) -> Option<ValueTargets> {
    let mut choice = None;
    ui.scope_builder(egui::UiBuilder::default(), |ui| {
        let egui_id_window_open = ui.auto_id_with(egui_id_hashable);
        let mut choose_ctl_window_opened = ui.data_mut(|d| d.get_temp(egui_id_window_open).unwrap_or(false));
        if choose_ctl_window_opened {
            ui.label("selecting...");
        } else if ui
            .button(egui_phosphor::regular::LIST_MAGNIFYING_GLASS.to_string())
            .on_hover_text(match choice_context {
                ValueUsageContext::MappingSrc => "Select main source",
                ValueUsageContext::MappingDst => "Select main destination",
                ValueUsageContext::TfmStepAuxSrc => "Select source",
                ValueUsageContext::TfmStepAuxDst => "Select destination",
                ValueUsageContext::TfmStepAuxXrc => "Select source-destination",
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
                        if choice_context.is_src() {
                            ui.separator();
                            ui.collapsing("Sink (a local variable)", |ui| {
                                ui.separator();
                                if ui.button("Sink").clicked() {
                                    static_value = Some(ValueTargets::Xrc(ValueXrcs::Sink(Default::default())));
                                }
                            });
                        } else {
                            ui.separator();
                            ui.collapsing("Void", |ui| {
                                ui.separator();
                                if ui.button("Void").clicked() {
                                    static_value = Some(ValueTargets::Dst(ValueDsts::Void(None)));
                                }
                            });
                        }
                    } else {
                        ui.separator();
                        ui.collapsing("Static", |ui| {
                            ui.separator();
                            if ui.button("Local static value").clicked() {
                                static_value = Some(ValueTargets::Src(ValueSrcs::Static(Default::default())));
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
                            ValueUsageContext::TfmStepAuxSrc | ValueUsageContext::MappingSrc => {
                                Some(ValueTargets::Src(ValueSrcs::Dynamic(dynamic)))
                            }
                            ValueUsageContext::MappingDst => Some(ValueTargets::Dst(ValueDsts::Dynamic(dynamic))),
                            ValueUsageContext::TfmStepAuxDst => Some(ValueTargets::Dst(ValueDsts::Dynamic(dynamic))),
                            ValueUsageContext::TfmStepAuxXrc => Some(ValueTargets::Xrc(ValueXrcs::Dynamic(dynamic))),
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
