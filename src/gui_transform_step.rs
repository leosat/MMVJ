use crate::base_num::BaseNumT;
use crate::relativity::Relativity;

use crate::config::MORE_DEBUG;
use crate::config::WithSanitize;
use crate::gui_common::{
    DrawEgui, GuiCmd, GuiCmdScriptAuxRename, GuiDndJob, GuiDndJobMoveTfmStep, GuiDndJobNewTfmStep, GuiInKinds,
    ScriptAuxKind, bool_to_simple_change_gui_cmd, draw_collapsing_ui, get_item_name_with_random_suffix,
};
use crate::gui_mapping::ValueRefChoiceContext;
use crate::gui_telemetry_graph::{GuiTelemetryGraphStates, make_trace_graph_2d};
use crate::gui_value::{GuiInInterval, GuiInValue, GuiInValueEditParams, draw_value_choice_iface};
use crate::hid_device::HID_AXIS_MAX_RANGE;
use crate::mapping::MappingEngineCmd;
use crate::num_interval::NumInterval;
use crate::num_interval::SYMM_UNIT_INTERVAL;
use crate::schemas_cfg::{DevicesCfgNew, VariablesCfg};
use crate::schemas_common::{ObjId, WithRuntimeId};
use crate::schemas_transform::*;
use crate::schemas_value::{
    DescriptionCfg, MappedValue, StaticValueCfg, ValueSrcs, WithDescriptionMut, WithNumericValue,
};
use crate::tracing::GraphDisplayStyle;
// use documented::{Documented, DocumentedFields};
use egui::text::LayoutJob;
use egui::{Button, CollapsingHeader, FontId, Sense, TextFormat, WidgetText};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use strum::IntoEnumIterator;
use unchecked_refcell::UncheckedRefCell;

// -----------------------------

const EMBEDDED_GRAPH_MAX_WIDTH: f32 = 500.0;

pub(crate) enum TfmStepTraceStage {
    In,
    Out,
    Custom(GraphDisplayStyle),
}

impl TfmStepCfg {
    pub(crate) fn get_graph_hash_key_string(&self) -> String {
        self.to_string() + " / @id: " + &self.get_id().to_string()
    }

    pub(crate) fn get_graph_legend(&self) -> String {
        format!(
            "Input: Blue, range: {}, Output: Red, range: {}.",
            self.common_state_ref().get_in_interval(),
            self.common_state_ref().get_out_interval()
        )
    }
}

// ---------------------------------

fn draw_graph_docked_or_windowed(
    tfm_step: &mut TfmStepCfg,
    gui_graphs: &UncheckedRefCell<GuiTelemetryGraphStates>,
    ui: &mut egui::Ui,
) -> bool {
    let mut changed = false;

    let is_graph_window_opened_egui_id = ui.make_persistent_id(1);
    let egui_state_is_graph_window_opened =
        &mut ui.data_mut(|d| d.get_temp::<bool>(is_graph_window_opened_egui_id).unwrap_or(false));
    let was_graph_window_opened = *egui_state_is_graph_window_opened;

    // ui.label(state.0.read().get_state_id().to_string());

    ui.vertical(|ui| {
        // ---------------------------------------
        if tfm_step.common_state_ref().trace_channel.is_none() {
            let (trace_graph_handle, gui_graph_state) = make_trace_graph_2d(
                &tfm_step.get_graph_hash_key_string(),
                &tfm_step.get_graph_legend(),
                Some(SYMM_UNIT_INTERVAL),
            );

            tfm_step.common_state_mut().trace_channel = Some(Arc::new(crate::tracing::make_trace_channel(vec![
                crate::tracing::TraceTarget::Graph(trace_graph_handle),
            ])));

            gui_graphs
                .borrow_mut()
                .insert(tfm_step.get_graph_hash_key_string(), gui_graph_state);

            changed = true; // to signal sync channels from gui cache to operational struct (when cache is implemented).
        }

        let mut graph_displayed = false;
        let mut display_graph = |ui: &mut egui::Ui| {
            if let Some(gui_graph) = gui_graphs.borrow_mut().get_mut(&tfm_step.get_graph_hash_key_string()) {
                if !tfm_step.common_state_ref().is_gui_tracing_enabled() {
                    tfm_step.common_state_ref().enable_gui_tracing();
                    changed = true;
                }
                graph_displayed = true;
                if gui_graph._in_interval != tfm_step.common_state_ref().get_in_interval() {
                    gui_graph.legend = tfm_step.get_graph_legend();
                }
                gui_graph.consume_input_queue_and_draw_gui(ui);
            };
        };

        if *egui_state_is_graph_window_opened {
            egui::Window::new(format!("{} transform input-output monitor.", tfm_step))
                .id(is_graph_window_opened_egui_id.with(42))
                .open(egui_state_is_graph_window_opened)
                .resizable(true)
                .order(egui::Order::TOP)
                .show(ui.ctx(), move |ui| {
                    display_graph(ui);
                })
                .map(|r| r.inner.unwrap_or_default())
                .unwrap_or_default();
            if !tfm_step.common_state_ref().is_gui_tracing_enabled() {
                tfm_step.common_state_ref().enable_gui_tracing();
                changed = true;
            }
            ui.label("... live monitor graph window opened ... ");
        } else {
            let c = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.id().with("collapsing"),
                false,
            );

            {
                let mut header_clicked = false;
                let mut r = c.show_header(ui, |ui| {
                    header_clicked = ui
                        .add(egui::Label::new("live monitor graph.").sense(egui::Sense::click()))
                        .clicked();
                    if !was_graph_window_opened
                        && ui
                            .button(egui_phosphor::bold::APP_WINDOW.to_string())
                            .on_hover_text("Open graph in a separate window")
                            .clicked()
                    {
                        *egui_state_is_graph_window_opened = true;
                    }
                });
                r.set_open(r.is_open() ^ header_clicked);
                r.body(|ui| {
                    display_graph(ui);
                });
            }
        }

        ui.data_mut(|d| d.insert_temp(is_graph_window_opened_egui_id, *egui_state_is_graph_window_opened));

        if !graph_displayed && tfm_step.common_state_ref().is_gui_tracing_enabled() {
            tfm_step.common_state_ref().disable_gui_tracing();
            changed = true;
        }

        changed
    })
    .inner
}

// ---------------------------------

#[derive(Default)]
pub(crate) enum GuiInTfmStepsSeq<'g> {
    Edit {
        graph_states: &'g UncheckedRefCell<GuiTelemetryGraphStates>,
        cfg_devices: &'g DevicesCfgNew,
        cfg_variables: &'g VariablesCfg,
        #[allow(clippy::all)]
        transient_script_aux_edits: &'g UncheckedRefCell<HashMap<(ObjId, ScriptAuxKind), (String, String)>>,
        hier: Vec<usize>,
    },
    #[allow(unused)]
    #[default]
    Display,
}

impl<'g> Clone for GuiInTfmStepsSeq<'g> {
    fn clone(&self) -> Self {
        match self {
            Self::Edit {
                graph_states,
                hier,
                cfg_devices,
                cfg_variables,
                transient_script_aux_edits,
            } => Self::Edit {
                graph_states,
                cfg_devices,
                hier: hier.clone(),
                cfg_variables,
                transient_script_aux_edits,
            },
            Self::Display => Self::Display,
        }
    }
}

impl<'g> GuiInTfmStepsSeq<'g> {
    pub(crate) fn _clone_mut(&mut self) -> Self {
        self.clone()
    }

