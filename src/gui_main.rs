use crate::config::MORE_DEBUG;
use crate::config::WithSelfSanitize;
use crate::debug::DebugLevel;
use crate::device_and_device_manager::AvailableDeviceInfoIface;
use crate::device_and_device_manager::DeviceKind;
use crate::device_and_device_manager::DeviceManagerCommon;
use crate::device_and_device_manager::MatchesAvailableDeviceIface;
use crate::device_and_device_manager::WithDeviceClassification;
use crate::driver::DriverCmd;
use crate::driver::check_system_requirements;
use crate::gui_common::GuiDndJob;
use crate::gui_common::{
    DrawEgui, GuiCmd, GuiCmdChangeVirtualHid, GuiCmdVariableChange, GuiCmdVariableRemove, GuiInKinds, ScriptAuxKind,
    draw_collapsing_ui, get_item_name_with_random_suffix,
};
use crate::gui_common::{GuiCmdDeviceKeyRename, GuiCmdDeviceMatcherRemove};
use crate::gui_device::GuiInDeviceCfg;
use crate::gui_mapping::DndJobMoveTfmStep_Visitor;
use crate::gui_style::GUI_STYLE_DARK;
use crate::gui_style::GUI_STYLE_GRAY_OUTLINED;
use crate::gui_style::GUI_STYLE_LIGHT;
use crate::gui_style::GuiStyle;
use crate::gui_telemetry_graph::GuiTelemetryGraphStates;
use crate::gui_undo::GuiCfgUndo;
use crate::hid_manager::{AvailableHIDDeviceInfo, HidManager};
#[cfg(feature = "midi")]
use crate::midi::{AvailableMidiDeviceInfo, MidiManager};
use crate::schemas_cfg::Config;
use crate::schemas_common::DeviceCfgCommonIface;
use crate::schemas_common::ObjId;
use crate::schemas_hid::HidDeviceCfg;
use crate::schemas_mapping::MapperMode;
use crate::schemas_mapping::MapperModeKind;
#[cfg(feature = "midi")]
use crate::schemas_midi::MidiMatcherCfg;
use crate::schemas_ui::UiMonitorsCfg;
use crate::schemas_value::VariableCfg;
use eframe::egui::{self};
use eframe::egui_glow;
use eframe::wgpu::naga::FastHashSet;
use egui::scroll_area::ScrollSource;
use egui_file_dialog::FileDialog;
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::ops::Deref;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use traversable::TraversableMut;
use unchecked_refcell::UncheckedRefCell;
use winit::platform::wayland::EventLoopBuilderExtWayland;
use winit::platform::x11::EventLoopBuilderExtX11;

//====================================================================================
#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    monitors_only: bool,
    cmd_tx: UnboundedSender<DriverCmd>,
    cancellation_token: CancellationToken,
    cfg: Config,
) -> anyhow::Result<()> {
    let eframe_result = {
        let mapping_engine_cmd = cmd_tx.clone();

        eframe::run_native(
            crate::config::APP_LONG_NAME,
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_always_on_top()
                    .with_movable_by_background(true)
                    .with_visible(true)
                    .with_decorations(true)
                    .with_active(true)
                    .with_window_level(egui::WindowLevel::AlwaysOnTop)
                    .with_inner_size([900.0, 800.0]),
                event_loop_builder: Some(Box::new(|builder| {
                    EventLoopBuilderExtX11::with_any_thread(builder, true);
                    EventLoopBuilderExtWayland::with_any_thread(builder, true);
                })),
                renderer: eframe::Renderer::Glow,
                //multisampling: 0,
                //dithering: false,
                glow_options: egui_glow::GlowConfiguration {
                    vsync: true,
                    hardware_acceleration: egui_glow::HardwareAcceleration::Preferred,
                    shader_version: Default::default(),
                },

                persist_window: true,
                // persistence_path: Some({
                //     let mut p = std::env::temp_dir().to_path_buf(); p.push("mmvj"); p}),
                ..Default::default()
            },
            Box::new(move |cc| {
                // -----------------------------------------------------
                let mut app = GuiMain {
                    cfg_undo: GuiCfgUndo::new_with_cfg(&cfg, cc.egui_ctx.time()),
                    exit_app: false,
                    // ---
                    pending_cmds: Default::default(),
                    post_draw_cmds: Default::default(),
                    // ---
                    transient_states_variable_edit: Default::default(),
                    transient_states_device_key_edit: Default::default(),
                    transient_states_script_aux_edit: Default::default(),
                    // ---
                    file_dialog: FileDialog::new(),
                    file_picking_action_in_progress: FilePickingAction::None,
                    // ---
                    current_opened_tab: GuiMainTabs::Mappings,
                    gui_tab_mappings_current_opened_mapping_idx: 0,
                    // ----------------------------------------------
                    telemetry_graphs: UncheckedRefCell::new(GuiTelemetryGraphStates::new()),
                    // ----------------------------------------------
                    monitors_only,
                    show_monitors: true,
                    // ----------------------------------------------
                    driver_tx: mapping_engine_cmd,
                    // ----------------------------------------------
                    cfg,
                    // ----------------------------------------------
                    cfg_yaml: String::default(),
                    // ----------------------------------------------
                    cancellation_token,
                    // ----------------------------------------------
                    #[cfg(feature = "midi")]
                    midi_mgr: MidiManager::new(DebugLevel::Off).expect("Can't create midi manager"),
                    hid_mgr: HidManager::new(DebugLevel::Off, false).expect("Can't create joysticks manager"),
                    _running_virtual_hid: None,
                    avail_dev_info_cache: Default::default(),
                    style: GUI_STYLE_LIGHT.clone(),
                    system_requirements_chk_status: check_system_requirements().err().map(|e| format!("{e}")),
                    gui_tab_mappings_windowed_mappings: Default::default(),
                };

                if let Some(storage) = cc.storage {
                    log::info!("Persistent storage is available ... ");
                    let saved: Option<GuiMainSavedState> = eframe::get_value(storage, eframe::APP_KEY);
                    if let Some(saved) = saved {
                        // dbg!(&saved);
                        log::info!("Restoring Gui state ... ");
                        app.fill_from_saved_state(&saved);
                        log::info!("Done restoring Gui state ... ");
                    }
                } else {
                    log::warn!("Persistent storage is not accessible, will not restore Gui state.");
                }

                app.style.setup_egui_ctx(&cc.egui_ctx);
                app.show_monitors |= monitors_only;
                app.update_cfg_yaml();

                Ok(Box::new(app))
            }),
        )
    };
    log::info!("GUI closed, status: {:?}", eframe_result);
    GuiMain::send_driver_cmd_static(&cmd_tx, DriverCmd::StatusGuiClosed);
    Ok(())
}

//====================================================================================
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
enum GuiMainTabs {
    ConfigDescription,
    Mappings,
    Devices,
    Variables,
    ConfigYaml,
    RuntimeConfigState,
    Log,
}

enum FilePickingAction {
    Load,
    Save,
    None,
}

pub(super) struct GuiTransientStateItemEdit<T> {
    edited_key: String,
    edited_definition: T,
}

#[derive(Serialize, Deserialize, Debug)]
pub(super) struct GuiMainSavedState {
    style: GuiStyle,
    current_opened_tab: GuiMainTabs,
    gui_tab_mappings_current_opened_mapping_idx: usize,
    show_monitors: bool,
}

