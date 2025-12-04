use crate::config::MORE_DEBUG;
use crate::gui_common::{
    DrawEgui, GuiCmd, GuiDndJob, GuiDndJobMoveTfmStep, ScriptAuxKind, bool_to_simple_change_gui_cmd, draw_collapsing_ui,
};
use crate::gui_telemetry_graph::GuiTelemetryGraphStates;
use crate::gui_transform_step::GuiInTfmStepsSeq;
use crate::gui_value::{GuiInValue, GuiInValueEditParams};
use crate::mapping::MappingEngineCmd;
use crate::schemas_cfg::{DevicesCfgNew, VariablesCfg};

use crate::schemas_common::{ObjId, WithRuntimeId};
use crate::schemas_mapping::Mapping;
use crate::schemas_transform::{DynValFilter, TfmCfgDuplicateTreeWithNewState, collect_dynamic_value_matchers};
use crate::schemas_transform::{TfmSeqCfg, TfmStepCfg};
use crate::schemas_value::{DynValueRefs, ValueDsts};
use std::any::Any;
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::atomic::Ordering::Relaxed;

use egui::epaint::CornerRadiusF32;
use egui::{Frame, Shadow, TextEdit};
use traversable::TraversableMut;
use unchecked_refcell::UncheckedRefCell;
// -------------------------------

#[derive(Default, Clone, Copy)]
pub(crate) enum GuiInMapping<'s> {
    #[default]
    _Display,
    Edit {
        graph_states: &'s UncheckedRefCell<GuiTelemetryGraphStates>,
        cfg_devices: &'s DevicesCfgNew,
        cfg_variables: &'s VariablesCfg,
        #[allow(clippy::type_complexity)]
        transient_script_aux_edits: &'s UncheckedRefCell<HashMap<(ObjId, ScriptAuxKind), (String, String)>>,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ValueUsageContext {
    MappingSrc,
    MappingDst,
    TfmStepAuxSrc,
    TfmStepAuxDst,
    TfmStepAuxXrc,
}

impl ValueUsageContext {
    pub(crate) fn is_dst(&self) -> bool {
        match self {
            Self::MappingDst => true,
            Self::TfmStepAuxDst => true,
            Self::MappingSrc => false,
            Self::TfmStepAuxSrc => false,
            Self::TfmStepAuxXrc => true,
        }
    }
    pub(crate) fn is_src(&self) -> bool {
        match self {
            Self::MappingDst => false,
            Self::TfmStepAuxDst => false,
            Self::MappingSrc => true,
            Self::TfmStepAuxSrc => true,
            Self::TfmStepAuxXrc => true,
        }
    }
}