    pub(crate) fn clone_and_push_hier(&self, id: ObjId) -> Self {
        let mut tmp = self.clone();
        match &mut tmp {
            GuiInTfmStepsSeq::Edit { hier, .. } => (*hier).push(*id),
            GuiInTfmStepsSeq::Display => unreachable!(),
        }
        tmp
    }

    pub(crate) fn get_hier(&self) -> Vec<usize> {
        match self {
            GuiInTfmStepsSeq::Edit { hier: obj_ids_hier, .. } => obj_ids_hier.to_vec(),
            GuiInTfmStepsSeq::Display => unreachable!(),
        }
    }

    pub(crate) fn is_editor(&self) -> bool {
        match self {
            GuiInTfmStepsSeq::Edit { .. } => true,
            GuiInTfmStepsSeq::Display => false,
        }
    }

    pub(crate) fn _get_graphs(&self) -> &'g UncheckedRefCell<GuiTelemetryGraphStates> {
        match self {
            GuiInTfmStepsSeq::Edit { graph_states, .. } => graph_states,
            GuiInTfmStepsSeq::Display => unreachable!(),
        }
    }
}

fn draw_step_in_out(ui: &mut egui::Ui, state: &TfmStepCommonState, is_enabled: bool) {
    let last_in = state.last_in.load(std::sync::atomic::Ordering::Relaxed);
    let last_out = state.last_out.load(std::sync::atomic::Ordering::Relaxed);

    let mut job = LayoutJob::default();

    let format = TextFormat {
        font_id: FontId::monospace(12.0),
        color: ui
            .style()
            .visuals
            .text_color()
            .gamma_multiply(1.0 - 0.5 * (!is_enabled as u8 as f32)),
        ..Default::default()
    };

    let in_label = if state.is_in_relative() { "rel" } else { "abs" };
    let out_label = if state.is_out_relative() { "rel" } else { "abs" };

    job.append(
        &format!(" {} ({:+08.2}) {}", in_label, last_in, state.get_in_interval()),
        0.0,
        format.clone(),
    );

    job.append(
        &format!(" -> {} ({:+08.2}) {}", out_label, last_out, state.get_out_interval()),
        0.0,
        format,
    );

    ui.label(job);
}

fn egui_dnd_drop_job_to_insert_job(
    egui_drop_job_tuple: (egui::InnerResponse<()>, Option<Arc<GuiDndJob>>),
    obj_ids_hier: &[usize],
    target_drop_idx: usize,
) -> Option<(usize, GuiDndJob)> {
    if let Some(dnd_job) = egui_drop_job_tuple.1 {
        match &*dnd_job {
            GuiDndJob::MoveTfmStep(dnd_job) => {
                if !obj_ids_hier.contains(&dnd_job.src_obj_runtime_id) {
                    // dbg!(&dnd_job);
                    return Some((target_drop_idx, GuiDndJob::MoveTfmStep((*dnd_job).clone())));
                } else if MORE_DEBUG {
                    dbg!("Won't drop inside self");
                }
            }
            GuiDndJob::NewTfmStep(dnd_job) => {
                return Some((target_drop_idx, GuiDndJob::NewTfmStep((*dnd_job).clone())));
            }
        }
    }
    None
}

impl<'s> DrawEgui<'s> for TfmSeqCfg {
    type In = GuiInTfmStepsSeq<'s>;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, state: Self::In, ui: &mut egui::Ui) -> Self::Out {
        if state.is_editor() {
            let mut gui_out = None;
            let gui_out_mut = &mut gui_out;

            ui.separator();

            *gui_out_mut = gui_out_mut.clone().or(ui
                .collapsing("Description", |ui| self.desc.egui(GuiInKinds::Edit, ui))
                .body_returned
                .unwrap_or_default());

            if let AutoOrManual::Manual(in_meta) = &mut self.in_meta {
                ui.separator();
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label("Input range:");
                    changed |= in_meta.interval.egui(
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
                    ui.label("Input relativity:");
                    changed |= in_meta.relativity.egui(GuiInKinds::Edit, ui);
                });

                if changed {
                    self.recompute_metadata_with_known_inputs();
                    *gui_out_mut = Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter));
                }
            }

            ui.separator();

            ui.horizontal(|ui| {
                ui.label(WidgetText::from("Steps:"));
                ui.separator();

                // '_Add_New_Tfm_Step:
                let is_win_opened_egui_id = ui.make_persistent_id("+ add step").with(*self.id); // gui::Id::new("Create new step window").with(self.id); // ui.make_persistent_id("..");
                let is_win_opened = &mut ui.data_mut(|d| d.get_temp::<bool>(is_win_opened_egui_id).unwrap_or(false));
                if !*is_win_opened {
                    if ui.small_button(format!("add {}", egui_phosphor::bold::STEPS)).clicked() {
                        *is_win_opened = true;
                    }
                } else {
                    ui.label("... choosing step to add ...");
                }

                let step_to_add_opt = egui::Window::new("Select transformation to add...")
                    .id(is_win_opened_egui_id)
                    .open(is_win_opened)
                    .resizable(true)
                    .order(egui::Order::TOP)
                    .show(ui.ctx(), |ui| {
                        ui.separator();
                        static STEPS_TEMPLATES_CACHE: std::sync::LazyLock<Vec<TfmStepCfg>> =
                            std::sync::LazyLock::new(|| TfmStepCfg::iter().collect::<Vec<_>>());
                        let mut step_to_add = None;
                        for step in &*STEPS_TEMPLATES_CACHE {
                            let btn_response = ui.add(Button::new(step.to_string()).sense(Sense::click_and_drag()));
                            if btn_response.clicked() {
                                step_to_add = Some(step.clone_with_new_state_no_recurse());
                            } else if btn_response.drag_started() {
                                btn_response.dnd_set_drag_payload(GuiDndJob::NewTfmStep(Box::from(
                                    GuiDndJobNewTfmStep {
                                        step: step.clone_with_new_state_no_recurse(),
                                    },
                                )));
                                // TODO?: maybe visual feedback of draggin the button...
                            }
                        }
                        ui.separator();
                        step_to_add
                    });
                if let Some(step_to_add) = step_to_add_opt
                    && let Some(step) = step_to_add.inner.flatten()
                {
                    self.steps.push(step);
                    self.recompute_metadata_with_known_inputs();
                    *gui_out_mut = gui_out_mut
                        .clone()
                        .or(Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter)));
                }
                ui.data_mut(|d| d.insert_temp(is_win_opened_egui_id, *is_win_opened));
            });

            ui.separator();

            let mut matched_dnd_job = None;
            let hier = state.get_hier();
            for (step_idx, step) in self.steps.iter_mut().enumerate() {
                // -- dnd detect drop.
                if matched_dnd_job.is_none() {
                    matched_dnd_job = egui_dnd_drop_job_to_insert_job(
                        ui.dnd_drop_zone::<GuiDndJob, _>(egui::Frame::default(), |ui| {
                            ui.separator();
                        }),
                        &hier,
                        step_idx,
                    );
                }

                ui.push_id(step.get_id(), |ui| {
                    *gui_out_mut = gui_out_mut
                        .clone()
                        .or(step.egui((step_idx, *self.id, state.clone()), ui))
                });
            }

            // -- dnd detect drop at array tail.
            matched_dnd_job = matched_dnd_job.or(egui_dnd_drop_job_to_insert_job(
                ui.dnd_drop_zone::<GuiDndJob, _>(egui::Frame::new(), |ui| {
                    ui.separator();
                }),
                &hier,
                usize::MAX,
            ));

            if let Some((dst_obj_idx, dnd_job_tmp)) = &mut matched_dnd_job {
                if MORE_DEBUG {
                    dbg!("Dropping!");
                }
                match dnd_job_tmp {
                    GuiDndJob::MoveTfmStep(dnd_job_tmp) => {
                        if dnd_job_tmp.dst_container_id_opt.is_none() {
                            dnd_job_tmp.dst_container_id_opt = Some(self.id);
                        }
                        if dnd_job_tmp.dst_idx_opt.is_none() {
                            dnd_job_tmp.dst_idx_opt = Some(*dst_obj_idx);
                        }
                        dnd_job_tmp.do_copy = ui.input(|i| i.modifiers.ctrl);
                        gui_out = gui_out.or(Some(GuiCmd::DragAndDrop(GuiDndJob::MoveTfmStep(dnd_job_tmp.clone()))));
                    }
                    GuiDndJob::NewTfmStep(dnd_job) => {
                        if *dst_obj_idx == usize::MAX {
                            self.steps.push(dnd_job.step.clone());
                        } else {
                            self.steps.insert(*dst_obj_idx, dnd_job.step.clone());
                        }
                        gui_out = gui_out.or(Some(GuiCmd::ConfigChangeSimple));
                    }
                }
            }

            // -----------------------------------------
            if gui_out.is_some() {
                if let Some(GuiCmd::LocalItemRemove(idx)) = gui_out {
                    self.steps.remove(idx);
                    gui_out = Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter));
                }
                self.recompute_metadata_with_known_inputs();
            }

            gui_out
        } else {
            unreachable!()
        }
    }
}