impl From<&mut GuiMain> for GuiMainSavedState {
    fn from(app: &mut GuiMain) -> Self {
        Self {
            current_opened_tab: app.current_opened_tab,
            gui_tab_mappings_current_opened_mapping_idx: app.gui_tab_mappings_current_opened_mapping_idx,
            show_monitors: app.show_monitors,
            style: app.style.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingCommand {
    pub(super) cmd: GuiCmd,
    /* aux metadata for pending and postponed commands may go here */
}

#[derive(Default)]
struct AvailDevInfoCache {
    available_hid: Option<Vec<AvailableHIDDeviceInfo>>,
    #[cfg(feature = "midi")]
    available_midi: Option<Vec<AvailableMidiDeviceInfo>>,
}

impl AvailDevInfoCache {
    fn clear(&mut self) {
        self.available_hid = None;
        #[cfg(feature = "midi")]
        {
            self.available_midi = None;
        }
    }

    fn hid(&mut self, hid_mgr: &HidManager) -> &[AvailableHIDDeviceInfo] {
        self.available_hid
            .get_or_insert_with(|| {
                hid_mgr.enumerate_available_devices(Some(
                    //  HidDeviceKind::Misc |
                    //  HidDeviceKind::MiscMappable
                    DeviceKind::Mouse | DeviceKind::Keyboard | DeviceKind::Joystick | DeviceKind::Gamepad,
                ))
            })
            .as_slice()
    }

    #[cfg(feature = "midi")]
    fn midi(&mut self, midi_mgr: &MidiManager) -> &[AvailableMidiDeviceInfo] {
        self.available_midi
            .get_or_insert_with(|| midi_mgr.enumerate_available_devices(None))
            .as_slice()
    }
}

pub(crate) struct GuiMain {
    pub(super) cfg_undo: GuiCfgUndo,
    // ---
    system_requirements_chk_status: Option<String>, // TODO: common status notifications abstraction #121
    exit_app: bool,
    // ---
    pub(super) style: GuiStyle,
    // ---
    pub(super) pending_cmds: Vec<PendingCommand>,
    post_draw_cmds: Vec<PendingCommand>,
    // ---
    pub(super) transient_states_variable_edit: HashMap<String, GuiTransientStateItemEdit<VariableCfg>>,
    pub(super) transient_states_device_key_edit: HashMap<String, String>,
    pub(super) transient_states_script_aux_edit: UncheckedRefCell<HashMap<(ObjId, ScriptAuxKind), (String, String)>>,
    // ---
    file_dialog: FileDialog,
    file_picking_action_in_progress: FilePickingAction,
    // ---
    driver_tx: UnboundedSender<DriverCmd>,
    // ---
    current_opened_tab: GuiMainTabs,
    // ---
    pub(super) gui_tab_mappings_windowed_mappings: FastHashSet<usize>,
    pub(super) gui_tab_mappings_current_opened_mapping_idx: usize,
    // -----------------------------------------
    pub(super) telemetry_graphs: UncheckedRefCell<GuiTelemetryGraphStates>,
    // -----------------------------------------
    monitors_only: bool,
    show_monitors: bool,
    // -----------------------------------------
    pub(super) cfg: Config,
    // -----------------------------------------
    cfg_yaml: String,
    //----------------------------------------------
    cancellation_token: CancellationToken,
    //----------------------------------------------
    #[cfg(feature = "midi")]
    midi_mgr: MidiManager,
    hid_mgr: HidManager,
    _running_virtual_hid: Option<Vec<AvailableHIDDeviceInfo>>,
    avail_dev_info_cache: AvailDevInfoCache,
}

impl eframe::App for GuiMain {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // ---------------------------------------------
        // Calling undo/redo handler early so that shortcuts are consumed
        // before e.g. a separate text edit undo-redo can see them.
        self.handle_cfg_undo_redo_pressed(ui);

        // ---------------------------------------------
        fn do_save(app: &mut GuiMain, frame: &mut eframe::Frame) {
            if let Some(storage) = frame.storage_mut() {
                app.save_state(storage);
            }
        }

        // ---------------------------------------------
        if ui.ctx().input(|i| i.viewport().close_requested()) || self.cancellation_token.is_cancelled() || self.exit_app
        {
            do_save(self, frame);
            self.send_driver_cmd(DriverCmd::Halt);
            if !ui.ctx().input(|i| i.viewport().close_requested()) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }

        // ---------------------------------------------
        if self.show_monitors {
            for (monitor_cfg_idx, monitor_cfg) in self.cfg.ui.monitors.iter_mut().enumerate() {
                match monitor_cfg {
                    UiMonitorsCfg::Axis(m) => ui.ctx().show_viewport_immediate(
                        egui::ViewportId::from_hash_of(format!("monitor {monitor_cfg_idx}")),
                        egui::ViewportBuilder::default()
                            .with_title(format!("MMVJ Monitor: {}", m.name))
                            .with_transparent(false)
                            .with_fullsize_content_view(true)
                            .with_visible(true)
                            .with_active(true)
                            .with_inner_size([800.0, 50.0]),
                        |ui, _class| {
                            if ui.ctx().input(|i| i.viewport().close_requested()) {
                                self.show_monitors = false;
                            }
                            m.egui((), ui);
                            // self.axis_monitors.ui(ui, frame);
                        },
                    ),
                }
            }
        }

        if branches::unlikely(self.system_requirements_chk_status.is_some()) {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.add_space(10.0);
                    ui.label(self.system_requirements_chk_status.as_ref().unwrap());
                    ui.add_space(30.0);
                    if ui.button("Close").clicked() {
                        self.system_requirements_chk_status = None;
                        // ui.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
            return;
        }

        if self.monitors_only {
            if ui.ctx().input(|r| r.viewport().visible()).unwrap_or_default() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(true));
            }
            return;
        }

        let start_time = std::time::Instant::now();

        egui::Panel::top("top_panel")
            .frame(
                egui::Frame::default()
                    .fill(ui.visuals().window_fill)
                    .inner_margin(12)
                    .outer_margin(0),
            )
            .show(ui, |ui| {
                egui::MenuBar::default().ui(ui, |ui| {
                    match self.file_picking_action_in_progress {
                        FilePickingAction::Load => {
                            self.file_dialog.update(ui.ctx());
                            if let Some(path) = self.file_dialog.take_picked() {
                                self.submit_post_draw_cmd(GuiCmd::LoadCfgFile(path));
                                self.file_picking_action_in_progress = FilePickingAction::None;
                            }
                        }
                        FilePickingAction::Save => {
                            self.file_dialog.update(ui.ctx());
                            if let Some(path) = self.file_dialog.take_picked() {
                                self.submit_post_draw_cmd(GuiCmd::SaveCfg(Some(path), None));
                                self.file_picking_action_in_progress = FilePickingAction::None;
                            }
                        }
                        FilePickingAction::None => {}
                    };

                    ui.separator();
                    ui.menu_button("File", |ui| {
                        ui.separator();
                        if ui
                            .button(format!(
                                "{} {} New blank config",
                                egui_phosphor::regular::NOTE_BLANK,
                                egui_phosphor::regular::DOTS_SIX_VERTICAL
                            ))
                            .clicked()
                        {
                            self.submit_post_draw_cmd(GuiCmd::LoadCfgBlank());
                        }

                        ui.separator();
                        ui.menu_button(
                            format!(
                                "{} {} Load demo config",
                                egui_phosphor::regular::TREE_PALM,
                                egui_phosphor::regular::DOTS_SIX_VERTICAL,
                            ),
                            |ui| {
                                for (demo_name, demo) in crate::config::DEMO_CFG.lock().unwrap().iter() {
                                    if ui
                                        .button(format!(
                                            "{} {} {}",
                                            egui_phosphor::regular::TREE_PALM,
                                            egui_phosphor::regular::DOTS_SIX_VERTICAL,
                                            demo_name
                                        ))
                                        .on_hover_text(&*demo.description)
                                        .clicked()
                                    {
                                        self.submit_post_draw_cmd(GuiCmd::LoadCfgDemo(demo_name.into()));
                                    }
                                }
                            },
                        );

                        ui.separator();
                        if ui
                            .button(format!(
                                "{} {} Load config from file",
                                egui_phosphor::regular::FOLDER_OPEN,
                                egui_phosphor::regular::DOTS_SIX_VERTICAL
                            ))
                            .clicked()
                        {
                            self.file_picking_action_in_progress = FilePickingAction::Load;
                            self.file_dialog.pick_file();
                        }

                        ui.separator();
                        if ui
                            .button(format!(
                                "{} {} Save config as ...",
                                egui_phosphor::regular::FLOPPY_DISK,
                                egui_phosphor::regular::DOTS_SIX_VERTICAL
                            ))
                            .clicked()
                        {
                            self.file_picking_action_in_progress = FilePickingAction::Save;
                            self.file_dialog.save_file();
                        }
                        ui.separator();
                        ui.scope(|ui| {
                            if self.cfg.cfg_file.is_none() {
                                ui.disable();
                            }
                            if ui
                                .button(format!(
                                    "{} {} Save config as copy (with auto suffix)",
                                    egui_phosphor::regular::COPY,
                                    egui_phosphor::regular::DOTS_SIX_VERTICAL
                                ))
                                .clicked()
                            {
                                self.submit_post_draw_cmd(GuiCmd::SaveCfg(
                                    None,
                                    Some(format!(
                                        "copy.{}.yaml",
                                        chrono::Local::now().format("%Y-%m-%d-%H-%M-%S")
                                    )),
                                ));
                            }
                            ui.separator();
                            if ui
                                .button(format!(
                                    "{} {} Save current config (overwrite!)",
                                    egui_phosphor::regular::FLOPPY_DISK_BACK,
                                    egui_phosphor::regular::DOTS_SIX_VERTICAL
                                ))
                                .clicked()
                            {
                                self.submit_post_draw_cmd(GuiCmd::SaveCfg(None, None));
                            }
                        });
                        ui.separator();
                        if ui
                            .button(format!(
                                "{} {} Quit",
                                egui_phosphor::regular::DOOR_OPEN,
                                egui_phosphor::regular::DOTS_SIX_VERTICAL
                            ))
                            .clicked()
                        {
                            self.exit_app = true;
                        }
                    });
                    ui.separator();
                    ui.menu_button("View", |ui| {
                        ui.separator();
                        if ui.button("Use gray outlined theme").clicked() {
                            self.set_style(GUI_STYLE_GRAY_OUTLINED, ui);
                        }
                        ui.separator();
                        if ui.button("Use light theme").clicked() {
                            self.set_style(GUI_STYLE_LIGHT, ui);
                        }
                        ui.separator();
                        if ui.button("Use dark theme").clicked() {
                            self.set_style(GUI_STYLE_DARK, ui);
                        }
                        ui.separator();
                        if ui.button("Reset UI cache").clicked() {
                            ui.data_mut(|d| d.clear());
                        }
                    });
                    ui.separator();
                    ui.menu_button("Edit", |ui| {
                        self.draw_undo_redo_edit_menu(ui);
                    });

                    ui.separator();
                    self.draw_undo_redo_buttons(ui);
                    self.draw_and_handle_pending_changes_button(ui);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                        ui.separator();
                        ui.label(format!(
                            // crate::config::APP_LONG_NAME,
                            "{} <{}. v. {}> (about)",
                            egui_phosphor::fill::TREE_EVERGREEN,
                            crate::config::APP_NAME.to_uppercase(),
                            crate::config::APP_VERSION_STR
                        ))
                        .on_hover_ui(|ui| {
                            ui.vertical(|ui| {
                                ui.set_min_height(200.0);
                                ui.collapsing("Licensing information", |ui| {
                                    ui.collapsing("License", |ui| {
                                        ui.label(&*crate::config::LICENSE);
                                    });
                                    ui.separator();
                                    ui.collapsing("Third-party components licenses", |ui| {
                                        egui::ScrollArea::vertical()
                                            .show(ui, |ui| ui.label(&*crate::config::LICENSE_THIRDPARTY));
                                    });
                                });
                            });
                        });
                        ui.separator();
                        if !self.cfg.ui.monitors.is_empty() {
                            if !self.show_monitors {
                                if ui.button("Open monitoring overlays.").clicked() {
                                    self.show_monitors = true;
                                };
                            } else if ui.button("Close monitoring overlays.").clicked() {
                                self.show_monitors = false;
                            }
                        } else {
                            ui.label("No monitoring overlay configured.");
                        }
                    });
                    ui.spacing();
                    ui.separator();
                    ui.separator();
                });
            });