impl<'s> DrawEgui<'s> for Mapping {
    type In = (usize, GuiInMapping<'s>);
    type Out = Option<GuiCmd>;

    fn egui(&mut self, state: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut gui_out = None;
        let _mapping_idx = state.0;
        let state = state.1;
        match state {
            GuiInMapping::_Display => gui_out,
            GuiInMapping::Edit {
                graph_states,
                cfg_devices,
                cfg_variables,
                transient_script_aux_edits,
                ..
            } => {
                gui_out = Frame::default()
                    .inner_margin(0)
                    .shadow(Shadow::default())
                    .corner_radius(CornerRadiusF32::default().at_least(5.0))
                    .show(ui, |ui| {
                        let mut gui_out = ui
                            .horizontal(|ui| {
                                if ui
                                    .add(
                                        TextEdit::singleline(&mut self.name)
                                            .desired_width(0.0)
                                            .clip_text(false)
                                            .hint_text("Enter mapping name..."),
                                    )
                                    .changed()
                                {
                                    return bool_to_simple_change_gui_cmd(true);
                                };

                                ui.separator();

                                ui.horizontal(|ui| {
                                    ui.collapsing("Idle tick info...", |ui| {
                                        ui.separator();
                                        if self.requires_idle_tick {
                                            ui.label("Required.");
                                        } else {
                                            ui.label("Not required.");
                                        }
                                        #[allow(clippy::single_match)]
                                        match &self.dst {
                                            ValueDsts::Void(..) => {}
                                            ValueDsts::Dynamic(d) => match d {
                                                DynValueRefs::DeviceControlMatcher(d) => {
                                                    if d.control_matcher.get_idle_tick_enabled_flag().load(Relaxed) {
                                                        ui.label("On for dest control");
                                                    } else {
                                                        ui.label("Off for dest control");
                                                    }
                                                }
                                                _ => {}
                                            },
                                        }
                                    });
                                });

                                None
                            })
                            .inner;
                        ui.separator();
                        // ----------------------------------
                        /*Last mapping in*/
                        {
                            let last_in = self.last_in.load(Relaxed);
                            gui_out = gui_out.or(ui
                                .horizontal(|ui| {
                                    // ui.label(
                                    //     egui::RichText::new("Main src").strong(), // .color(Color32::LIGHT_BLUE.gamma_multiply(0.7)),
                                    // );
                                    // ui.separator();
                                    let gui_out = self.src.egui(
                                        GuiInValue::Edit(GuiInValueEditParams {
                                            allow_interval_edit: true,
                                            slider_log_scale: false,
                                            cfg_variables,
                                            cfg_devices,
                                            name: "Choose main mapping source",
                                            choice_case: ValueUsageContext::MappingSrc.into(),
                                        }),
                                        ui,
                                    );
                                    ui.separator();
                                    ui.label(
                                        egui::RichText::new(format!("<= {:+08.2}", last_in)).size(12.0).strong(), // .color(Color32::LIGHT_BLUE.gamma_multiply(0.7)),
                                    );
                                    ui.separator();
                                    gui_out
                                })
                                .inner);
                        }
                        let all_dynamic_sources =
                            collect_dynamic_value_matchers(&self.transformation, |ctx| ctx.contains(DynValFilter::Src));
                        if !all_dynamic_sources.is_empty() {
                            ui.separator();
                            ui.collapsing("In-pipeline referenced dynamic sources ...", |ui| {
                                for mut dcm in all_dynamic_sources {
                                    ui.separator();
                                    dcm.egui(
                                        GuiInValue::Display {
                                            usage_context: ValueUsageContext::MappingSrc,
                                        },
                                        ui,
                                    );
                                }
                            });
                        }
                        ui.separator();
                        //---------------------------------------------
                        /*Last mapping out*/
                        {
                            let last_out = self.last_out.load(Relaxed);
                            gui_out = gui_out.or(ui
                                .horizontal(|ui| {
                                    // ui.label(
                                    //     egui::RichText::new("Main dst").strong(), // .color(Color32::LIGHT_RED.gamma_multiply(0.7)),
                                    // );
                                    // ui.separator();
                                    let gui_out = self.dst.egui(
                                        GuiInValue::Edit(GuiInValueEditParams {
                                            allow_interval_edit: true,
                                            slider_log_scale: false,
                                            cfg_variables,
                                            cfg_devices,
                                            name: "Choose main mapping destination",
                                            choice_case: ValueUsageContext::MappingDst.into(),
                                        }),
                                        ui,
                                    );
                                    ui.separator();
                                    ui.label(
                                        egui::RichText::new(format!("=> {:+08.2}", last_out))
                                            .size(12.0)
                                            .strong(), // .color(Color32::LIGHT_RED.gamma_multiply(0.7)),
                                    );
                                    ui.separator();
                                    gui_out
                                })
                                .inner);
                        }
                        //---------------------------------------------
                        let all_dynamic_destinations =
                            collect_dynamic_value_matchers(&self.transformation, |ctx| ctx.contains(DynValFilter::Dst));
                        if !all_dynamic_destinations.is_empty() {
                            ui.separator();
                            ui.collapsing("In-pipeline referenced dynamic destinations ...", |ui| {
                                ui.separator();
                                for mut dcm in all_dynamic_destinations {
                                    dcm.egui(
                                        GuiInValue::Display {
                                            usage_context: ValueUsageContext::MappingDst,
                                        },
                                        ui,
                                    );
                                }
                            });
                        }
                        ui.separator();

                        gui_out = gui_out.or(ui
                            .collapsing(
                                egui::RichText::new("Transformation").heading(), // .background_color(Color32::from_gray(220))
                                // .color(Color32::BLACK)
                                |ui| {
                                    self.transformation.egui(
                                        GuiInTfmStepsSeq::Edit {
                                            graph_states,
                                            cfg_devices,
                                            hier: Vec::new(),
                                            cfg_variables,
                                            transient_script_aux_edits,
                                        },
                                        ui,
                                    )
                                },
                            )
                            .body_returned
                            .unwrap_or_default());

                        if let Some(GuiCmd::DragAndDrop(GuiDndJob::MoveTfmStep(dnd_job))) = &gui_out {
                            if MORE_DEBUG {
                                dbg!(&dnd_job);
                            }
                            let mut dnd_visitor = DndJobMoveTfmStep_Visitor {
                                dnd_job: dnd_job.clone(),
                                dropped_tfm_step: None,
                            };
                            for i in 0..=1 {
                                if self.transformation.traverse_mut(&mut dnd_visitor) == ControlFlow::Break(()) {
                                    gui_out = Some(GuiCmd::ConfigChangeSimple);
                                    break;
                                } else if i == 1 {
                                    log::error!("Dnd job failed to complete in 2 traversals. Error in implementation.");
                                }
                            }
                        }
                        gui_out
                    })
                    .inner;

                if gui_out.is_some() {
                    self.recompute_metadata(Some(self.name.clone()));
                }

                gui_out
            }
        }
    }
}

// -------------------------------------

impl crate::gui_main::GuiMain {
    pub(crate) fn draw_mappings_editor_gui(&mut self, ui: &mut egui::Ui) {
        let mut gui_out = None;
        let gui_out_mut = &mut gui_out;
        draw_collapsing_ui(
            ui,
            Some("Mappings list"),
            Some(&format!(
                "{} Mappings list (total: {}) ",
                egui_phosphor::bold::LIST,
                self.cfg.mappings.len(),
            )),
            |ui| {
                ui.horizontal(|ui| {
                    if self.cfg.mappings.iter().any(|v| v.enabled)
                        && ui
                            .button(egui_phosphor::bold::STOP.to_string())
                            .on_hover_text("Disable all mappings")
                            .clicked()
                    {
                        for m in &mut *self.cfg.mappings {
                            m.enabled = false;
                            *gui_out_mut = Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter));
                        }
                    };
                    if !self.cfg.mappings.iter().all(|v| v.enabled)
                        && ui
                            .button(egui_phosphor::bold::PLAY.to_string())
                            .on_hover_text("Enable all mappings")
                            .clicked()
                    {
                        for m in &mut *self.cfg.mappings {
                            m.enabled = true;
                            *gui_out_mut = Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter));
                        }
                    };

                    ui.separator();
                    if ui
                        .button(egui_phosphor::bold::PLUS.to_string())
                        .on_hover_text("Create new mapping")
                        .clicked()
                    {
                        self.cfg.mappings.push(Mapping::default());
                        self.update_cfg_yaml();
                    };
                });
            },
        )
        .body(|ui| {
            for (mapping_idx, mapping) in &mut self.cfg.mappings.iter_mut().enumerate() {
                ui.separator();
                ui.push_id(mapping_idx, |ui| {
                    ui.horizontal(|ui| {
                        {
                            let (label, on_hover_text) = if mapping.enabled {
                                (egui_phosphor::bold::STOP.to_string(), "Disable")
                            } else {
                                (egui_phosphor::bold::PLAY.to_string(), "Enable")
                            };
                            if ui.button(label).on_hover_text(on_hover_text).clicked() {
                                mapping.enabled = !mapping.enabled;
                                *gui_out_mut = Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter));
                            }
                        }

                        ui.separator();

                        ui.selectable_value(
                            &mut self.gui_tab_mappings_current_opened_mapping_idx,
                            mapping_idx,
                            format!(
                                "({mapping_idx}) {} {}",
                                mapping.name,
                                egui_phosphor::bold::DOTS_SIX_VERTICAL,
                            ),
                        );

                        ui.separator();

                        if ui
                            .button(egui_phosphor::bold::TRASH.to_string())
                            .on_hover_text("Remove mapping")
                            .clicked()
                        {
                            *gui_out_mut = Some(GuiCmd::LocalItemRemove(mapping_idx));
                        }
                    });
                });
            }
            ui.separator();
        });

        ui.separator();
        if self.cfg.mappings.len() > self.gui_tab_mappings_current_opened_mapping_idx {
            let mapping = &mut self.cfg.mappings[self.gui_tab_mappings_current_opened_mapping_idx];
            ui.group(|ui| {
                ui.push_id("mapping editor", |ui| {
                    mapping
                        .egui(
                            (
                                self.gui_tab_mappings_current_opened_mapping_idx,
                                GuiInMapping::Edit {
                                    graph_states: &self.telemetry_graphs,
                                    cfg_devices: &self.cfg.devices,
                                    cfg_variables: &self.cfg.variables,
                                    transient_script_aux_edits: &self.transient_states_script_aux_edit,
                                },
                            ),
                            ui,
                        )
                        .inspect(|out| *gui_out_mut = Some(out.clone()));
                })
            });
        }

        // if let Some(cmd) = &gui_out_mut {
        //     dbg!(cmd);
        // }

        if let Some(GuiCmd::LocalItemRemove(mapping_idx_to_remove)) = *gui_out_mut {
            self.cfg.mappings.remove(mapping_idx_to_remove);
            *gui_out_mut = Some(GuiCmd::MappingChange(MappingEngineCmd::UpdateMappingRouter))
        }

        if let Some(cmd) = gui_out {
            // dbg!(&cmd);
            self.submit_post_draw_cmd(cmd);
        }
    }
}