// ---------------------------------

impl<'s> DrawEgui<'s> for TfmStepCfg {
    type In = (usize, usize, GuiInTfmStepsSeq<'s>);
    type Out = Option<GuiCmd>;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let step_idx = gui_in.0;
        let container_id = gui_in.1;
        let step_id = self.get_id();
        match gui_in.2 {
            GuiInTfmStepsSeq::Edit {
                graph_states,
                cfg_devices,
                cfg_variables,
                ..
            } => {
                let in_is_relative = self.common_state_ref().is_in_relative();
                let transform_name = self.to_string();
                let label = format!("({}) {}", step_idx + 1, transform_name);
                let is_enabled = *self.get_enabled_ref_mut();
                let state_id = self.get_id();
                let heading = {
                    let mut heading = egui::RichText::new(&label);
                    if !is_enabled {
                        heading = heading.color(ui.visuals().weak_text_color()).size(17.0);
                    } else {
                        heading = heading
                            // .background_color(egui::Color32::from_gray(220))
                            // .color(egui::Color32::BLACK)
                            .size(17.0)
                            .strong();
                    }
                    heading
                };
                ui.scope_builder(
                    egui::UiBuilder::new().id(egui::Id::new(container_id).with(state_id)),
                    |ui| {
                        let collapsing = egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            ui.id().with("collapsing"),
                            // egui::Id::new(container_id).with(state_id),
                            false,
                        );
                        let header_response = collapsing
                            .show_header(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if crate::config::MORE_DEBUG {
                                        ui.label(format!(
                                            "ui.id() = {:?} + container_id: {} + tfm step state_id: {}",
                                            ui.id(),
                                            container_id,
                                            state_id
                                        ));
                                    }
                                    let _dnd_src = ui.dnd_drag_source(
                                        ui.id().with("DND Source"),
                                        GuiDndJob::MoveTfmStep(GuiDndJobMoveTfmStep {
                                            src_container_id: container_id.into(),
                                            src_obj_runtime_id: state_id,
                                            dst_container_id_opt: None,
                                            dst_idx_opt: None,
                                            do_copy: false,
                                        }),
                                        |ui| {
                                            ui.label(heading);
                                        },
                                    );

                                    if self.doc_str() != DEFAULT_TRANSFORM_DESCRIPTION {
                                        ui.label(
                                            egui::RichText::from(egui_phosphor::bold::CIRCLE_WAVY_QUESTION).size(16.0),
                                        )
                                        .on_hover_text(self.doc_str());
                                    }

                                    let mut gui_out = None;

                                    ui.separator();
                                    let enable_disable_text = if is_enabled { "on" } else { "off" };
                                    gui_out = gui_out.or(bool_to_simple_change_gui_cmd(
                                        ui.checkbox(self.get_enabled_ref_mut(), enable_disable_text).changed(),
                                    ));

                                    ui.separator();
                                    ui.scope(|ui| {
                                        ui.set_max_width(EMBEDDED_GRAPH_MAX_WIDTH);
                                        if draw_graph_docked_or_windowed(self, graph_states, ui) {
                                            gui_out = Some(GuiCmd::ConfigChangeSimple);
                                        }
                                    });

                                    draw_step_in_out(ui, self.common_state_ref(), is_enabled);

                                    ui.separator();
                                    if ui
                                        .button(egui_phosphor::bold::TRASH.to_string())
                                        .on_hover_text("Remove step")
                                        .clicked()
                                    {
                                        gui_out = Some(GuiCmd::LocalItemRemove(step_idx));
                                    }
                                    gui_out
                                })
                                .inner
                            })
                            .body(|ui| {
                                if crate::config::MORE_DEBUG {
                                    ui.label(format!("{:?}", ui.id()));
                                }
                                if !is_enabled {
                                    // let mut style = ui.style().as_ref().clone();
                                    // style.visuals.widgets.inactive.fg_stroke.color =
                                    //     style.visuals.widgets.inactive.fg_stroke.color.gamma_multiply(0.3);
                                    // ui.set_style(style);
                                    ui.disable();
                                }

                                if let Some(desc) = self.description_mut() {
                                    ui.separator();
                                    if let Some(gui_out) = ui
                                        .collapsing("Description", |ui| desc.egui(GuiInKinds::Edit, ui))
                                        .body_returned
                                        .unwrap_or_default()
                                    {
                                        return Some(gui_out);
                                    }
                                }

                                ui.separator();

                                ui.scope(
                                    // .collapsing(
                                    //     "Parameters",
                                    #[allow(unused)]
                                    |ui| {
                                        let label = self.to_string();
                                        let in_interval = self.common_state_ref().get_in_interval();
                                        match self {
                                            Self::Script(s) => s.egui(
                                                (
                                                    step_id,
                                                    gui_in.2.clone_and_push_hier(step_id),
                                                    GuiInValue::Edit(GuiInValueEditParams {
                                                        allow_interval_edit: true,
                                                        slider_log_scale: false,
                                                        cfg_variables,
                                                        cfg_devices,
                                                    }),
                                                ),
                                                ui,
                                            ),
                                            Self::Nop(_) | Self::Invert(_) => None,
                                            Self::Integrate(s) => s.egui(in_interval, ui),
                                            Self::Steering(s) => {
                                                ui.push_id(state_id, |ui| {
                                                    s.egui(gui_in.2.clone_and_push_hier(step_id), ui)
                                                })
                                                .inner
                                            }
                                            Self::Clamp(s) => s.egui(in_interval, ui),
                                            Self::RaiseFall(s) => s.egui((cfg_variables, cfg_devices, in_interval), ui),
                                            Self::Ema(s) => s.egui(in_is_relative, ui),
                                            Self::Linear(s) => s.egui(in_interval, ui),
                                            Self::Smoothstep { .. } => None,
                                            Self::SCurve(s) => s.egui((), ui),
                                            Self::Exp(s) => s.egui((), ui),
                                            Self::SignedPower(s) => s.egui((), ui),
                                            Self::OneEuro(s) => s.egui(in_is_relative, ui),
                                            Self::_HighPass(_) => None,
                                            Self::_ForceFeedback(_) => None,
                                        }
                                    },
                                )
                                .inner
                            });

                        header_response.1.inner.or(header_response.2.and_then(|v| v.inner))
                    },
                )
                .inner
            }
            GuiInTfmStepsSeq::Display => {
                unreachable!()
            }
        }
    }
}