        // -----------------------------------------------------
        egui::Panel::bottom("status_panel")
            .frame(
                egui::Frame::default()
                    .inner_margin(12)
                    .outer_margin(0)
                    .fill(ui.visuals().window_fill),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.separator();
                    if self.cfg.global.mode.egui((), ui) {
                        self.submit_post_draw_cmd(GuiCmd::BaseTickRateChange);
                    }
                });
                ui.separator();
                ui.label(format!(
                    "Cfg title: {} | Cfg file: {} | Cfg edit epoch: {}",
                    self.cfg.title,
                    self.cfg
                        .cfg_file
                        .as_ref()
                        .and_then(|f| Some(f.to_string_lossy().to_string()))
                        .unwrap_or("no associated file".to_string()),
                    self.cfg.edit_epoch
                ));
            });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.separator();
                ui.selectable_value(
                    &mut self.current_opened_tab,
                    GuiMainTabs::ConfigDescription,
                    "Config Description",
                );
                ui.separator();
                ui.selectable_value(&mut self.current_opened_tab, GuiMainTabs::Devices, "Devices & Matchers");
                ui.separator();
                ui.selectable_value(
                    &mut self.current_opened_tab,
                    GuiMainTabs::Variables,
                    "Variables & Params",
                );
                ui.separator();
                ui.selectable_value(&mut self.current_opened_tab, GuiMainTabs::Mappings, "Mappings");
                ui.separator();
                ui.selectable_value(
                    &mut self.current_opened_tab,
                    GuiMainTabs::ConfigYaml,
                    "Config text view",
                );
                ui.separator();
                ui.selectable_value(&mut self.current_opened_tab, GuiMainTabs::Log, "Log");
                ui.separator();
                ui.selectable_value(
                    &mut self.current_opened_tab,
                    GuiMainTabs::RuntimeConfigState,
                    egui_phosphor::fill::BUG.to_string(),
                )
                .on_hover_text("Runtime debug info. User, do not enter :)!");
            });
            ui.separator();
            egui::ScrollArea::both()
                .scroll_source(ScrollSource::ALL)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.separator();
                    match self.current_opened_tab {
                        GuiMainTabs::Mappings => {
                            self.draw_mappings_editor_gui(ui);
                        }
                        GuiMainTabs::Devices => {
                            draw_collapsing_ui(
                                ui,
                                None::<()>,
                                Some(&format!(
                                    "{} HID: {}/{}/{}/{}/...",
                                    egui_phosphor::bold::DOTS_SIX_VERTICAL,
                                    egui_phosphor::bold::JOYSTICK,
                                    egui_phosphor::bold::GAME_CONTROLLER,
                                    egui_phosphor::bold::MOUSE,
                                    egui_phosphor::bold::KEYBOARD,
                                )),
                                |_| {},
                            )
                            .body(|ui| {
                                self.draw_device_and_matchers_hid(ui);
                            });
                            ui.separator();
                            #[cfg(feature = "midi")]
                            draw_collapsing_ui(
                                ui,
                                None::<()>,
                                Some(&format!(
                                    "{} MIDI: {}/{}/...",
                                    egui_phosphor::regular::DOTS_SIX_VERTICAL,
                                    egui_phosphor::regular::PIANO_KEYS,
                                    egui_phosphor::regular::FADERS,
                                )),
                                |_| {},
                            )
                            .body(|ui| {
                                self.draw_device_and_matchers_midi(ui);
                            });
                        }
                        GuiMainTabs::ConfigYaml => {
                            ui.label("Configuration YAML (select text and use Ctrl+C to copy):");
                            ui.separator();
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .scroll_source(ScrollSource::ALL)
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::from(&self.cfg_yaml).monospace());
                                });
                        }
                        GuiMainTabs::ConfigDescription => {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .scroll_source(ScrollSource::ALL)
                                .show(ui, |ui| {
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut self.cfg.title)
                                                .font(egui::TextStyle::Monospace)
                                                .interactive(true)
                                                .desired_width(f32::INFINITY)
                                                .hint_text("Set the config title here"),
                                        )
                                        .changed()
                                    {
                                        self.submit_post_draw_cmd(GuiCmd::ConfigChangeSimple);
                                    }
                                    ui.separator();
                                    if ui
                                        .add(
                                            egui::TextEdit::multiline(&mut self.cfg.description.0)
                                                .font(egui::TextStyle::Monospace)
                                                .interactive(true)
                                                .desired_width(f32::INFINITY)
                                                .hint_text(
                                                    "Please describe the details about this configuration here.",
                                                ),
                                        )
                                        .changed()
                                    {
                                        self.submit_post_draw_cmd(GuiCmd::ConfigChangeSimple);
                                    }
                                });
                        }
                        GuiMainTabs::RuntimeConfigState => {
                            self.draw_runtime_state_debug(ui);
                        }
                        GuiMainTabs::Log => {
                            egui_logger::logger_ui().show(ui);
                        }
                        GuiMainTabs::Variables => {
                            self.draw_variables(ui);
                        }
                    };
                });
        });

        self.process_post_draw_commands(ui);
        self.cfg_undo_checkpoint_try_finish(ui);

        let frame_time = start_time.elapsed();
        let target_ft = std::time::Duration::from_millis(16);
        if frame_time < target_ft {
            ui.ctx().request_repaint_after(target_ft - frame_time);
        } else {
            ui.ctx().request_repaint();
        }
    }
}

