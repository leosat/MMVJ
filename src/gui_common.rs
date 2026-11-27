use crate::base_num::BaseNumT;
use crate::config::WithSelfSanitize;
use crate::device_and_device_manager::DeviceKind;
use crate::gui_mapping::ValueUsageContext;
use crate::gui_value::GuiInValue;
use crate::gui_widgets::ClippingIndicator;
use crate::mapping::MappingEngineCmd;
use crate::num_interval::NumInterval;
use crate::schemas_transform::{DynValFilter, collect_dynamic_value_matchers};
use crate::schemas_value::DynValueRefs;
use crate::{schemas_common::ObjId, schemas_control_matcher::ControlMatchers};
use crate::{
    schemas_common::WithRuntimeId,
    schemas_hid::HidControlMatcherCfg,
    schemas_mapping::Mapping,
    schemas_transform::TfmStepCfg,
    schemas_value::{DeviceControlMatcherRef, VariableCfg, VariableRef},
};
use eframe::egui;
use egui::util::IdTypeMap;
use egui::{
    Ui,
    collapsing_header::{CollapsingState, HeaderResponse},
};
use enumflags2::BitFlags;
use regex::Regex;
use std::collections::HashMap;
use std::{any::Any, path::PathBuf};
use traversable::Traversable;

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
    pub(crate) new_definition: VariableCfg,
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
        #[cfg(feature = "scripting")]
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
pub(crate) struct GuiCmdChangeVirtualHid {
    pub(crate) virtual_hid_key: String,
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
    LoadCfgFile(PathBuf),
    LoadCfgDemo(String),
    LoadCfgBlank(),
    SaveCfg(Option<PathBuf>, Option<String>),
    // -----------------------------
    ChangeVirtualHid(GuiCmdChangeVirtualHid),
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
    /// Checkpoint config for undor and propagate updated config to mapper, no other actions required.
    ConfigChangeSimple,
    /// Propagate updated config to mapper, no other actions required, no undo checkpoint done.
    ConfigChangeMiscNoUndoCheckpoint,
    /// Propagate updated config to mapper, fully reload the driver.
    ConfigChangeDriverRestart,
    // -----------------------------
    BaseTickRateChange,
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

pub(crate) trait DrawEgui<'s> {
    type In;
    type Out;
    fn egui(&mut self, gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out;
    #[allow(unused)]
    fn egui_immutable(&self, gui_in: Self::In, ui: &mut egui::Ui) {}
}

pub(crate) fn get_item_name_with_random_suffix(prefix: &str, count: usize) -> String {
    format!("{} #{}.{}", prefix, count, fastrand::u32(..))
}

//------------------------------
pub(crate) fn draw_collapsing_ui<'a, T, SaltT: std::hash::Hash + std::fmt::Debug>(
    ui: &'a mut Ui,
    id_salt: Option<SaltT>,
    heading: Option<&'a str>,
    header_fn: impl FnOnce(&mut Ui) -> T,
) -> HeaderResponse<'a, ()> {
    let id = if let Some(salt) = id_salt {
        ui.scope_id().with(salt)
    } else {
        ui.scope_id().with(heading)
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

pub(crate) fn with_clipping_indicator_state(
    ui: &mut egui::Ui,
    id: egui::Id,
    interval: NumInterval<BaseNumT>,
    callback: impl Fn(&mut egui::Ui, &mut ClippingIndicator),
) {
    let mut ci = ui.data_mut(|d| {
        get_or_create_clipping_indicators_cache(d)
            .entry(id)
            .or_insert(ClippingIndicator::new(interval))
            .clone()
    });
    callback(ui, &mut ci);
    ui.data_mut(|d| get_or_create_clipping_indicators_cache(d).insert(id, ci));
}

fn get_or_create_clipping_indicators_cache(cache: &mut IdTypeMap) -> &mut HashMap<egui::Id, ClippingIndicator> {
    cache.get_temp_mut_or_insert_with(egui::Id::unique("CLIPPING INDICATORS CACHE"), Default::default)
}

pub(crate) fn clear_clipping_indicators_cache(ui: &mut egui::Ui) {
    ui.data_mut(|d| {
        get_or_create_clipping_indicators_cache(d).clear();
    })
}

fn get_or_create_dynamic_targets_cache(cache: &mut IdTypeMap) -> &mut HashMap<egui::Id, Vec<DynValueRefs>> {
    cache.get_temp_mut_or_insert_with(egui::Id::unique("DYNAMIC TARGETS CACHE"), Default::default)
}

pub(crate) fn clear_dynamic_targets_cache(ui: &mut egui::Ui) {
    ui.data_mut(|d| {
        get_or_create_dynamic_targets_cache(d).clear();
    })
}

pub(crate) fn collect_and_cache_dyn_srcs(ui: &mut egui::Ui, obj: &impl Traversable) -> Vec<DynValueRefs> {
    ui.data_mut(|d| {
        get_or_create_dynamic_targets_cache(d)
            .entry(egui::Id::unique(obj as *const _).with("dynamic srcs"))
            .or_insert(collect_dynamic_value_matchers(obj, |ctx| {
                ctx.contains(DynValFilter::Src)
            }))
            .clone()
    })
}

pub(crate) fn collect_and_cache_dyn_dsts(ui: &mut egui::Ui, obj: &impl Traversable) -> Vec<DynValueRefs> {
    ui.data_mut(|d| {
        get_or_create_dynamic_targets_cache(d)
            .entry(egui::Id::unique(obj as *const _).with("dynamic dsts"))
            .or_insert(collect_dynamic_value_matchers(obj, |ctx| {
                ctx.contains(DynValFilter::Dst)
            }))
            .clone()
    })
}

pub(crate) fn draw_dyn_value_refs_list(ui: &mut egui::Ui, refs: &mut [DynValueRefs], msg: &str) {
    if !refs.is_empty() {
        ui.vertical(|ui| {
            ui.set_min_width(800.0);
            ui.collapsing(msg, |ui| {
                ui.separator();
                for r#ref in refs {
                    r#ref.egui(
                        GuiInValue::Display {
                            usage_context: ValueUsageContext::MappingDst,
                        },
                        ui,
                    );
                    ui.separator();
                }
            });
        });
    }
}