// ---------------------------------

impl<'s> DrawEgui<'s> for ClampCfg {
    type In = NumInterval<BaseNumT>;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, in_interval: Self::In, ui: &mut egui::Ui) -> Self::Out {
        ui.label(format!("Input interval: {} ", in_interval));

        let mut changed = false;
        let clamping_interval = self.get_clamping_interval();
        let mut clamp_from_iherited_from_in_interval = clamping_interval.from == in_interval.from;
        let mut clamp_to_iherited_from_in_interval = clamping_interval.to == in_interval.to;

        changed |= ui
            .checkbox(
                &mut clamp_from_iherited_from_in_interval,
                "Lower bound inherited from current input interval.",
            )
            .changed();

        changed |= ui
            .checkbox(
                &mut clamp_to_iherited_from_in_interval,
                "Upper bound inherited from current input interval.",
            )
            .changed();

        ui.separator();

        ui.horizontal(|ui| {
            if clamp_from_iherited_from_in_interval {
                self.range.from = in_interval.from;
            }

            if clamp_to_iherited_from_in_interval {
                self.range.to = in_interval.to;
            }

            changed |= self.range.egui(
                GuiInInterval::Edit {
                    max_range: HID_AXIS_MAX_RANGE,
                    from_label: "From: ",
                    to_label: "To: ",
                    sanitize_and_sort: true,
                    truncate: false,
                },
                ui,
            );
        });

        ui.separator();
        changed |= ui
            .checkbox(&mut self.override_range, "Override output interval.")
            .changed();

        self.sanitize_inplace();

        bool_to_simple_change_gui_cmd(changed)
    }
}

impl<'s> DrawEgui<'s> for IntegrateCfg {
    type In = NumInterval<BaseNumT>;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, input_interval: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = false;

        ui.horizontal(|ui| {
            changed |= self.range.egui(
                GuiInInterval::Edit {
                    max_range: input_interval.scale(1000.0).make_range_inclusive(),
                    from_label: "From: ",
                    to_label: "To: ",
                    sanitize_and_sort: true,
                    truncate: false,
                },
                ui,
            );
        });

        ui.separator();

        changed |= ui
            .add(
                egui::Slider::new(&mut self.smoothing_alpha, 0.001..=1.0)
                    .text("Input gain")
                    .logarithmic(false),
            )
            .changed();

        bool_to_simple_change_gui_cmd(changed)
    }
}

impl<'s> DrawEgui<'s> for RaiseFallCfg {
    type In = (&'s VariablesCfg, &'s DevicesCfgNew, NumInterval<BaseNumT>);
    type Out = Option<GuiCmd>;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let (cfg_variables, cfg_devices, input_interval) = gui_in;
        let rize_and_fall_rates_interval = 0.01..=input_interval.to() * 10.0; // TODO: make multiplier configurable.
        let mut changed = false;
        ui.label(format!("Input interval: {}", input_interval));
        changed |= ui
            .add(
                egui::Slider::new(&mut self.fall_delay, 0.0..=10.0).text("Fall delay"), // .drag_value_speed(0.001),
            )
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut self.raise_rate, rize_and_fall_rates_interval.clone()).text("Raise rate"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut self.fall_rate, rize_and_fall_rates_interval).text("Fall rate"))
            .changed();
        changed |= ui
            .add(
                egui::Slider::new(&mut self.smoothing_alpha, 0.01..=1.0)
                    .text("Smoothing alpha")
                    .logarithmic(false),
            )
            .changed();
        ui.separator();
        ui.collapsing("Fall hold factor:", |ui| {
            ui.separator();
            changed |= self.fall_hold_factor.egui(
                (
                    "Choose hold factor source",
                    "Choose hold factor source",
                    ValueRefChoiceContext::TfmStepAuxSrc,
                    GuiInValue::Edit(GuiInValueEditParams {
                        allow_interval_edit: false,
                        slider_log_scale: false,
                        cfg_variables,
                        cfg_devices,
                    }),
                ),
                ui,
            );
            ui.separator();
            changed |= ui
                .checkbox(&mut self.invert_fall_hold_factor, "Invert fall hold factor")
                .changed();
        });
        bool_to_simple_change_gui_cmd(changed)
    }
}

impl<'s> DrawEgui<'s> for EmaFilterCfg {
    type In = bool;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, in_is_relative: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = false;
        changed |= ui
            .add(
                // TODO: intervals, validation!
                egui::Slider::new(&mut self.tau, 0.001..=2.0)
                    .text("Time constant")
                    .logarithmic(true),
            )
            .changed();
        if in_is_relative {
            ui.separator();
            changed |= draw_gui_idle_tick_params(ui, self);
        }
        bool_to_simple_change_gui_cmd(changed)
    }
}

impl<'s> DrawEgui<'s> for LinearCfg {
    type In = NumInterval<BaseNumT>;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, interval: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = ui
            .add(
                // TODO: intervals, validation!
                egui::Slider::new(&mut self.slope, -30.0..=30.0).text("Slope"),
            )
            .changed();
        let span = interval.span() as BaseNumT;
        changed |= ui
            .add(
                egui::Slider::new(&mut self.shift_x, -span..=span)
                    .text("Input shift by.")
                    .logarithmic(false),
            )
            .changed();
        changed |= ui
            .add(
                egui::Slider::new(&mut self.shift_y, -span..=span)
                    .text("Output shift by")
                    .logarithmic(false),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.center_symmetric,
                "Apply symmetrically around center of the value interval",
            )
            .changed();
        bool_to_simple_change_gui_cmd(changed)
    }
}

impl<'s> DrawEgui<'s> for SCurveCfg {
    type In = ();
    type Out = Option<GuiCmd>;

    fn egui(&mut self, _state: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let changed = ui
            .add(
                // TODO: intervals, validation!
                egui::Slider::new(&mut self.steepness, 1.0..=30.0)
                    .text("Steepness")
                    .logarithmic(false),
            )
            .changed();
        bool_to_simple_change_gui_cmd(changed)
    }
}

impl<'s> DrawEgui<'s> for NormExpCfg {
    type In = ();
    type Out = Option<GuiCmd>;

    fn egui(&mut self, _state: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed: bool = false;
        changed |= ui
            .add(
                egui::Slider::new(&mut self.base, 1.001..=200.0)
                    .text("Base")
                    .logarithmic(true),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.center_symmetric,
                "Apply symmetrically around center of the value interval",
            )
            .changed();
        if changed {
            Some(GuiCmd::ConfigChangeSimple)
        } else {
            None
        }
    }
}

impl<'s> DrawEgui<'s> for SignedPowerCfg {
    type In = ();
    type Out = Option<GuiCmd>;

    fn egui(&mut self, _state: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed: bool = false;
        changed |= ui
            .add(
                egui::Slider::new(&mut self.power, 0.001..=30.0)
                    .text("Power")
                    .logarithmic(true),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.center_symmetric,
                "Apply symmetrically around center of the value interval",
            )
            .changed();
        bool_to_simple_change_gui_cmd(changed)
    }
}