fn mapper_mode_to_icon(mode: MapperModeKind) -> &'static str {
    match mode {
        MapperModeKind::Reactive => egui_phosphor::regular::ACTIVITY,
        MapperModeKind::Capped => egui_phosphor::regular::WALL,
        MapperModeKind::Stable => egui_phosphor::regular::ANCHOR,
    }
}

fn mapper_mode_to_desc(mode: MapperModeKind) -> &'static str {
    match mode {
        MapperModeKind::Reactive => {
            "In Reactive mode all input events result in immediate execution of all the relevant mappings. \
            Use it for minimal response latency. Warning: high frequency input devices such as \
            gaming mice may cause higher CPU load. "
        }
        MapperModeKind::Capped => {
            "In Capped mode whenever input rate exceeds the configured threshold (except for button/key events), mapping gets delayed to match the \
            configured frequency ceiling. Mappings triggered by button/key events are executed immediately. \
            Any other input with rate lower than that is executed immediatly as in Reactive mode. \
            Relative deltas get accumulated until consumed by mappings run. \
            Remapping behavior of relative deltas is allowing extrapolation, clamping is not applied."
        }
        MapperModeKind::Stable => {
            "In Stable mode all the mappings except for those triggered by button/key events are executed with fixed configured frequency, \
            irrelevant to the frequency of input. Relative deltas get accumulated until consumed by mappings run. \
            Remapping behavior of relative deltas is allowing extrapolation, clamping is not applied. \
            Mappings triggered by button/key events are executed immediately."
        }
    }
}

fn mapper_mode_to_mapper_frequency_desc(mode: MapperModeKind) -> &'static str {
    match mode {
        MapperModeKind::Reactive => "Mappings are executed immediately",
        MapperModeKind::Capped => "Mapping frequency ceiling (except for button/key events)",
        MapperModeKind::Stable => "Stable mapping frequency (except for button/key events)",
    }
}

impl<'s> DrawEgui<'s> for MapperMode {
    type In = ();
    type Out = bool;

    fn egui(&mut self, _gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        let mut changed = false;
        ui.label("Current mode: ".to_string())
            .on_hover_text(mapper_mode_to_desc((&*self).into()));
        ui.label(mapper_mode_to_icon((&*self).into()))
            .on_hover_text(mapper_mode_to_desc((&*self).into()));
        ui.label(egui::RichText::new(self.to_string()).strong())
            .on_hover_text(mapper_mode_to_desc((&*self).into()));

        ui.separator();
        let base_desc = "Mappings scheduled for base tick run at this frequency irrespective to user input.";
        ui.label("base rate:").on_hover_text(base_desc);
        let mut base_rate = self.get_base_tick_rate();
        if ui
            .add(
                egui::Slider::new(
                    &mut base_rate,
                    crate::config::MIN_BASE_FREQ_HZ..=crate::config::MAX_BASE_FREQ_HZ,
                )
                .logarithmic(true)
                .suffix(" Hz"),
            )
            .on_hover_text(base_desc)
            .changed()
        {
            self.set_base_rate(base_rate);
            changed = true;
        };

        if let Some(mut mapping_rate) = self.get_mapping_rate() {
            ui.separator();
            ui.label(if self.is_capped() {
                "mapping rate limit:"
            } else {
                "mapping rate:"
            })
            .on_hover_text(mapper_mode_to_mapper_frequency_desc((&*self).into()));
            if ui
                .add(
                    egui::Slider::new(
                        &mut mapping_rate,
                        self.get_base_tick_rate()..=crate::config::MAX_BASE_FREQ_HZ,
                    )
                    .logarithmic(true)
                    .suffix(" Hz"),
                )
                .on_hover_text(mapper_mode_to_mapper_frequency_desc((&*self).into()))
                .changed()
            {
                self.set_mapping_rate(mapping_rate);
                changed = true;
            };
        }

        ui.separator();
        ui.label(" Switch to: ".to_string());
        if !self.is_reactive()
            && ui
                .button(mapper_mode_to_icon(MapperModeKind::Reactive))
                .on_hover_text(mapper_mode_to_desc(MapperModeKind::Reactive))
                .clicked()
        {
            self.make_reactive();
            changed = true;
        }
        if !self.is_capped()
            && ui
                .button(mapper_mode_to_icon(MapperModeKind::Capped))
                .on_hover_text(mapper_mode_to_desc(MapperModeKind::Capped))
                .clicked()
        {
            self.make_capped();
            changed = true;
        }

        if !self.is_stable()
            && ui
                .button(mapper_mode_to_icon(MapperModeKind::Stable))
                .on_hover_text(mapper_mode_to_desc(MapperModeKind::Stable))
                .clicked()
        {
            self.make_stable();
            changed = true;
        }

        if changed {
            self.sanitize_inplace(());
        }
        changed
    }
}

// ================================================================================
// ================================================================================
// ================================================================================
// ================================================================================

impl GuiMain {
    fn set_style(&mut self, style: GuiStyle, ui: &mut egui::Ui) {
        self.style = style;
        self.style.setup_egui_ctx(ui.ctx());
    }