#[allow(non_camel_case_types)]
struct DndJobMoveTfmStep_Visitor {
    dnd_job: GuiDndJobMoveTfmStep,
    dropped_tfm_step: Option<TfmStepCfg>,
}

impl traversable::VisitorMut for DndJobMoveTfmStep_Visitor {
    type Break = ();

    #[allow(clippy::all)]
    fn enter_mut(&mut self, node: &mut dyn Any) -> std::ops::ControlFlow<Self::Break> {
        if let Some(tfm_seq) = node.downcast_mut::<TfmSeqCfg>() {
            if MORE_DEBUG {
                dbg!(format!("Drag and drop: visiting tfm step id {}", tfm_seq.id));
            }
            if self.dropped_tfm_step.is_none() {
                if let Some(found) = tfm_seq
                    .steps
                    .iter()
                    .enumerate()
                    .find(|&(_, step)| step.get_id() == self.dnd_job.src_obj_runtime_id)
                {
                    if MORE_DEBUG {
                        dbg!("Drag and drop: found object to drag.");
                        dbg!(found);
                    }
                    if self.dnd_job.do_copy {
                        self.dropped_tfm_step = Some(found.1.clone().duplicate_tree_with_new_state());
                    } else {
                        self.dropped_tfm_step = Some(found.1.clone());

                        if self.dnd_job.dst_idx_opt.unwrap() != usize::MAX
                            && self.dnd_job.dst_container_id_opt.unwrap() == self.dnd_job.src_container_id
                            && self.dnd_job.dst_idx_opt.unwrap() > found.0
                        {
                            self.dnd_job.dst_idx_opt = Some(self.dnd_job.dst_idx_opt.unwrap() - 1);
                            debug_assert!(self.dnd_job.dst_idx_opt.unwrap() < tfm_seq.steps.len());
                        }

                        tfm_seq
                            .steps
                            .retain_mut(|step| step.get_id() != self.dnd_job.src_obj_runtime_id);

                        tfm_seq.recompute_metadata_with_known_inputs();
                    }
                }
            } else if tfm_seq.id == self.dnd_job.dst_container_id_opt.unwrap() {
                if MORE_DEBUG {
                    dbg!("Drag and drop: insering!");
                }

                if self.dnd_job.dst_idx_opt.unwrap() == usize::MAX {
                    tfm_seq.steps.push(self.dropped_tfm_step.as_ref().unwrap().clone());
                    tfm_seq.steps.len() - 1
                } else {
                    tfm_seq.steps.insert(
                        self.dnd_job.dst_idx_opt.unwrap(),
                        self.dropped_tfm_step.as_ref().unwrap().clone(),
                    );
                    self.dnd_job.dst_idx_opt.unwrap()
                };

                tfm_seq.recompute_metadata_with_known_inputs();

                return std::ops::ControlFlow::Break(());
            }
        }
        std::ops::ControlFlow::Continue(())
    }
}