// -----------------
#[must_use]
pub(crate) fn draw_regex_editor(in_regex: &mut regex::Regex, ui: &mut egui::Ui) -> bool {
    let mut changed = false;
    let edited_regex_string_id = ui
        .scope_id()
        .with("edited regex string")
        .with(in_regex as *mut regex::Regex);
    let mut regex_compilation_failed = false;
    let mut regex_string = if let Some(edited_regex_string) = ui.data(|d| d.get_temp::<String>(edited_regex_string_id))
    {
        regex_compilation_failed = true;
        edited_regex_string
    } else {
        in_regex.to_string()
    };
    if ui.text_edit_singleline(&mut regex_string).changed() {
        if let Ok(regex) = Regex::new(&regex_string) {
            changed = true;
            *in_regex = regex;
            log::info!("Compiled regex from string {regex_string}");
            ui.data_mut(|d| d.remove_temp::<String>(edited_regex_string_id));
        } else {
            log::error!("Failed to compile regex from string {regex_string}");
            ui.data_mut(|d| d.insert_temp(edited_regex_string_id, regex_string));
        }
    }
    if regex_compilation_failed {
        ui.label(egui::RichText::new("regex compilation failure").color(egui::Color32::RED))
            .on_hover_text("Regex string is not saved!");
    }
    changed
}

// -----------------

fn default_ui_callback(_ui: &mut egui::Ui) {}
fn default_trigger_callback(_ui: &mut egui::Ui) -> bool {
    false
}

pub(crate) struct OptionallyWindowed<
    's,
    WindowTitleT = &'s str,
    PlaceholderUiT = fn(&mut egui::Ui),
    WindowUiT = fn(&mut egui::Ui),
    TriggerUiT = fn(&mut egui::Ui) -> bool,
> where
    WindowTitleT: egui::IntoAtoms<'s>,
    PlaceholderUiT: FnOnce(&mut egui::Ui),
    WindowUiT: FnOnce(&mut egui::Ui),
    TriggerUiT: FnOnce(&mut egui::Ui) -> bool,
{
    window_id: egui::Id,
    window_title: WindowTitleT,
    window_initial_trigger_ui: TriggerUiT,
    window_opened_placeholder_ui: PlaceholderUiT,
    window_ui: WindowUiT,
    _lifetime: std::marker::PhantomData<&'s ()>,
}

impl<'s> Default for OptionallyWindowed<'s> {
    fn default() -> Self {
        Self::new(egui::Id::NULL)
    }
}

impl<'s> OptionallyWindowed<'s> {
    pub(crate) fn new(window_id: egui::Id) -> Self {
        Self {
            window_id,
            window_title: "",
            window_initial_trigger_ui: default_trigger_callback,
            window_opened_placeholder_ui: default_ui_callback,
            window_ui: default_ui_callback,
            _lifetime: std::marker::PhantomData,
        }
    }
}

impl<'s, WindowTitleT, PlaceholderUiT, WindowUiT, TriggerUiT>
    OptionallyWindowed<'s, WindowTitleT, PlaceholderUiT, WindowUiT, TriggerUiT>