    pub(super) fn reset_runtime_state(&mut self) {
        self.telemetry_graphs.borrow_mut().clear();
        self.transient_states_script_aux_edit.borrow_mut().clear();
        self.transient_states_variable_edit.clear();
        self.transient_states_device_key_edit.clear();
        self.pending_cmds.clear();
    }

    pub(super) fn fill_from_saved_state(&mut self, saved: &GuiMainSavedState) {
        self.style = saved.style.clone();
        self.current_opened_tab = saved.current_opened_tab;
        self.gui_tab_mappings_current_opened_mapping_idx = saved.gui_tab_mappings_current_opened_mapping_idx;
        self.show_monitors = saved.show_monitors;
    }

    pub(super) fn save_state(&mut self, storage: &mut dyn eframe::Storage) {
        let saved: GuiMainSavedState = self.into();
        // dbg!("SAVING STATE!");
        // dbg!(&saved);
        eframe::set_value(storage, eframe::APP_KEY, &saved);
        storage.flush();
    }

    pub(super) fn get_config_string(&self) -> String {
        self.cfg.get_yaml_str()
    }

    pub(super) fn update_cfg_yaml(&mut self) {
        self.cfg_yaml = self.get_config_string();
    }

    fn send_driver_cmd(&self, cmd: DriverCmd) {
        Self::send_driver_cmd_static(&self.driver_tx, cmd);
    }

    fn send_driver_cmd_static(driver_tx: &UnboundedSender<DriverCmd>, cmd: DriverCmd) {
        let res = driver_tx
            .send(cmd)
            .inspect_err(|e| log::error!("Failed to send command to driver: {e}"));
        #[cfg(debug_assertions)]
        res.expect("Failed to send command to driver");
    }

    pub(super) fn execute_gui_command(&mut self, mut gui_cmd: GuiCmd, ui: &mut egui::Ui) -> Result<(), String> {
        const RX_TIMEOUT_MS: u64 = 5000;

        match &mut gui_cmd {
            GuiCmd::ScriptAuxRename(cmd) => {
                if self.cfg.traverse_mut(cmd).is_break() {
                    return Err(format!(
                        "Script aux data rename failed: name {} is already used.",
                        cmd.new_key
                    ));
                };

                self.send_driver_cmd(DriverCmd::ChangeConfigSimple { cfg: self.cfg.clone() });
            }
            GuiCmd::DeviceMatcherRename(cmd) => {
                if cmd.old_key == cmd.new_key || cmd.new_key.is_empty() {
                    return Err(format!(
                        "Can't rename device matcher {} to {} due to names conflict",
                        cmd.old_key, cmd.new_key
                    ));
                }
                let mut conflict = self.cfg.devices.hid.contains_key(&cmd.new_key);
                #[cfg(feature = "midi")]
                {
                    conflict |= self.cfg.devices.midi.contains_key(&cmd.new_key);
                }
                if conflict {
                    return Err(format!("Device key '{}' already exists. Rename aborted.", cmd.new_key));
                }
                if cmd.is_hid
                    && let Some(device_cfg) = self.cfg.devices.hid.remove(&cmd.old_key)
                {
                    self.cfg.devices.hid.insert(cmd.new_key.clone(), device_cfg);
                } else {
                    #[cfg(feature = "midi")]
                    if let Some(device_cfg) = self.cfg.devices.midi.remove(&cmd.old_key) {
                        self.cfg.devices.midi.insert(cmd.new_key.clone(), device_cfg);
                    }
                }
                let _ = self.cfg.traverse_mut(cmd);
                self.execute_gui_command(GuiCmd::ConfigChangeSimple, ui)?;
                if cmd.is_virtual {
                    self.execute_gui_command(
                        GuiCmd::ChangeVirtualHid(GuiCmdChangeVirtualHid {
                            virtual_hid_key: cmd.old_key.clone(), /* TODO: do not restart, just reassociate they
                                                                  key to already opened device within the HID manager*/
                        }),
                        ui,
                    )?;
                }
            }
            GuiCmd::DeviceMatcherRemove(cmd) => {
                if self.cfg.traverse_mut(cmd).is_break() {
                    return Err(format!(
                        "Device '{}' is referenced in mappings. Removal skipped.",
                        cmd.device_key
                    ));
                }
                self.cfg.devices.hid.remove(&cmd.device_key);
                #[cfg(feature = "midi")]
                self.cfg.devices.midi.remove(&cmd.device_key);
                self.cfg.sanitize_inplace(());
                self.execute_gui_command(GuiCmd::ConfigChangeSimple, ui)?;
                if cmd.is_virtual {
                    self.execute_gui_command(
                        GuiCmd::ChangeVirtualHid(GuiCmdChangeVirtualHid {
                            virtual_hid_key: cmd.device_key.clone(),
                        }),
                        ui,
                    )?;
                }
            }
            GuiCmd::ControlMatcherChange(cmd) => {
                let _ = self.cfg.traverse_mut(cmd);
                self.cfg.sanitize_inplace(());
                self.send_driver_cmd(DriverCmd::ChangeConfigSimple { cfg: self.cfg.clone() });
            }
            GuiCmd::VariableChange(cmd) => {
                if cmd.old_key != cmd.new_key && self.cfg.variables.contains_key(&cmd.new_key) {
                    return Err(format!("Variable name {} is already used.", cmd.new_key));
                }
                self.cfg
                    .variables
                    .insert(cmd.new_key.clone(), cmd.new_definition.clone());
                let _ = self.cfg.traverse_mut(cmd);
                if cmd.old_key != cmd.new_key {
                    self.cfg.variables.remove(&cmd.old_key);
                }
                self.send_driver_cmd(DriverCmd::ChangeConfigSimple { cfg: self.cfg.clone() });
            }
            GuiCmd::ControlMatcherRemove(cmd) => {
                if self.cfg.traverse_mut(cmd).is_break() {
                    return Err(format!(
                        "Control '{}' in device '{}' is referenced in mappings. Removal skipped.",
                        cmd.control_key, cmd.device_key
                    ));
                }
                if let Some(dev) = self.cfg.devices.hid.get_mut(&cmd.device_key) {
                    dev.controls.remove(&cmd.control_key);
                } else {
                    #[cfg(feature = "midi")]
                    if let Some(dev) = self.cfg.devices.midi.get_mut(&cmd.device_key) {
                        dev.controls.remove(&cmd.control_key);
                    }
                }
                self.cfg.sanitize_inplace(());
                self.send_driver_cmd(DriverCmd::ChangeConfigSimple { cfg: self.cfg.clone() });
            }

            GuiCmd::VariableRemove(cmd) => {
                if self.cfg.traverse_mut(cmd).is_break() {
                    return Err(format!(
                        "Variable '{}' is referenced in mappings. Removal skipped.",
                        cmd.variable_key
                    ));
                }
                self.cfg.variables.remove(&cmd.variable_key);
                self.cfg.sanitize_inplace(());
                self.send_driver_cmd(DriverCmd::ChangeConfigSimple { cfg: self.cfg.clone() });
            }
            GuiCmd::MappingChange(action) => self.send_driver_cmd(DriverCmd::ChangeMappings {
                mappings: Some(self.cfg.mappings.clone()),
                action: action.clone(),
            }),
            GuiCmd::ConfigChangeMiscNoUndoCheckpoint | GuiCmd::ConfigChangeSimple => {
                self.send_driver_cmd(DriverCmd::ChangeConfigSimple { cfg: self.cfg.clone() })
            }
            GuiCmd::ConfigChangeDriverRestart => {
                self.execute_gui_command(GuiCmd::ConfigChangeSimple, ui)?;
                let (tx, rx) = std::sync::mpsc::channel::<()>();
                self.send_driver_cmd(DriverCmd::Reload { sync_tx: Some(tx) });
                let _ = rx
                    .recv()
                    .inspect_err(|e| log::error!("Failed to wait for sync from driver after driver reload: {e}"));
            }
            GuiCmd::DragAndDrop(GuiDndJob::MoveTfmStep(dnd_job)) => {
                let mut dnd_visitor = DndJobMoveTfmStep_Visitor {
                    dnd_job: dnd_job.clone(),
                    dropped_tfm_step: None,
                };
                for i in 0..=1 {
                    if self.cfg.mappings.traverse_mut(&mut dnd_visitor) == ControlFlow::Break(()) {
                        break;
                    } else if i == 1 {
                        log::error!("Dnd job failed to complete in 2 traversals. Error in implementation. {dnd_job:?}");
                    }
                }
                self.execute_gui_command(GuiCmd::ConfigChangeSimple, ui)?;
            }
            GuiCmd::DragAndDrop(..) | GuiCmd::LocalItemRemove(..) => {
                log::error!(
                    "Drag and drop and local item removal commands are expected to be \
                      handled on upper level in Gui. Error in implementation.
                    Command failed:
                      {gui_cmd:?}"
                )
            }
            GuiCmd::ChangeVirtualHid(GuiCmdChangeVirtualHid { virtual_hid_key }) => {
                self.cfg.sanitize_inplace(());
                self.avail_dev_info_cache.clear();
                let (tx, rx) = std::sync::mpsc::channel::<()>();
                self.send_driver_cmd(DriverCmd::ChangeVirtualHid {
                    cfg: self.cfg.clone(),
                    virtual_hid_key: virtual_hid_key.clone(),
                    sync_tx: tx,
                });
                let _ = rx
                    .recv_timeout(Duration::from_millis(RX_TIMEOUT_MS))
                    .inspect_err(|e| log::error!("{e}"));
            }
            GuiCmd::LoadCfgDemo(..) | GuiCmd::LoadCfgFile(..) | GuiCmd::LoadCfgBlank() => {
                self.avail_dev_info_cache.clear();

                let (resp_tx, resp_rx) = std::sync::mpsc::channel::<Result<Config, String>>();
                self.send_driver_cmd(DriverCmd::LoadCfg {
                    cfg_source: match gui_cmd {
                        GuiCmd::LoadCfgDemo(demo_name) => crate::driver::LoadCfgSource::Demo(demo_name.clone()),
                        GuiCmd::LoadCfgFile(path) => crate::driver::LoadCfgSource::File(path.clone()),
                        GuiCmd::LoadCfgBlank() => crate::driver::LoadCfgSource::Blank(),
                        _ => unreachable!(),
                    },
                    cfg_tx: resp_tx,
                });

                match resp_rx.recv_timeout(Duration::from_millis(RX_TIMEOUT_MS)) {
                    Ok(Ok(new_cfg)) => {
                        self.cfg = new_cfg;
                        self.update_cfg_yaml();
                        self.pending_cmds.clear();
                        self.cfg_undo_reset(ui);
                    }
                    Ok(Err(e)) => {
                        return Err(format!("Failed to load config: {e:?}"));
                    }
                    Err(e) => {
                        return Err(format!("Failed to receive opened config from driver: {e:?}"));
                    }
                }
            }
            GuiCmd::SaveCfg(cfg_file, cfg_suffix) => {
                self.send_driver_cmd(DriverCmd::SaveCfg {
                    cfg_file: cfg_file.clone(),
                    cfg_suffix: cfg_suffix.clone(),
                });
            }
            GuiCmd::BaseTickRateChange => self.send_driver_cmd(DriverCmd::ChangeMappingMode {
                mode: self.cfg.global.mode,
            }),
            GuiCmd::CmdSeqence(gui_cmds) => {
                let mut res = None;
                for cmd in gui_cmds.drain(..) {
                    if let GuiCmd::BreakOnErr = cmd {
                        if let Some(Err(_)) = res.as_ref() {
                            log::info!("Gui command sequence execution break.");
                            return res.unwrap();
                        }
                    } else {
                        res = Some(self.execute_gui_command(cmd, ui).inspect_err(|e| warn!("{e:?}")));
                    }
                }
            }
            GuiCmd::BreakOnErr => {}
            GuiCmd::SubmitPending(gui_cmd) => self.submit_pending_cmd(*(*gui_cmd).clone(), ui),
        }

        Ok(())
    }