impl<'s> DrawEgui<'s> for OneEuroFilterCfg {
    type In = bool;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, in_is_relative: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed: bool = false;
        changed |= ui
            .add(
                egui::Slider::new(&mut self.min_cutoff_hz, 0.1..=1000.0)
                    .text("Lowpass base cutoff")
                    .suffix("Hz")
                    .logarithmic(true),
            )
            .changed();
        changed |= ui
            .add(
                egui::Slider::new(&mut self.beta, 0.0001..=10.0)
                    .text("Beta")
                    .logarithmic(true),
            )
            .changed();
        changed |= ui
            .add(
                egui::Slider::new(&mut self.d_cutoff_hz, 0.001..=1000.0)
                    .text("Derivative cutoff")
                    .suffix("Hz")
                    .logarithmic(true),
            )
            .changed();
        if in_is_relative {
            ui.separator();
            changed |= draw_gui_idle_tick_params(ui, self);
        }
        bool_to_simple_change_gui_cmd(changed)
    }
}

impl<'s> DrawEgui<'s> for ForceFeedbackCfg {
    type In = GuiInTfmStepsSeq<'s>;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut gui_out = None;
        match gui_in {
            GuiInTfmStepsSeq::Edit {
                cfg_devices,
                cfg_variables,
                ..
            } => {
                let mut changed = false;
                let mut use_custom = self.custom_source.is_some();

                changed |= ui
                    .checkbox(&mut use_custom, "Use custom value source for FFB")
                    .on_hover_text(self.custom_source_doc_str())
                    .changed();

                ui.separator();
                if !use_custom {
                    if self.custom_source.is_some() {
                        self.custom_source = None;
                        changed = true;
                    }
                    ui.horizontal(|ui| {
                        ui.label("Use force feedback component from mapping destination HID device: ")
                            .on_hover_text(
                                "Force feedback is received in 2d space with direction (Const force effect) or bound to X or Y \
                                component (Spring/Friction/Damper/Inertia effects). \
                                We are using FFB readings from destination virtual HID associated with the pipeline",
                            );
                        changed |= ui
                            .selectable_value(&mut self.component, ForceFeedbackComponent::X, "X")
                            .on_hover_text("Use X component of FFB")
                            .changed();
                        changed |= ui
                            .selectable_value(&mut self.component, ForceFeedbackComponent::Y, "Y")
                            .on_hover_text("Use Y component of FFB")
                            .changed();
                    });
                } else {
                    if use_custom && self.custom_source.is_none() {
                        self.custom_source = Some(ValueSrcs::Static(StaticValueCfg {
                            value: 0.0,
                            interval: SYMM_UNIT_INTERVAL,
                        }));
                        changed = true;
                    }

                    if let Some(ref mut custom_src) = self.custom_source {
                        let old_id = custom_src.get_id();
                        ui.horizontal(|ui| {
                            if custom_src.egui(
                                (
                                    "FFB custom source",
                                    "FFB custom source",
                                    ValueRefChoiceContext::TfmStepAuxSrc,
                                    GuiInValue::Edit(GuiInValueEditParams {
                                        allow_interval_edit: false,
                                        slider_log_scale: false,
                                        cfg_variables,
                                        cfg_devices,
                                    }),
                                ),
                                ui,
                            ) {
                                gui_out = if old_id != custom_src.get_id() {
                                    Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter))
                                } else {
                                    Some(GuiCmd::ConfigChangeSimple)
                                }
                            };
                        })
                        .response
                        .on_hover_text(self.custom_source_doc_str());
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    changed |= ui
                        .checkbox(&mut self.invert, "Invert force feedback")
                        .on_hover_text(self.invert_doc_str())
                        .changed();
                });

                ui.separator();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut self.gain, 0.0..=3.0)
                            .text("Force feedback gain.")
                            .logarithmic(false),
                    )
                    .on_hover_text(self.gain_doc_str())
                    .changed();

                gui_out = gui_out.or(bool_to_simple_change_gui_cmd(changed));

                ui.separator();
                gui_out = gui_out.or({
                    let c = ui.collapsing("Force feedback transformation:", |ui| {
                        self.transformation
                            .egui(gui_in.clone().clone_and_push_hier(self.transformation.id), ui)
                    });
                    c.header_response.on_hover_text(self.transformation_doc_str());
                    c.body_returned.unwrap_or_default()
                });
            }
            GuiInTfmStepsSeq::Display => {}
        }

        gui_out
    }
}

impl<'s> DrawEgui<'s> for SteeringCfg {
    type In = GuiInTfmStepsSeq<'s>;
    type Out = Option<GuiCmd>;