where
    WindowTitleT: egui::IntoAtoms<'s>,
    PlaceholderUiT: FnOnce(&mut egui::Ui),
    WindowUiT: FnOnce(&mut egui::Ui),
    TriggerUiT: FnOnce(&mut egui::Ui) -> bool,
{
    #[allow(unused)]
    pub(crate) fn with_id(mut self, window_id: egui::Id) -> Self {
        self.window_id = window_id;
        self
    }

    pub(crate) fn with_title<NewWindowTitleT>(
        self,
        window_title: NewWindowTitleT,
    ) -> OptionallyWindowed<'s, NewWindowTitleT, PlaceholderUiT, WindowUiT, TriggerUiT>
    where
        NewWindowTitleT: egui::IntoAtoms<'s>,
    {
        OptionallyWindowed {
            window_id: self.window_id,
            window_title,
            window_initial_trigger_ui: self.window_initial_trigger_ui,
            window_opened_placeholder_ui: self.window_opened_placeholder_ui,
            window_ui: self.window_ui,
            _lifetime: std::marker::PhantomData,
        }
    }

    pub(crate) fn with_initial_trigger_ui<NewTriggerUiT>(
        self,
        window_initial_trigger_ui: NewTriggerUiT,
    ) -> OptionallyWindowed<'s, WindowTitleT, PlaceholderUiT, WindowUiT, NewTriggerUiT>
    where
        NewTriggerUiT: FnOnce(&mut egui::Ui) -> bool,
    {
        OptionallyWindowed {
            window_id: self.window_id,
            window_title: self.window_title,
            window_initial_trigger_ui,
            window_opened_placeholder_ui: self.window_opened_placeholder_ui,
            window_ui: self.window_ui,
            _lifetime: std::marker::PhantomData,
        }
    }

    pub(crate) fn with_placeholder_ui<NewPlaceholderUiT>(
        self,
        window_opened_placeholder_ui: NewPlaceholderUiT,
    ) -> OptionallyWindowed<'s, WindowTitleT, NewPlaceholderUiT, WindowUiT, TriggerUiT>
    where
        NewPlaceholderUiT: FnOnce(&mut egui::Ui),
    {
        OptionallyWindowed {
            window_id: self.window_id,
            window_title: self.window_title,
            window_initial_trigger_ui: self.window_initial_trigger_ui,
            window_opened_placeholder_ui,
            window_ui: self.window_ui,
            _lifetime: std::marker::PhantomData,
        }
    }

    pub(crate) fn with_window_ui<NewWindowUiT>(
        self,
        window_ui: NewWindowUiT,
    ) -> OptionallyWindowed<'s, WindowTitleT, PlaceholderUiT, NewWindowUiT, TriggerUiT>
    where
        NewWindowUiT: FnOnce(&mut egui::Ui),
    {
        OptionallyWindowed {
            window_id: self.window_id,
            window_title: self.window_title,
            window_initial_trigger_ui: self.window_initial_trigger_ui,
            window_opened_placeholder_ui: self.window_opened_placeholder_ui,
            window_ui,
            _lifetime: std::marker::PhantomData,
        }
    }

    pub(crate) fn show(self, ui: &mut egui::Ui) -> bool {
        let mut window_opened = ui.data_mut(|d| *d.get_temp_mut_or_default::<bool>(self.window_id));

        if window_opened {
            (self.window_opened_placeholder_ui)(ui);
            egui::Window::new(self.window_title)
                .open(&mut window_opened)
                .resizable(true)
                .id(self.window_id)
                .show(ui.ctx(), |ui| (self.window_ui)(ui));
        } else if (self.window_initial_trigger_ui)(ui) {
            window_opened = true;
        }

        ui.data_mut(|d| d.insert_temp(self.window_id, window_opened));
        window_opened
    }
}

pub(crate) fn draw_button_to_window<'s>(
    window_title: impl egui::IntoAtoms<'s>,
    window_id: egui::Id,
    window_open_button_atoms: impl egui::IntoAtoms<'s>,
    window_open_button_on_hover_text: impl Into<egui::WidgetText>,
    window_opened_text: impl Into<egui::WidgetText>,
    ui: &'s mut egui::Ui,
    func: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut window_opened = ui.data_mut(|d| *d.get_temp_mut_or_default::<bool>(window_id));

    if window_opened {
        ui.label(window_opened_text);
        egui::Window::new(window_title)
            .open(&mut window_opened)
            .resizable(true)
            .id(window_id)
            .show(ui.ctx(), |ui| func(ui));
    } else {
        if ui
            .button(window_open_button_atoms)
            .on_hover_text(window_open_button_on_hover_text)
            .clicked()
        {
            window_opened = true;
        };
    }
    ui.data_mut(|d| d.insert_temp(window_id, window_opened));
    window_opened
}