    fn process_post_draw_commands(&mut self, ui: &mut egui::Ui) {
        if self.post_draw_cmds.is_empty() {
            return;
        }

        if !self.post_draw_cmds.get(0).is_some_and(|cmd| {
            matches!(
                cmd,
                PendingCommand {
                    cmd: GuiCmd::ConfigChangeMiscNoUndoCheckpoint
                }
            )
        }) {
            self.cfg_undo_checkpoint_start(ui);
        }

        if MORE_DEBUG {
            for cmd in self.post_draw_cmds.iter() {
                log::info!("Applying post-draw command {:?}", cmd.cmd);
            }
        }

        let mut res = None;

        for cmd in self.post_draw_cmds.clone() {
            if let GuiCmd::BreakOnErr = cmd.cmd {
                if let Some(Err(_)) = res.as_ref() {
                    break;
                }
            } else {
                res = Some(self.execute_gui_command(cmd.cmd, ui).inspect_err(|e| warn!("{e:?}")));
            }
        }

        self.post_draw_cmds.clear();
        self.update_cfg_yaml();
    }

    pub(super) fn process_pending_commands(&mut self, ui: &mut egui::Ui) {
        if self.pending_cmds.is_empty() {
            return;
        }

        if MORE_DEBUG {
            for pending in self.pending_cmds.iter() {
                log::info!("Applying pending command {:?}", pending.cmd);
            }
        }

        self.pending_cmds.dedup_by(|next, prev| *next == *prev);

        self.store_applied_pending_commands_for_undo(ui);

        let mut res = None;
        for cmd in &self.pending_cmds.clone() {
            if let GuiCmd::BreakOnErr = cmd.cmd {
                if let Some(Err(_)) = res.as_ref() {
                    break;
                }
            } else {
                res = Some(self.execute_gui_command(cmd.cmd.clone(), ui));
            }
        }

        self.pending_cmds.clear();
        self.update_cfg_yaml();
    }

    #[must_use]
    fn draw_key_edit_gui(
        transient_states_device_key_edit: &mut HashMap<String, String>,
        dmk: &String,
        is_virtual: bool,
        ui: &mut egui::Ui,
    ) -> Option<GuiCmd> {
        let mut gui_out = None;
        let is_editing = transient_states_device_key_edit.contains_key(dmk);
        if !is_editing {
            if ui
                .button(egui_phosphor::fill::IDENTIFICATION_BADGE.to_string())
                .on_hover_text("Edit config key name")
                .clicked()
            {
                transient_states_device_key_edit.insert(dmk.clone(), dmk.clone());
            }
        } else {
            let mut pending_change_to_submit = None;
            let new_key = transient_states_device_key_edit.get_mut(dmk).unwrap();

            ui.horizontal(|ui| {
                if ui.button(egui_phosphor::bold::CHECK_FAT.to_string()).clicked() {
                    pending_change_to_submit = Some(GuiCmdDeviceKeyRename {
                        old_key: dmk.clone(),
                        new_key: new_key.clone(),
                        is_hid: true,
                        is_virtual,
                    });
                }
                ui.text_edit_singleline(new_key);
            });

            if let Some(pending_change_to_submit) = pending_change_to_submit {
                gui_out = Some(GuiCmd::DeviceMatcherRename(pending_change_to_submit.clone()));
                transient_states_device_key_edit.remove(&pending_change_to_submit.old_key);
            }
        }
        gui_out
    }