    fn egui(&mut self, state: Self::In, ui: &mut egui::Ui) -> Self::Out {
        match state {
            GuiInTfmStepsSeq::Edit {
                cfg_devices,
                cfg_variables,
                ..
            } => {
                let mut changed_simple = false;
                let mut gui_out = None;
                let gui_out_mut = &mut gui_out;

                ui.separator();
                ui.horizontal(|ui| {
                    let param_name = "Input gain";
                    ui.label(param_name).on_hover_text(self.input_gain_doc_str());
                    changed_simple |= self.input_gain.egui(
                        (
                            param_name,
                            param_name,
                            ValueRefChoiceContext::TfmStepAuxSrc,
                            GuiInValue::Edit(GuiInValueEditParams {
                                allow_interval_edit: false,
                                slider_log_scale: false,
                                cfg_variables,
                                cfg_devices,
                            }),
                        ),
                        ui,
                    );
                })
                .response
                .on_hover_text(self.input_gain_doc_str());
                ui.separator();
                ui.horizontal(|ui| {
                    let param_name = "Autocentering halflife";
                    ui.label("Autocenter halflife (0 == off): ")
                        .on_hover_text(self.auto_center_halflife_doc_str());
                    changed_simple |= ui
                        .horizontal(|ui| {
                            self.auto_center_halflife.egui(
                                (
                                    param_name,
                                    param_name,
                                    ValueRefChoiceContext::TfmStepAuxSrc,
                                    GuiInValue::Edit(GuiInValueEditParams {
                                        allow_interval_edit: false,
                                        slider_log_scale: false,
                                        cfg_variables,
                                        cfg_devices,
                                    }),
                                ),
                                ui,
                            )
                        })
                        .inner;
                });

                ui.separator();
                if let Some(ff) = &mut self.force_feedback {
                    if self.auto_center_halflife.get_numeric_value() > 0.0 {
                        ui.horizontal(|ui| {
                            let param_name = "Apply autocentering along with force feedback.";
                            ui.label(param_name)
                                .on_hover_text(Self::auto_center_along_force_feedback_doc_str_static());
                            changed_simple |= self.auto_center_along_force_feedback.egui(
                                (
                                    param_name,
                                    param_name,
                                    ValueRefChoiceContext::TfmStepAuxSrc,
                                    GuiInValue::Edit(GuiInValueEditParams {
                                        allow_interval_edit: false,
                                        slider_log_scale: false,
                                        cfg_variables,
                                        cfg_devices,
                                    }),
                                ),
                                ui,
                            );
                        });
                    }
                    ui.separator();
                    ui.collapsing("Force feedback params:", |ui| {
                        ui.separator();
                        *gui_out_mut = gui_out_mut.clone().or(ff.egui(state.clone(), ui));
                    })
                    .header_response
                    .on_hover_text(self.doc_str());
                } else {
                    ui.separator();
                    #[allow(clippy::field_reassign_with_default)]
                    if ui.button("Add force feedback params").clicked() {
                        let mut ff = ForceFeedbackCfg::default();
                        ff.enabled = true;
                        self.force_feedback = Some(ff);
                        changed_simple |= true;
                    }
                }

                ui.separator();
                ui.collapsing("Hold factor", |ui| {
                    ui.separator();
                    ui.horizontal(|ui| {
                        let id = self.hold_factor.get_id();
                        if self.hold_factor.egui(
                            (
                                "Choose hold factor source",
                                "Choose hold factor source",
                                ValueRefChoiceContext::TfmStepAuxSrc,
                                GuiInValue::Edit(GuiInValueEditParams {
                                    allow_interval_edit: false,
                                    slider_log_scale: false,
                                    cfg_devices,
                                    cfg_variables,
                                }),
                            ),
                            ui,
                        ) {
                            if id == self.hold_factor.get_id() {
                                *gui_out_mut = Some(GuiCmd::ConfigChangeSimple);
                            } else {
                                *gui_out_mut = Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter));
                            }
                        };
                    });
                })
                .header_response
                .on_hover_text(self.hold_factor_doc_str());

                gui_out = gui_out.or(bool_to_simple_change_gui_cmd(changed_simple));

                ui.separator();
                ui.horizontal(|ui| {
                    let param_name = "External accumulator";
                    ui.label(param_name);
                    if let Some(acc) = &mut self.accumulator {
                        changed_simple |= acc.egui(
                            (
                                param_name,
                                param_name,
                                ValueRefChoiceContext::TfmStepAuxSrc,
                                GuiInValue::Edit(GuiInValueEditParams {
                                    allow_interval_edit: false,
                                    slider_log_scale: false,
                                    cfg_variables,
                                    cfg_devices,
                                }),
                            ),
                            ui,
                        );
                        ui.separator();
                        if ui.button("Use built-in accumulator").clicked() {
                            self.accumulator = None;
                        }
                    }
                    {
                        let choice = match draw_value_choice_iface(
                            ValueRefChoiceContext::TfmStepAuxDst,
                            ui,
                            "Custom accumulator",
                            "Custom accumulator",
                            cfg_devices,
                            cfg_variables,
                        ) {
                            Some(vr) => match vr {
                                crate::schemas_value::ValuesRt::Src(value_srcs) => match value_srcs {
                                    ValueSrcs::Static(_) => None,
                                    ValueSrcs::Dynamic(srcs) => Some(srcs),
                                },
                                crate::schemas_value::ValuesRt::Dst(dsts) => match dsts {
                                    crate::schemas_value::ValueDsts::Dynamic(d) => Some(d),
                                    crate::schemas_value::ValueDsts::Void => None,
                                },
                            },
                            None => None,
                        };

                        if choice.is_some() {
                            self.accumulator = choice;
                            changed_simple |= true;
                        }
                    }
                })
                .response
                .on_hover_text(self.accumulator_doc_str());

                ui.separator();
                gui_out.or({
                    let c = ui.collapsing(
                        "Accumulated user input (pre force feedback and autocentering) transform",
                        |ui| {
                            self.integrated_user_input_transform
                                .egui(state.clone_and_push_hier(self.integrated_user_input_transform.id), ui)
                        },
                    );
                    c.header_response
                        .on_hover_text(self.integrated_user_input_transform_doc_str());
                    c.body_returned.unwrap_or_default()
                })
            }
            GuiInTfmStepsSeq::Display => None,
        }
    }
}

// =========================================

impl TfmStepCommonState {
    pub(crate) fn enable_gui_tracing(&self) {
        self.gui_trace_graph_opened.store(true, Relaxed);
    }

    pub(crate) fn disable_gui_tracing(&self) {
        self.gui_trace_graph_opened.store(false, Relaxed);
    }

    pub(crate) fn is_gui_tracing_enabled(&self) -> bool {
        self.gui_trace_graph_opened.load(Relaxed)
    }

    pub(crate) fn gui_trace(
        &self,
        stage: TfmStepTraceStage,
        vd: &MappedValue<BaseNumT>,
        timestamp: std::time::Instant,
    ) {
        if self.is_gui_tracing_enabled() {
            use egui::Color32;

            use crate::tracing::GraphDisplayStyle;

            let graph_style = match stage {
                TfmStepTraceStage::In => GraphDisplayStyle::as_line()
                    .with_color(Color32::BLUE)
                    .with_point_width(1.3),
                TfmStepTraceStage::Out => GraphDisplayStyle::as_line()
                    .with_color(Color32::RED)
                    .with_point_width(1.3),
                TfmStepTraceStage::Custom(graph_display_style) => graph_display_style,
            };

            if let Some(tc) = self.trace_channel.as_ref() {
                tc.trace(
                    SYMM_UNIT_INTERVAL.map_from(
                        vd.value,
                        &vd.interval,
                        crate::num_interval::OutOfRangePolicy::WarnAndClamp,
                    ),
                    SYMM_UNIT_INTERVAL,
                    timestamp,
                    graph_style,
                );
            }
        }
    }
}

// --------------------------------------------

fn draw_gui_idle_tick_params(ui: &mut egui::Ui, tfm: &mut impl TfmStepIdleBehavior) -> bool {
    let mut changed = false;
    ui.collapsing("Relative input on idle tick behavior:", |ui| {
        if !*tfm.relative_input_feed_on_idle_mut() && !*tfm.relative_input_reset_on_idle_mut() {
            ui.separator();
            ui.label("Current: applied only on user input, skipped on idle.");
        }
        ui.separator();
        ui.horizontal(|ui| {
            let frequency_warning = "Warning: behavior depends on idle tick frequency.".to_string();
            changed |= ui
                .checkbox(tfm.relative_input_feed_on_idle_mut(), "Feed on idle tick.")
                .on_hover_text(&frequency_warning)
                .changed();

            if *tfm.relative_input_feed_on_idle_mut() {
                ui.label(&frequency_warning);
                *tfm.relative_input_reset_on_idle_mut() = false;
            }

            ui.separator();
            changed |= ui
                .checkbox(tfm.relative_input_reset_on_idle_mut(), "Reset on idle tick.")
                .on_hover_text(&frequency_warning)
                .changed();
            if *tfm.relative_input_reset_on_idle_mut() {
                ui.label(&frequency_warning);
                *tfm.relative_input_feed_on_idle_mut() = false;
            }
        });
    });
    changed
}

// --------------------------------------------
impl<'s> DrawEgui<'s> for ScriptCfg {
    type In = (ObjId, GuiInTfmStepsSeq<'s>, GuiInValue<'s>);
    type Out = Option<GuiCmd>;

    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let tfm_step_id = gui_in.0;
        let mut changed_settings_general = false;
        let mut changed_settings_simple = false;
        let mut changed_script = false;
        let mut gui_out = None;
        let gui_out_mut = &mut gui_out;

