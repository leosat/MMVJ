use crate::config::WithSelfSanitize;
use crate::device_and_device_manager::DeviceKind;
use crate::mapping::MappingEngineCmd;
use crate::{schemas_common::ObjId, schemas_control_matcher::ControlMatchers};
use crate::{
    schemas_common::WithRuntimeId,
    schemas_hid::HidControlMatcherCfg,
    schemas_mapping::Mapping,
    schemas_transform::TfmStepCfg,
    schemas_value::{DeviceControlMatcherRef, VariableRef, VariableState},
};
use eframe::egui;
use egui::{
    Ui,
    collapsing_header::{CollapsingState, HeaderResponse},
};
use enumflags2::BitFlags;
use std::{any::Any, path::PathBuf};

pub(crate) enum GuiInKinds {
    Edit,
    #[allow(unused)]
    Display,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum GuiDeviceClassifications {
    #[cfg(feature = "midi")]
    Midi,
    Hid(BitFlags<DeviceKind>),
    #[default]
    Unsupported,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuiDndJobMoveTfmStep {
    pub(crate) src_container_id: ObjId,
    pub(crate) src_obj_runtime_id: ObjId,
    pub(crate) dst_container_id_opt: Option<ObjId>,
    pub(crate) dst_idx_opt: Option<usize>,
    pub(crate) do_copy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuiDndJobNewTfmStep {
    pub(crate) step: TfmStepCfg,
}

#[derive(Debug, Clone)]
pub(crate) enum GuiDndJob {
    MoveTfmStep(GuiDndJobMoveTfmStep),
    NewTfmStep(std::sync::Arc<std::sync::Mutex<GuiDndJobNewTfmStep>>),
    // NewTfmStep(Box<GuiDndJobNewTfmStep>),
}

impl PartialEq for GuiDndJob {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::MoveTfmStep(l0), Self::MoveTfmStep(r0)) => l0 == r0,
            // TODO?: ... comparing ptrs with mutex is unstable...
            // TODO?: not critical for any of our purposes it's ok to have it returning false.
            (Self::NewTfmStep(_), Self::NewTfmStep(_)) => false,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuiCmdVariableChange {
    pub(crate) old_key: String,
    pub(crate) new_key: String,
    pub(crate) new_definition: VariableState,
}

impl traversable::VisitorMut for GuiCmdVariableChange {
    type Break = ();
    fn enter_mut(&mut self, this: &mut dyn core::any::Any) -> std::ops::ControlFlow<Self::Break> {
        if let Some(variable_ref) = this.downcast_mut::<VariableRef>()
            && variable_ref.variable_key == self.old_key
        {
            variable_ref.variable_key = self.new_key.clone();
            variable_ref.variable = self.new_definition.clone();
        }
        std::ops::ControlFlow::Continue(())
    }

    fn leave_mut(&mut self, this: &mut dyn core::any::Any) -> std::ops::ControlFlow<Self::Break> {
        if let Some(mapping) = this.downcast_mut::<Mapping>() {
            mapping.sanitize_inplace(());
        }

        std::ops::ControlFlow::Continue(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuiCmdDeviceKeyRename {
    pub(crate) old_key: String,
    pub(crate) new_key: String,
    pub(crate) is_hid: bool,
    pub(crate) is_virtual: bool,
}

impl traversable::VisitorMut for GuiCmdDeviceKeyRename {
    type Break = ();
    fn enter_mut(&mut self, node: &mut dyn Any) -> std::ops::ControlFlow<Self::Break, Self::Break> {
        if let Some(dcm) = node.downcast_mut::<DeviceControlMatcherRef>()
            && dcm.device_matcher_key == self.old_key
        {
            dcm.device_matcher_key = self.new_key.clone();
        }
        std::ops::ControlFlow::Continue(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuiCmdControlMatcherRemove {
    pub(crate) device_key: String,
    pub(crate) control_key: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuiCmdDeviceMatcherRemove {
    pub(crate) device_key: String,
    pub(crate) is_virtual: bool,
}

impl traversable::VisitorMut for GuiCmdControlMatcherRemove {
    type Break = ();
    fn enter_mut(&mut self, node: &mut dyn std::any::Any) -> std::ops::ControlFlow<Self::Break> {
        if let Some(dcr) = node.downcast_mut::<DeviceControlMatcherRef>()
            && dcr.device_matcher_key == self.device_key
            && dcr.control_matcher_key == self.control_key
        {
            return std::ops::ControlFlow::Break(());
        }
        std::ops::ControlFlow::Continue(())
    }
}

impl traversable::VisitorMut for GuiCmdDeviceMatcherRemove {
    type Break = ();
    fn enter_mut(&mut self, node: &mut dyn std::any::Any) -> std::ops::ControlFlow<Self::Break> {
        if let Some(dcr) = node.downcast_mut::<DeviceControlMatcherRef>()
            && dcr.device_matcher_key == self.device_key
        {
            return std::ops::ControlFlow::Break(());
        }
        std::ops::ControlFlow::Continue(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuiCmdVariableRemove {
    pub(crate) variable_key: String,
}

impl traversable::VisitorMut for GuiCmdVariableRemove {
    type Break = ();
    fn enter_mut(&mut self, node: &mut dyn std::any::Any) -> std::ops::ControlFlow<Self::Break> {
        if let Some(var_ref) = node.downcast_mut::<VariableRef>()
            && var_ref.variable_key == self.variable_key
        {
            return std::ops::ControlFlow::Break(());
        }
        std::ops::ControlFlow::Continue(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuiCmdControlMatcherChange {
    pub(crate) new_cm: ControlMatchers,
}

impl traversable::VisitorMut for GuiCmdControlMatcherChange {
    type Break = ();
    fn enter_mut(&mut self, node: &mut dyn Any) -> std::ops::ControlFlow<Self::Break> {
        match &self.new_cm {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(cm) => {
                use crate::schemas_midi::MidiControlMatcherCfg;

                if let Some(c) = node.downcast_mut::<MidiControlMatcherCfg>()
                    && c.id == cm.id
                {
                    *c = cm.clone();
                }
            }
            ControlMatchers::Hid(cm) => {
                if let Some(c) = node.downcast_mut::<HidControlMatcherCfg>()
                    && c.id == cm.id
                {
                    *c = cm.clone();
                }
            }
        }

        std::ops::ControlFlow::Continue(())
    }
}

// --------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ScriptAuxKind {
    Source,
    Destination,
    Transformation,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuiCmdScriptAuxRename {
    pub(crate) tfm_step_id: ObjId,
    pub(crate) kind: ScriptAuxKind,
    pub(crate) old_key: String,
    pub(crate) new_key: String,
}

impl traversable::VisitorMut for GuiCmdScriptAuxRename {
    type Break = ();
    fn enter_mut(&mut self, this: &mut dyn std::any::Any) -> std::ops::ControlFlow<Self::Break> {
        if let Some(step) = this.downcast_mut::<TfmStepCfg>()
            && step.get_id() == self.tfm_step_id
            && let TfmStepCfg::Script(script) = step
        {
            match self.kind {
                ScriptAuxKind::Source => {
                    if script.aux_srcs.contains_key(&self.new_key) {
                        return std::ops::ControlFlow::Break(());
                    }
                    if let Some(val) = script.aux_srcs.remove(&self.old_key) {
                        script.aux_srcs.insert(self.new_key.clone(), val);
                    }
                }
                ScriptAuxKind::Destination => {
                    if script.aux_dsts.contains_key(&self.new_key) {
                        return std::ops::ControlFlow::Break(());
                    }
                    if let Some(val) = script.aux_dsts.remove(&self.old_key) {
                        script.aux_dsts.insert(self.new_key.clone(), val);
                    }
                }
                ScriptAuxKind::Transformation => {
                    if script.aux_transformations.contains_key(&self.new_key) {
                        return std::ops::ControlFlow::Break(());
                    }
                    if let Some(val) = script.aux_transformations.remove(&self.old_key) {
                        script.aux_transformations.insert(self.new_key.clone(), val);
                    }
                }
            }
        }
        std::ops::ControlFlow::Continue(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuiCmdVirtualDeviceChange {
    pub(crate) restart_persistent: bool,
}

// --------------------------------
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GuiCmd {
    // -----------------------------
    CmdSeqence(Vec<GuiCmd>),
    BreakOnErr,
    // -----------------------------
    SubmitPending(Box<GuiCmd>),
    // -----------------------------
    LoadCfg(PathBuf),
    SaveCfg(Option<PathBuf>, Option<String>),
    // -----------------------------
    VirtualDeviceChange(GuiCmdVirtualDeviceChange),
    // -----------------------------
    VariableChange(GuiCmdVariableChange),
    VariableRemove(GuiCmdVariableRemove),
    // -----------------------------
    DeviceMatcherRename(GuiCmdDeviceKeyRename),
    DeviceMatcherRemove(GuiCmdDeviceMatcherRemove),
    // -----------------------------
    ControlMatcherChange(GuiCmdControlMatcherChange),
    ControlMatcherRemove(GuiCmdControlMatcherRemove),
    // -----------------------------
    ScriptAuxRename(GuiCmdScriptAuxRename),
    // -----------------------------
    MappingChange(MappingEngineCmd),
    // -----------------------------
    ConfigChangeSimple,
    ConfigChangeDriverRestart,
    // -----------------------------
    IdleTickRateChange,
    // -----------------------------
    DragAndDrop(GuiDndJob),
    LocalItemRemove(usize),
}

pub(crate) fn bool_to_simple_change_gui_cmd(changed: bool) -> Option<GuiCmd> {
    if changed {
        Some(GuiCmd::ConfigChangeSimple)
    } else {
        None
    }
}

// -------------------------------

// -------------------------------

// pub(crate) fn bool_to_general_change_gui_cmd(changed: bool) -> Option<GuiCmd> {
//     if changed {
//         Some(GuiCmd::ConfigChangeSimple)
//     } else {
//         None
//     }
// }

// -------------------------------
#[allow(unused)]
pub(crate) trait DrawEgui<'s> {
    type In;
    type Out;
    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out;
}

pub(crate) fn get_item_name_with_random_suffix(prefix: &str, count: usize) -> String {
    format!("{} #{}.{}", prefix, count, fastrand::u32(..))
}

//------------------------------
pub(crate) fn draw_collapsing_ui<'a, T>(
    ui: &'a mut Ui,
    id_salt: Option<impl std::hash::Hash>,
    heading: Option<&'a str>,
    header_fn: impl FnOnce(&mut Ui) -> T,
) -> HeaderResponse<'a, ()> {
    let id = if let Some(salt) = id_salt {
        ui.id().with(salt)
    } else {
        ui.id().with(heading)
    };
    let mut clicked = false;
    ui.visuals_mut().resize_corner_size = 20.0;
    let mut c = CollapsingState::load_with_default_open(ui.ctx(), id, false).show_header(ui, |ui: &mut egui::Ui| {
        if let Some(heading) = heading {
            clicked = ui.label(egui::RichText::new(heading).size(14.0).strong()).clicked();
        }
        header_fn(ui);
    });
    if clicked {
        c.set_open(clicked ^ c.is_open());
    }
    c
}