    #[must_use]
    fn draw_device_remove_button(dmk: &str, is_virtual: bool, ui: &mut egui::Ui) -> Option<GuiCmd> {
        if ui
            .small_button(egui_phosphor::fill::TRASH.to_string())
            .on_hover_text("Try to remove (will be removed if not referenced)")
            .clicked()
        {
            Some(GuiCmd::DeviceMatcherRemove(GuiCmdDeviceMatcherRemove {
                device_key: dmk.to_owned(),
                is_virtual,
            }))
        } else {
            None
        }
    }

    // ================================================================================

    #[cfg(feature = "midi")]
    fn draw_device_and_matchers_midi(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        draw_collapsing_ui(ui, None::<()>, Some("Available"), |_| {}).body(|ui| {
            for (ad_idx, ad) in self.avail_dev_info_cache.midi(&self.midi_mgr).iter().enumerate() {
                ui.separator();
                ui.collapsing(format!("({}) {}", ad_idx, ad.name), |ui| {
                    ad.egui_immutable((), ui);
                });
            }
        });

        ui.separator();

        Self::draw_devices_editor(
            &mut self.cfg.devices.midi,
            self.avail_dev_info_cache.midi(&self.midi_mgr),
            &mut self.transient_states_device_key_edit,
            false,
            "Matchers",
            "Add MIDI device matcher",
            "MIDI device matcher",
            |_: &str| -> MidiMatcherCfg { MidiMatcherCfg::default() } as fn(&str) -> MidiMatcherCfg,
            |_: &MidiMatcherCfg| -> bool { true } as fn(&MidiMatcherCfg) -> bool,
            ui,
        )
        .map(|out| self.submit_post_draw_cmd(out));
    }

    pub(crate) fn draw_available_devices<AvailDevIterT, AvailDevIterItemT, AvailDevIterItemDerefTargetT>(
        label_matched: &str,
        label_matched_none: &str,
        available_devices: AvailDevIterT,
        filterer: &impl MatchesAvailableDeviceIface,
        ui: &mut egui::Ui,
    ) where
        AvailDevIterT: Iterator<Item = AvailDevIterItemT>,
        AvailDevIterItemT: Deref<Target = AvailDevIterItemDerefTargetT>,
        AvailDevIterItemDerefTargetT: for<'a> DrawEgui<'a, In = ()> + AvailableDeviceInfoIface,
    {
        let mut mads = available_devices
            .filter(|ad| filterer.matches_available_device(&**ad))
            .collect::<Vec<_>>();
        if mads.is_empty() {
            ui.label(label_matched_none);
        } else {
            ui.collapsing(label_matched, |ui| {
                mads.iter_mut().enumerate().for_each(|(ad_idx, ad)| {
                    ui.collapsing(format!("({}) {}", ad_idx, ad.get_name().to_owned()), |ui| {
                        ad.egui_immutable((), ui);
                    });
                });
            });
        }
    }

    // ================================================================================
    // ================================================================================
    // ================================================================================
    // ================================================================================

    fn draw_device_and_matchers_hid(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        draw_collapsing_ui(ui, None::<()>, Some("Available"), |_| {}).body(|ui| {
            for (ad_idx, ad) in self.avail_dev_info_cache.hid(&self.hid_mgr).iter().enumerate() {
                ui.separator();
                ui.collapsing(format!("({}) {}", ad_idx, ad.name), |ui| {
                    ad.egui_immutable((), ui);
                });
            }
        });

        ui.separator();

        // __________________________________________________
        Self::draw_devices_editor(
            &mut self.cfg.devices.hid,
            self.avail_dev_info_cache.hid(&self.hid_mgr),
            &mut self.transient_states_device_key_edit,
            true,
            "Virtual",
            "add virtual device",
            "Virtual HID",
            HidDeviceCfg::new_virtual as fn(&str) -> HidDeviceCfg,
            |d: &HidDeviceCfg| -> bool { d.is_a_virtual() } as fn(&HidDeviceCfg) -> bool,
            ui,
        )
        .map(|out| self.submit_post_draw_cmd(out));

        ui.separator();

        Self::draw_devices_editor(
            &mut self.cfg.devices.hid,
            self.avail_dev_info_cache.hid(&self.hid_mgr),
            &mut self.transient_states_device_key_edit,
            false,
            "Matchers",
            "Add HID matcher",
            "HID matcher",
            |_: &str| -> HidDeviceCfg { HidDeviceCfg::new_matcher() } as fn(&str) -> HidDeviceCfg,
            |d: &HidDeviceCfg| -> bool { !d.is_a_virtual() } as fn(&HidDeviceCfg) -> bool,
            ui,
        )
        .map(|out| self.submit_post_draw_cmd(out));
    }