        match gui_in.1 {
            GuiInTfmStepsSeq::Edit {
                transient_script_aux_edits,
                ..
            } => {
                // -----------------------------------------------
                // -----------------------------------------------
                // -----------------------------------------------
                {
                    let mut draw_aux_data_ui = |aux_kind: ScriptAuxKind| {
                        ui.separator();
                        ui.push_id(aux_kind, |ui| {
                            let c = draw_collapsing_ui(
                                ui,
                                None::<()>,
                                Some(match aux_kind {
                                    ScriptAuxKind::Source => "Aux sources",
                                    ScriptAuxKind::Destination => "Aux destinations",
                                    ScriptAuxKind::Transformation => "Aux transformations",
                                }),
                                |ui| {
                                    ui.separator();
                                    if ui
                                        .button(egui_phosphor::bold::LIST_PLUS.to_string())
                                        .on_hover_text(match aux_kind {
                                            ScriptAuxKind::Source => "add source",
                                            ScriptAuxKind::Destination => "add destination",
                                            ScriptAuxKind::Transformation => "add transformation",
                                        })
                                        .clicked()
                                    {
                                        match aux_kind {
                                            ScriptAuxKind::Source => {
                                                self.aux_srcs.insert(
                                                    get_item_name_with_random_suffix("Src", self.aux_srcs.len() + 1),
                                                    Default::default(),
                                                );
                                            }
                                            ScriptAuxKind::Destination => {
                                                self.aux_dsts.insert(
                                                    get_item_name_with_random_suffix("Dst", self.aux_dsts.len() + 1),
                                                    Default::default(),
                                                );
                                            }
                                            ScriptAuxKind::Transformation => {
                                                self.aux_transformations.insert(
                                                    get_item_name_with_random_suffix(
                                                        "Tfm",
                                                        self.aux_transformations.len() + 1,
                                                    ),
                                                    TfmSeqCfg {
                                                        in_meta: AutoOrManual::Manual(Default::default()),
                                                        ..Default::default()
                                                    },
                                                );
                                            }
                                        };
                                        changed_settings_general = true;
                                    }
                                },
                            )
                            .body(|ui| {
                                ui.separator();

                                let keys: Vec<String> = match aux_kind {
                                    ScriptAuxKind::Source => self.aux_srcs.keys().cloned().collect(),
                                    ScriptAuxKind::Destination => self.aux_dsts.keys().cloned().collect(),
                                    ScriptAuxKind::Transformation => self.aux_transformations.keys().cloned().collect(),
                                };
                                for (idx, name) in keys.iter().enumerate() {
                                    let key = (idx.into(), aux_kind);

                                    ui.separator();
                                    draw_collapsing_ui(ui, Some(idx), Some(name), |ui| {
                                        {
                                            ui.separator();
                                            let is_editing = transient_script_aux_edits.borrow().contains_key(&key);
                                            if !is_editing {
                                                if ui
                                                    .button(egui_phosphor::bold::IDENTIFICATION_BADGE.to_string())
                                                    .on_hover_text("Click to rename")
                                                    .clicked()
                                                {
                                                    transient_script_aux_edits
                                                        .borrow_mut()
                                                        .insert(key, (name.clone(), name.clone()));
                                                }
                                            } else {
                                                let rename_state = &mut *transient_script_aux_edits.borrow_mut();
                                                let (old, new) = rename_state.get_mut(&key).unwrap();
                                                ui.text_edit_singleline(new);
                                                ui.separator();
                                                if ui.button("done").clicked() {
                                                    if old != new && !new.is_empty() {
                                                        *gui_out_mut = gui_out_mut.clone().or(Some(
                                                            GuiCmd::ScriptAuxRename(GuiCmdScriptAuxRename {
                                                                tfm_step_id,
                                                                kind: aux_kind,
                                                                old_key: old.clone(),
                                                                new_key: new.clone(),
                                                            }),
                                                        ));
                                                    }
                                                    rename_state.remove(&key);
                                                }
                                                ui.separator();
                                                if ui.small_button("cancel").clicked() {
                                                    rename_state.remove(&key);
                                                }
                                            }
                                        }
                                        ui.separator();
                                        let remove = ui
                                            .button(egui_phosphor::bold::TRASH.to_string())
                                            .on_hover_text("Remove")
                                            .clicked();
                                        match aux_kind {
                                            ScriptAuxKind::Source => {
                                                if remove {
                                                    self.aux_srcs.remove(name);
                                                    changed_settings_general = true;
                                                }
                                            }
                                            ScriptAuxKind::Destination => {
                                                if remove {
                                                    self.aux_dsts.remove(name);
                                                    changed_settings_general = true;
                                                }
                                            }
                                            ScriptAuxKind::Transformation => {
                                                if remove {
                                                    self.aux_transformations.remove(name);
                                                    changed_settings_general = true;
                                                }
                                            }
                                        }
                                    })
                                    .body(|ui| {
                                        ui.horizontal(|ui| match aux_kind {
                                            ScriptAuxKind::Source => {
                                                if let Some(src) = self.aux_srcs.get_mut(name) {
                                                    let window_title = format!("Choose script input {} ", name);
                                                    *gui_out_mut =
                                                        gui_out_mut.clone().or(draw_egui_script_src_or_dest(
                                                            &mut ScriptSourceOrDestinationMut::Src(src),
                                                            ScriptCfgSourceOrDestinationGuiIn {
                                                                item_idx: idx,
                                                                a_value_gui_in: (
                                                                    &window_title,
                                                                    &window_title,
                                                                    ValueRefChoiceContext::TfmStepAuxSrc,
                                                                    gui_in.2,
                                                                ),
                                                            },
                                                            ui,
                                                        ));
                                                }
                                            }
                                            ScriptAuxKind::Destination => {
                                                if let Some(dst) = self.aux_dsts.get_mut(name) {
                                                    let window_title = format!("Choose script output {} ", name);
                                                    *gui_out_mut =
                                                        gui_out_mut.clone().or(draw_egui_script_src_or_dest(
                                                            &mut ScriptSourceOrDestinationMut::Dst(dst),
                                                            ScriptCfgSourceOrDestinationGuiIn {
                                                                item_idx: idx,
                                                                a_value_gui_in: (
                                                                    &window_title,
                                                                    &window_title,
                                                                    ValueRefChoiceContext::TfmStepAuxDst,
                                                                    gui_in.2,
                                                                ),
                                                            },
                                                            ui,
                                                        ));
                                                }
                                            }
                                            ScriptAuxKind::Transformation => {
                                                if let Some(tfm) = self.aux_transformations.get_mut(name) {
                                                    ui.push_id(idx, |ui| {
                                                        ui.vertical(|ui| {
                                                            *gui_out_mut =
                                                                gui_out_mut.clone().or(tfm.egui(gui_in.1.clone(), ui));
                                                        });
                                                    });
                                                }
                                            }
                                        });
                                    });
                                }
                            });

                            c.0.on_hover_text(match aux_kind {
                                ScriptAuxKind::Source => self.aux_srcs_doc_str(),
                                ScriptAuxKind::Destination => self.aux_dsts_doc_str(),
                                ScriptAuxKind::Transformation => self.aux_transformations_doc_str(),
                            });
                        });
                    };

                    // -----------------------------------------------
                    // -----------------------------------------------
                    draw_aux_data_ui(ScriptAuxKind::Source);
                    draw_aux_data_ui(ScriptAuxKind::Destination);
                    draw_aux_data_ui(ScriptAuxKind::Transformation);
                }

                // -------------------------------------------------------
                // -------------------------------------------------------
                ui.separator();
                let mut use_custom_interval = self.output_interval.is_some();
                changed_settings_simple |= ui
                    .checkbox(&mut use_custom_interval, "Custom output interval")
                    .on_hover_text(self.output_interval_doc_str())
                    .changed();
                if use_custom_interval {
                    ui.separator();
                    ui.horizontal(|ui| {
                        if self.output_interval.is_none() {
                            self.output_interval = Some(SYMM_UNIT_INTERVAL);
                            changed_settings_simple = true;
                        }
                        if let Some(interval) = &mut self.output_interval {
                            changed_settings_simple |= interval.egui(
                                GuiInInterval::Edit {
                                    max_range: HID_AXIS_MAX_RANGE,
                                    from_label: "From: ",
                                    to_label: "To: ",
                                    sanitize_and_sort: true,
                                    truncate: false,
                                },
                                ui,
                            );
                        }
                    });
                } else {
                    self.output_interval = None;
                }

                ui.separator();
                let mut use_custom_relativity = self.output_relativity.is_some();
                changed_settings_simple |= ui
                    .checkbox(&mut use_custom_relativity, "Custom output relativity")
                    .on_hover_text(self.output_relativity_doc_str())
                    .changed();
                if use_custom_relativity {
                    ui.separator();
                    ui.horizontal(|ui| {
                        if self.output_relativity.is_none() {
                            self.output_relativity = Some(Relativity::Abs);
                            changed_settings_simple = true;
                        }
                        if let Some(rel) = &mut self.output_relativity {
                            changed_settings_simple |= ui.radio_value(rel, Relativity::Abs, "Absolute").changed();
                            ui.separator();
                            changed_settings_simple |= ui.radio_value(rel, Relativity::Rel, "Relative").changed();
                        }
                    });
                } else {
                    self.output_relativity = None;
                }

                ui.separator();
                let c = draw_collapsing_ui(ui, None::<()>, Some("Script text"), |_| {}).body(|ui| {
                    ui.separator();
                    ui.label(egui::RichText::new(format!("Language: {}", self.lang)).strong())
                        .on_hover_text(self.lang_doc_str());
                    ui.separator();
                    changed_script = ui
                        .add(
                            egui::TextEdit::multiline(&mut self.script)
                                .font(egui::TextStyle::Monospace)
                                .interactive(true)
                                .desired_width(f32::INFINITY)
                                .hint_text("Create your script"),
                        )
                        .changed()
                });
                c.0.on_hover_text(self.script_doc_str());

                ui.separator();
                if changed_script {
                    *self.exe_state.get_mut() = None;
                    gui_out = Some(GuiCmd::ConfigChangeSimple)
                } else if changed_settings_general {
                    gui_out = Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter))
                } else if changed_settings_simple {
                    gui_out = Some(GuiCmd::ConfigChangeSimple)
                }
            }
            GuiInTfmStepsSeq::Display => {}
        };

        gui_out
    }
}