    #[must_use]
    fn draw_devices_editor<'s, DeviceCfgT, AvailDeviceInfoT>(
        devices: &mut std::collections::BTreeMap<String, DeviceCfgT>,
        available_devices: &[AvailDeviceInfoT],
        transient_states_device_key_edit: &mut HashMap<String, String>,
        drawing_virtual: bool,
        title: &str,
        on_hover_text: &str,
        new_name_prefix: &str,
        new_device_fn: fn(&str) -> DeviceCfgT,
        device_filter: fn(&DeviceCfgT) -> bool,
        ui: &mut egui::Ui,
    ) -> Option<GuiCmd>
    where
        AvailDeviceInfoT: 's + AvailableDeviceInfoIface,
        for<'a> DeviceCfgT: DeviceCfgCommonIface
            + DrawEgui<'a, In = (&'a [AvailDeviceInfoT], GuiInDeviceCfg<'a>), Out = Option<GuiCmd>>,
    {
        let mut gui_out = None;
        let mut changed_virtual_device_cfg_key = None;
        let gui_out_mut = &mut gui_out;

        draw_collapsing_ui(ui, None::<()>, Some(title), |ui| {
            ui.separator();

            if ui
                .button(egui_phosphor::bold::LIST_PLUS.to_string())
                .on_hover_text(on_hover_text)
                .clicked()
            {
                let dk = get_item_name_with_random_suffix(new_name_prefix, devices.len() + 1);
                devices.insert(dk.clone(), new_device_fn(dk.as_str()));
                *gui_out_mut = Some(GuiCmd::ConfigChangeSimple);
            }
        })
        .body(|ui| {
            if !devices.is_empty() {
                let keys: Vec<String> = devices
                    .iter()
                    .filter(|(_, device_cfg)| device_filter(device_cfg))
                    .map(|(device_cfg_key, _)| device_cfg_key.clone())
                    .collect();

                for cfg_key in keys.into_iter() {
                    ui.separator();
                    draw_collapsing_ui(ui, None::<()>, Some(&cfg_key), |ui| {
                        let device = &mut *devices.get_mut(&cfg_key).unwrap();
                        ui.separator();
                        Self::draw_key_edit_gui(transient_states_device_key_edit, &cfg_key, true, ui)
                            .and_then(|out| gui_out_mut.replace(out));
                        ui.separator();
                        ui.label(format!("classification: {}", device.get_classification()))
                            .on_hover_text("Based on controls configured");
                        ui.separator();
                        if ui.checkbox(device.is_enabled_mut(), "Enabled").changed() {
                            *gui_out_mut = Some(GuiCmd::ConfigChangeDriverRestart);
                        };

                        if let Some(persistent) = device.is_persistent_mut() {
                            ui.separator();
                            if ui.checkbox(persistent, "Persistent").changed() {
                                *gui_out_mut = Some(GuiCmd::ConfigChangeSimple);
                            };
                        }

                        ui.separator();
                        Self::draw_device_remove_button(&cfg_key, true, ui).and_then(|out| gui_out_mut.replace(out));
                    })
                    .body(|ui| {
                        let device = devices.get_mut(&cfg_key).unwrap();
                        device
                            .egui((available_devices, GuiInDeviceCfg::Edit { device_key: &cfg_key }), ui)
                            .map(|out| {
                                device.update_classification(); // TODO?: move to sanitize?
                                gui_out_mut.replace(out);
                            });
                    });

                    if drawing_virtual && gui_out_mut.is_some() && changed_virtual_device_cfg_key.is_none() {
                        changed_virtual_device_cfg_key = Some(cfg_key.clone());
                    }
                }
            }
        });

        if let Some(out) = gui_out_mut
            && let Some(changed_virtual_device_cfg_key) = changed_virtual_device_cfg_key
        {
            let tmp = Some(GuiCmd::CmdSeqence(vec![
                out.clone(),
                GuiCmd::BreakOnErr,
                GuiCmd::ChangeVirtualHid(GuiCmdChangeVirtualHid {
                    virtual_hid_key: changed_virtual_device_cfg_key.clone(),
                })
                .into(),
            ]));
            if !matches!(
                *out,
                GuiCmd::DeviceMatcherRename(..) /* TODO: control matcher renaming, when added, goes here to */
            ) {
                *gui_out_mut = tmp.map(|out| GuiCmd::SubmitPending(Box::new(out)));
            } else {
                *gui_out_mut = tmp;
            }
        }

        gui_out
    }

    fn draw_variables(&mut self, ui: &mut egui::Ui) {
        let mut variable_edit_to_submit = None;
        let mut variable_remove_to_submit = None;
        if ui
            .button(egui_phosphor::bold::LIST_PLUS.to_string())
            .on_hover_text("add variable")
            .clicked()
        {
            self.cfg.variables.insert(
                get_item_name_with_random_suffix("New Variable", self.cfg.variables.len() + 1),
                VariableCfg::default(),
            );
            self.submit_post_draw_cmd(GuiCmd::ConfigChangeSimple);
        }

        ui.separator();
        for v in &mut self.cfg.variables {
            ui.separator();

            ui.horizontal(|ui| {
                if self.transient_states_variable_edit.contains_key(v.0) {
                    let data = self.transient_states_variable_edit.get_mut(v.0).unwrap();
                    if ui
                        .button(egui_phosphor::bold::CHECK_FAT.to_string())
                        .on_hover_text("Submit")
                        .clicked()
                    {
                        variable_edit_to_submit = Some(GuiCmdVariableChange {
                            old_key: v.0.into(),
                            new_key: data.edited_key.clone(),
                            new_definition: data.edited_definition.clone(),
                        });
                    }

                    ui.text_edit_singleline(&mut data.edited_key);
                    data.edited_definition.egui(GuiInKinds::Edit, ui);
                } else {
                    if ui
                        .button(egui_phosphor::bold::PENCIL.to_string())
                        .on_hover_text("Edit")
                        .clicked()
                    {
                        let _ = self.transient_states_variable_edit.insert(
                            v.0.to_string(),
                            GuiTransientStateItemEdit {
                                edited_key: v.0.clone(),
                                edited_definition: v.1.clone(),
                            },
                        );
                    }
                    ui.separator();
                    ui.label(egui::RichText::new(v.0).strong().monospace());
                    ui.separator();
                    if v.1.egui(GuiInKinds::Display, ui) {
                        variable_edit_to_submit = Some(GuiCmdVariableChange {
                            old_key: v.0.into(),
                            new_key: v.0.into(),
                            new_definition: v.1.clone(),
                        });
                    }
                }
                ui.separator();
                if ui
                    .button(egui_phosphor::bold::TRASH.to_string())
                    .on_hover_text("Remove (will not apply if referenced in mappings)")
                    .clicked()
                {
                    variable_remove_to_submit = Some(GuiCmdVariableRemove {
                        variable_key: v.0.clone(),
                    });
                }
            });
        }

        if let Some(variable_edit_to_submit) = variable_edit_to_submit {
            self.submit_post_draw_cmd(GuiCmd::VariableChange(variable_edit_to_submit.clone()));
            self.transient_states_variable_edit
                .remove(&variable_edit_to_submit.old_key);
        } else if let Some(variable_remove_to_submit) = variable_remove_to_submit {
            self.submit_post_draw_cmd(GuiCmd::VariableRemove(variable_remove_to_submit));
        }
    }

    fn draw_and_handle_pending_changes_button(&mut self, ui: &mut egui::Ui) {
        if self.pending_cmds.is_empty() {
            return;
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} apply pending", egui_phosphor::bold::CHECK_SQUARE))
                .highlight()
                .clicked()
            {
                self.process_pending_commands(ui);
            }
            // ui.separator();
            // if ui
            //     .button(format!("{} cancel pending", egui_phosphor::bold::X_SQUARE))
            //     .highlight()
            //     .clicked()
            // {
            //     if MORE_DEBUG {
            //         for pending_change in self.pending_cmds.drain(..) {
            //             log::info!("Dropping pending change {pending_change:?}");
            //         }
            //     } else {
            //         self.pending_cmds.clear();
            //     }
            // }
        });
    }

    fn draw_runtime_state_debug(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Monitors Ui cfg:", |ui| {
            for m in self.cfg.ui.monitors.iter().enumerate() {
                match m.1 {
                    UiMonitorsCfg::Axis(ui_axis_monitor_cfg) => {
                        ui.collapsing(format!("{} {}", m.0, ui_axis_monitor_cfg.name), |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .scroll_source(ScrollSource::ALL)
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::from(format!("{:#?}", ui_axis_monitor_cfg)).monospace());
                                });
                        });
                    }
                }
            }
        });
        ui.separator();
        ui.collapsing("Mappings state:", |ui| {
            for m in self.cfg.mappings.iter().enumerate() {
                ui.collapsing(format!("{} {}", m.0, m.1.name), |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .scroll_source(ScrollSource::ALL)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::from(format!("{m:#?}")).monospace());
                        });
                });
            }
        });
        ui.separator();
        ui.collapsing("Device and control matchers state:", |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .scroll_source(ScrollSource::ALL)
                .show(ui, |ui| {
                    ui.label(egui::RichText::from(format!("{:#?}", self.cfg.devices)).monospace());
                });
        });
        ui.separator();
        ui.collapsing("Global params:", |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .scroll_source(ScrollSource::ALL)
                .show(ui, |ui| {
                    ui.label(egui::RichText::from(format!("{:#?}", self.cfg.global)).monospace());
                });
        });
        ui.separator();
        ui.collapsing("Pre-configured (predefined) controls configuration (static):", |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .scroll_source(ScrollSource::ALL)
                .show(ui, |ui| {
                    ui.label(egui::RichText::from(format!("{:#?}", crate::config::PREDEF_CONTROLS)).monospace());
                });
        });
    }

    pub(super) fn submit_pending_cmd(&mut self, cmd: GuiCmd, ui: &mut egui::Ui) {
        self.pending_cmds.push(PendingCommand { cmd });
        self.cfg_undo_checkpoint_start(ui);
    }

    pub(super) fn submit_post_draw_cmd(&mut self, cmd: GuiCmd) {
        self.post_draw_cmds.push(PendingCommand { cmd })
    }
}