pub(super) struct ScriptCfgSourceOrDestinationGuiIn<'s> {
    item_idx: usize,
    a_value_gui_in: (&'s str, &'s str, ValueRefChoiceContext, GuiInValue<'s>),
}

enum ScriptSourceOrDestinationMut<'s> {
    Src(&'s mut ScriptSourceCfg),
    Dst(&'s mut ScriptDestinationCfg),
}

fn draw_egui_script_src_or_dest<'s>(
    this: &mut ScriptSourceOrDestinationMut,
    gui_in: ScriptCfgSourceOrDestinationGuiIn<'s>,
    ui: &mut egui::Ui,
) -> Option<GuiCmd> {
    let mut gui_out = None;
    match gui_in.a_value_gui_in.3 {
        GuiInValue::Edit(_) => {
            ui.push_id(gui_in.item_idx, |ui| {
                ui.group(|ui| {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let mut remove_remap_interval = false;
                            let remap_to_or_from_interval: &mut Option<NumInterval<BaseNumT>> = match this {
                                ScriptSourceOrDestinationMut::Src(script_source_cfg) => {
                                    &mut script_source_cfg.remap_to_interval
                                }
                                ScriptSourceOrDestinationMut::Dst(script_destination_cfg) => {
                                    &mut script_destination_cfg.remap_from_interval
                                }
                            };
                            {
                                let remove_remap_interval = &mut remove_remap_interval;

                                if let Some(interval) = remap_to_or_from_interval {
                                    ui.horizontal(|ui| {
                                        ui.label("Remapping range:");
                                        if (interval).egui(
                                            GuiInInterval::Edit {
                                                max_range: HID_AXIS_MAX_RANGE,
                                                from_label: "From: ",
                                                to_label: "To: ",
                                                sanitize_and_sort: true,
                                                truncate: false,
                                            },
                                            ui,
                                        ) {
                                            gui_out = Some(GuiCmd::ConfigChangeSimple)
                                        };
                                        if ui
                                            .button(egui_phosphor::bold::TRASH.to_string())
                                            .on_hover_text("Remove remapping range")
                                            .clicked()
                                        {
                                            *remove_remap_interval = true;
                                            gui_out = Some(GuiCmd::ConfigChangeSimple)
                                        }
                                    });
                                } else {
                                    if ui
                                        .button(egui_phosphor::bold::ALIGN_LEFT)
                                        .on_hover_text("Define a range to remap...")
                                        .clicked()
                                    {
                                        *remap_to_or_from_interval = Some(SYMM_UNIT_INTERVAL);
                                        gui_out = Some(GuiCmd::ConfigChangeSimple)
                                    }
                                }
                            }
                            if remove_remap_interval {
                                *remap_to_or_from_interval = None;
                            }
                        })
                    });
                    ui.group(|ui| match this {
                        ScriptSourceOrDestinationMut::Src(script_source_cfg) => {
                            let id = script_source_cfg.source.get_id();
                            if script_source_cfg.source.egui(gui_in.a_value_gui_in, ui) {
                                if script_source_cfg.source.get_id() == id {
                                    gui_out = Some(GuiCmd::ConfigChangeSimple);
                                } else {
                                    gui_out = Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter));
                                }
                            }
                        }
                        ScriptSourceOrDestinationMut::Dst(script_destination_cfg) => {
                            if script_destination_cfg.destination.egui(gui_in.a_value_gui_in, ui) {
                                gui_out = Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter));
                            }
                        }
                    });
                })
                .inner
            })
            .inner
        }
        GuiInValue::Display => {}
    }
    gui_out
}

impl<'s> DrawEgui<'s> for Vec<TfmSeqCfg> {
    type In = GuiInTfmStepsSeq<'s>;
    type Out = Option<GuiCmd>;
    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut gui_out = None;
        for (tfm_idx, tfm_seq) in self.iter_mut().enumerate() {
            ui.separator();
            let ch = CollapsingHeader::new(format!("Aux transformation #{} ({:.80}...)", tfm_idx, *tfm_seq.desc))
                .id_salt(tfm_idx);
            gui_out = gui_out.or(ch
                .show(ui, |ui| ui.group(|ui| tfm_seq.egui(gui_in.clone(), ui)).inner)
                .body_returned
                .unwrap_or_default());
            ui.separator();
        }
        ui.separator();
        if ui.button("Add transformation sequence").clicked() {
            self.push(TfmSeqCfg::default());
            gui_out = Some(GuiCmd::ConfigChangeSimple);
        }
        gui_out
    }
}

impl<'s> DrawEgui<'s> for DescriptionCfg {
    type In = GuiInKinds;
    type Out = Option<GuiCmd>;
    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut gui_out = None;
        match gui_in {
            GuiInKinds::Edit => {
                gui_out = bool_to_simple_change_gui_cmd(
                    ui.add(
                        egui::TextEdit::multiline(&mut self.0)
                            .font(egui::TextStyle::Monospace)
                            .interactive(true)
                            // .background_color(Color32::DARK_BLUE)
                            // .text_color(Color32::GREEN)
                            .desired_width(f32::INFINITY)
                            .hint_text("Create description"),
                    )
                    .changed(),
                )
            }
            GuiInKinds::Display => {
                ui.label(format!("Description: {}", self.0));
            }
        };
        gui_out
    }
}
