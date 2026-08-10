use crate::config::MORE_DEBUG;
use crate::debug::DebugLevel;
use crate::device_and_device_manager::DeviceKind;
use crate::device_and_device_manager::DeviceManagerCommon;
use crate::device_and_device_manager::WithDeviceClassification;
use crate::driver::DriverCmd;
use crate::gui_common::{
    DrawEgui, GuiCmd, GuiCmdVariableChange, GuiCmdVariableRemove, GuiCmdVirtualDeviceChange, GuiInKinds, ScriptAuxKind,
    draw_collapsing_ui, get_item_name_with_random_suffix,
};
use crate::gui_common::{GuiCmdDeviceKeyRename, GuiCmdDeviceMatcherRemove};
use crate::gui_device::GuiInDeviceCfg;
use crate::gui_telemetry_graph::GuiTelemetryGraphStates;
use crate::hid_manager::{AvailableHIDDeviceInfo, HidManager};
#[cfg(feature = "midi")]
use crate::midi::{AvailableMidiDeviceInfo, MidiManager};
use crate::schemas_cfg::Config;
use crate::schemas_common::ObjId;
use crate::schemas_hid::HidDeviceCfg;
#[cfg(feature = "midi")]
use crate::schemas_midi::MidiMatcherCfg;
use crate::schemas_ui::UiMonitorsCfg;
use crate::schemas_value::VariableState;
use eframe::egui::{self};
use egui::Color32;
use egui_file_dialog::FileDialog;
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::f32;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use traversable::TraversableMut;
use unchecked_refcell::UncheckedRefCell;
use winit::platform::wayland::EventLoopBuilderExtWayland;
use winit::platform::x11::EventLoopBuilderExtX11;

#[allow(unused)]
fn get_visuals_high_contrast_light1() -> egui::Visuals {
    let mut visuals = egui::Visuals::light();

    // Force pure black and white
    visuals.override_text_color = Some(egui::Color32::BLACK);
    visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::BLACK;
    visuals.widgets.inactive.fg_stroke.color = egui::Color32::BLACK;
    visuals.widgets.hovered.fg_stroke.color = egui::Color32::BLACK;
    visuals.widgets.active.fg_stroke.color = egui::Color32::BLACK;
    visuals.widgets.open.fg_stroke.color = egui::Color32::BLACK;

    // Set background and fill contrasts
    visuals.panel_fill = egui::Color32::WHITE;
    visuals.widgets.noninteractive.bg_fill = egui::Color32::WHITE;
    visuals.widgets.inactive.bg_fill = egui::Color32::WHITE;
    visuals.widgets.hovered.bg_fill = egui::Color32::WHITE;
    visuals.widgets.active.bg_fill = egui::Color32::WHITE;
    visuals.widgets.open.bg_fill = egui::Color32::WHITE;

    visuals
}

//====================================================================================
#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
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
                hardware_acceleration: eframe::HardwareAcceleration::Preferred,
                persist_window: true,
                // persistence_path: Some({
                //     let mut p = std::env::temp_dir().to_path_buf(); p.push("mmvj"); p}),
                ..Default::default()
            },
            Box::new(move |cc| {
                // -----------------------------------------------------
                cc.egui_ctx.set_theme(egui::Theme::from_dark_mode(false));
                // cc.egui_ctx
                //     .all_styles_mut(|s| s.visuals = get_visuals_high_contrast_light1());
                cc.egui_ctx.set_pixels_per_point(1.2);
                // cc.egui_ctx.all_styles_mut(|s| s.visuals = get_visuals_w95_1());

                let mut fonts = egui::FontDefinitions::default();
                egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
                cc.egui_ctx.set_fonts(fonts);
                // -----------------------------------------------------

                let mut app = GuiMain {
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
                    available_hid: None,
                    #[cfg(feature = "midi")]
                    available_midi: None,
                };
                app.cfg_yaml = app.get_config_string();
                if let Some(storage) = cc.storage {
                    log::info!("Persistent storage is available ... ");
                    let saved: Option<GuiMainSavedState> = eframe::get_value(storage, eframe::APP_KEY);
                    if let Some(saved) = saved {
                        // dbg!(&saved);
                        log::info!("Restoring Gui state ... ");
                        app.fill_saved_state(&saved);
                        log::info!("Done restoring Gui state ... ");
                    }
                } else {
                    log::warn!("Persistent storage is not accessible, will not restore Gui state.");
                }
                app.show_monitors |= monitors_only;
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

struct GuiTransientStateItemEdit<T> {
    edited_key: String,
    edited_definition: T,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct GuiMainSavedState {
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
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PendingCommand {
    cmd: GuiCmd,
    /* aux metadata for pending and postponed commands may go here */
}

pub(crate) struct GuiMain {
    exit_app: bool,
    // ---
    pending_cmds: Vec<PendingCommand>,
    post_draw_cmds: Vec<PendingCommand>,
    // ---
    transient_states_variable_edit: HashMap<String, GuiTransientStateItemEdit<VariableState>>,
    transient_states_device_key_edit: HashMap<String, String>,
    pub transient_states_script_aux_edit: UncheckedRefCell<HashMap<(ObjId, ScriptAuxKind), (String, String)>>,
    // ---
    file_dialog: FileDialog,
    file_picking_action_in_progress: FilePickingAction,
    // ---
    driver_tx: UnboundedSender<DriverCmd>,
    // ---
    current_opened_tab: GuiMainTabs,
    // ---
    pub(crate) gui_tab_mappings_current_opened_mapping_idx: usize,
    // -----------------------------------------
    pub(crate) telemetry_graphs: UncheckedRefCell<GuiTelemetryGraphStates>,
    // -----------------------------------------
    monitors_only: bool,
    show_monitors: bool,
    // -----------------------------------------
    pub(crate) cfg: Config,
    // -----------------------------------------
    cfg_yaml: String,
    //----------------------------------------------
    cancellation_token: CancellationToken,
    //----------------------------------------------
    #[cfg(feature = "midi")]
    midi_mgr: MidiManager,
    hid_mgr: HidManager,
    _running_virtual_hid: Option<Vec<AvailableHIDDeviceInfo>>,
    available_hid: Option<Vec<AvailableHIDDeviceInfo>>,
    #[cfg(feature = "midi")]
    available_midi: Option<Vec<AvailableMidiDeviceInfo>>,
}

// ================================================================================
// ================================================================================
// ================================================================================
// ================================================================================

#[allow(unused)]
pub fn get_visuals_w95_1() -> egui::Visuals {
    use egui::{Stroke, epaint::Shadow};

    let mut visuals = egui::Visuals::light();

    let w95_gray = Color32::from_rgb(192, 192, 192);
    let dark_gray = Color32::from_rgb(128, 128, 128);
    // let white = Color32::from_rgb(255, 255, 255);
    let black = Color32::from_rgb(0, 0, 0);

    visuals.dark_mode = false;
    visuals.window_fill = w95_gray;
    visuals.panel_fill = w95_gray;
    visuals.window_stroke = Stroke::new(1.0_f32, black);
    visuals.window_shadow = Shadow::NONE;
    visuals.window_corner_radius = 0.0.into();
    visuals.menu_corner_radius = 0.0.into();

    visuals.override_text_color = Some(black);

    visuals.widgets.noninteractive.bg_fill = w95_gray;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, dark_gray);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, black);

    visuals.widgets.inactive.bg_fill = w95_gray;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, dark_gray);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, black);

    visuals.widgets.hovered.bg_fill = w95_gray;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.1_f32, black);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.2_f32, black);

    visuals.widgets.active.bg_fill = w95_gray;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, black);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, black);

    visuals
}

impl eframe::App for GuiMain {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        fn do_save(app: &mut GuiMain, frame: &mut eframe::Frame) {
            if let Some(storage) = frame.storage_mut() {
                app.save_state(storage);
            }
        }

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
            .show_inside(ui, |ui| {
                egui::MenuBar::default().ui(ui, |ui| {
                    match self.file_picking_action_in_progress {
                        FilePickingAction::Load => {
                            self.file_dialog.update(ui.ctx());
                            if let Some(path) = self.file_dialog.take_picked() {
                                self.submit_post_draw_cmd(GuiCmd::LoadCfg(path));
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

                    ui.menu_button("File", |ui| {
                        ui.separator();
                        if ui
                            .button(format!(
                                "{} {} Load config",
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
                                "{} {} Save config as ..?",
                                egui_phosphor::regular::FLOPPY_DISK,
                                egui_phosphor::regular::DOTS_SIX_VERTICAL
                            ))
                            .clicked()
                        {
                            self.file_picking_action_in_progress = FilePickingAction::Save;
                            self.file_dialog.save_file();
                        }
                        ui.separator();
                        if ui
                            .button(format!(
                                "{} {} Save config as copy ...",
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
                    self.draw_and_handle_pending_changes_button(ui);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                        ui.separator();
                        ui.label(format!(
                            // crate::config::APP_LONG_NAME,
                            "{} <{}. v. {}>",
                            egui_phosphor::fill::TREE_EVERGREEN,
                            crate::config::APP_NAME.to_uppercase(),
                            crate::config::APP_VERSION_STR
                        ));
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
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.separator();
                    ui.label("Idle tick rate: ");
                    if ui
                        .add(
                            egui::Slider::new(
                                &mut self.cfg.global.idle_tick_rate,
                                crate::config::MIN_BASE_FREQ_HZ..=crate::config::MAX_BASE_FREQ_HZ,
                            )
                            .logarithmic(true)
                            .suffix(" Hz"),
                        )
                        .on_hover_text("When no user input engine runs at this base clock.")
                        .changed()
                    {
                        self.submit_post_draw_cmd(GuiCmd::IdleTickRateChange);
                    };
                });
                ui.separator();
                ui.label(format!(" Config: {}", self.cfg.cfg_file.to_string_lossy()));
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
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
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                match self.current_opened_tab {
                    GuiMainTabs::Mappings => {
                        ui.separator();
                        self.draw_mappings_editor_gui(ui);
                    }
                    GuiMainTabs::Devices => {
                        ui.separator();
                        ui.group(|ui| {
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
                        });
                    }
                    GuiMainTabs::ConfigYaml => {
                        ui.separator();
                        ui.group(|ui| {
                            ui.label("Configuration YAML (select text and use Ctrl+C to copy):");
                            ui.separator();
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.cfg_yaml)
                                        .font(egui::TextStyle::Monospace)
                                        .interactive(false)
                                        // .background_color(Color32::DARK_BLUE)
                                        // .text_color(Color32::GREEN)
                                        .desired_width(f32::INFINITY)
                                        .hint_text("Config in YAML format"),
                                )
                            });
                        });
                    }
                    GuiMainTabs::ConfigDescription => {
                        ui.separator();
                        ui.group(|ui| {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                if ui
                                    .add(
                                        egui::TextEdit::multiline(&mut self.cfg.description)
                                            .font(egui::TextStyle::Monospace)
                                            .interactive(true)
                                            // .background_color(Color32::DARK_BLUE)
                                            // .text_color(Color32::GREEN)
                                            .desired_width(f32::INFINITY)
                                            .hint_text("Please describe the details about this configuration here."),
                                    )
                                    .changed()
                                {
                                    self.submit_post_draw_cmd(GuiCmd::ConfigChangeSimple);
                                }
                            })
                        });
                    }
                    GuiMainTabs::RuntimeConfigState => {
                        ui.separator();
                        ui.group(|ui| {
                            self.draw_runtime_state_debug(ui);
                        });
                    }
                    GuiMainTabs::Log => {
                        egui_logger::logger_ui().show(ui);
                    }
                    GuiMainTabs::Variables => {
                        ui.separator();
                        ui.group(|ui| {
                            self.draw_variables(ui);
                        });
                    }
                };
            });
        });

        self.process_post_draw_commands();

        let frame_time = start_time.elapsed();
        let target_ft = std::time::Duration::from_millis(16);
        if frame_time < target_ft {
            ui.ctx().request_repaint_after(target_ft - frame_time);
        } else {
            ui.ctx().request_repaint();
        }
    }
}

// ================================================================================
// ================================================================================
// ================================================================================
// ================================================================================

impl GuiMain {
    pub(crate) fn fill_saved_state(&mut self, saved: &GuiMainSavedState) {
        self.current_opened_tab = saved.current_opened_tab;
        self.gui_tab_mappings_current_opened_mapping_idx = saved.gui_tab_mappings_current_opened_mapping_idx;
        self.show_monitors = saved.show_monitors;
    }

    pub(crate) fn save_state(&mut self, storage: &mut dyn eframe::Storage) {
        let saved: GuiMainSavedState = self.into();
        // dbg!("SAVING STATE!");
        // dbg!(&saved);
        eframe::set_value(storage, eframe::APP_KEY, &saved);
        storage.flush();
    }

    pub(crate) fn get_config_string(&self) -> String {
        self.cfg.get_yaml_str()
    }

    pub(crate) fn update_cfg_yaml(&mut self) {
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

    fn execute_gui_command(&mut self, mut gui_cmd: GuiCmd) -> Result<(), String> {
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
                self.execute_gui_command(GuiCmd::ConfigChangeSimple)?;
                if cmd.is_virtual {
                    self.execute_gui_command(GuiCmd::VirtualDeviceChange(GuiCmdVirtualDeviceChange {
                        restart_persistent: true,
                    }))?;
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
                self.cfg.recompute_mappings_metadata();
                self.execute_gui_command(GuiCmd::ConfigChangeSimple)?;
                if cmd.is_virtual {
                    self.execute_gui_command(GuiCmd::VirtualDeviceChange(GuiCmdVirtualDeviceChange {
                        restart_persistent: true,
                    }))?;
                }
            }
            GuiCmd::ControlMatcherChange(cmd) => {
                let _ = self.cfg.traverse_mut(cmd);
                self.cfg.recompute_mappings_metadata();
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
                self.cfg.recompute_mappings_metadata();
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
                self.cfg.recompute_mappings_metadata();
                self.send_driver_cmd(DriverCmd::ChangeConfigSimple { cfg: self.cfg.clone() });
            }
            GuiCmd::MappingChange(action) => self.send_driver_cmd(DriverCmd::ChangeMappings {
                mappings: Some(self.cfg.mappings.clone()),
                action: action.clone(),
            }),
            GuiCmd::ConfigChangeSimple => self.send_driver_cmd(DriverCmd::ChangeConfigSimple { cfg: self.cfg.clone() }),
            GuiCmd::ConfigChangeDriverRestart => {
                self.execute_gui_command(GuiCmd::ConfigChangeSimple)?;
                self.send_driver_cmd(DriverCmd::Reload);
            }
            GuiCmd::DragAndDrop(_) | GuiCmd::LocalItemRemove(_) => {
                unreachable!(
                    "Drag and drop and local item removal commands are expected to be \
                      handled on upper level in Gui. Error in implementation."
                )
            }
            GuiCmd::VirtualDeviceChange(GuiCmdVirtualDeviceChange { restart_persistent }) => {
                self.cfg.recompute_mappings_metadata();
                self.reset_available_devices_caches();
                let (tx, rx) = std::sync::mpsc::channel::<()>();
                self.send_driver_cmd(DriverCmd::ChangeVirtualHids {
                    cfg: self.cfg.clone(),
                    restart_persistent: *restart_persistent,
                    report_done_tx: tx,
                });
                let _ = rx
                    .recv_timeout(Duration::from_millis(3000))
                    .inspect_err(|e| log::error!("{e}"));
            }
            GuiCmd::LoadCfg(path) => {
                let (resp_tx, resp_rx) = std::sync::mpsc::channel::<Result<Config, String>>();
                self.reset_available_devices_caches();
                self.send_driver_cmd(DriverCmd::LoadCfg {
                    cfg_file: path.clone(),
                    resp_tx,
                });
                match resp_rx.recv_timeout(Duration::from_millis(5000)) {
                    Ok(Ok(new_cfg)) => {
                        self.cfg = new_cfg;
                        self.update_cfg_yaml();
                        self.pending_cmds.clear();
                    }
                    Ok(Err(e)) => {
                        return Err(format!("Failed to load new config: {e:?}"));
                    }
                    Err(e) => {
                        return Err(format!("Failed to receive new config from driver: {e:?}"));
                    }
                }
            }
            GuiCmd::SaveCfg(cfg_file, cfg_suffix) => {
                self.send_driver_cmd(DriverCmd::SaveCfg {
                    cfg_file: cfg_file.clone(),
                    cfg_suffix: cfg_suffix.clone(),
                });
            }
            GuiCmd::IdleTickRateChange => self.send_driver_cmd(DriverCmd::ChangeIdleTickRate {
                rate: self.cfg.global.idle_tick_rate,
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
                        res = Some(self.execute_gui_command(cmd).inspect_err(|e| warn!("{e:?}")));
                    }
                }
            }
            GuiCmd::BreakOnErr => {}
            GuiCmd::SubmitPending(gui_cmd) => self.submit_pending_cmd(*(*gui_cmd).clone()),
        }
        Ok(())
    }

    fn process_post_draw_commands(&mut self) {
        if self.post_draw_cmds.is_empty() {
            return;
        }

        if MORE_DEBUG {
            for cmd in self.post_draw_cmds.iter() {
                log::info!("Applying post-draw command {:?}", cmd.cmd);
            }
        }

        let mut res = None;
        for cmd in self.post_draw_cmds.clone().drain(..) {
            if let GuiCmd::BreakOnErr = cmd.cmd {
                if let Some(Err(_)) = res.as_ref() {
                    break;
                }
            } else {
                res = Some(self.execute_gui_command(cmd.cmd).inspect_err(|e| warn!("{e:?}")));
            }
        }

        self.post_draw_cmds.clear();
        self.update_cfg_yaml();
    }

    fn process_pending_commands(&mut self) {
        if self.pending_cmds.is_empty() {
            return;
        }

        if MORE_DEBUG {
            for pending in self.pending_cmds.iter() {
                log::info!("Applying pending command {:?}", pending.cmd);
            }
        }

        self.pending_cmds.dedup_by(|next, prev| *next == *prev);

        let mut res = None;
        for cmd in self.pending_cmds.clone().drain(..) {
            if let GuiCmd::BreakOnErr = cmd.cmd {
                if let Some(Err(_)) = res.as_ref() {
                    break;
                }
            } else {
                res = Some(self.execute_gui_command(cmd.cmd));
            }
        }

        self.pending_cmds.clear();
        self.update_cfg_yaml();
    }

    fn draw_key_edit_gui(&mut self, ui: &mut egui::Ui, dmk: &String, is_virtual: bool) {
        let is_editing = self.transient_states_device_key_edit.contains_key(dmk);
        if !is_editing {
            if ui
                .button(egui_phosphor::fill::IDENTIFICATION_BADGE.to_string())
                .on_hover_text("Edit config key name")
                .clicked()
            {
                self.transient_states_device_key_edit.insert(dmk.clone(), dmk.clone());
            }
        } else {
            let mut pending_change_to_submit = None;
            let new_key = self.transient_states_device_key_edit.get_mut(dmk).unwrap();

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
                self.submit_post_draw_cmd(GuiCmd::DeviceMatcherRename(pending_change_to_submit.clone()));
                self.transient_states_device_key_edit
                    .remove(&pending_change_to_submit.old_key);
            }
        }
    }

    fn draw_device_remove_button(&mut self, ui: &mut egui::Ui, dmk: &str, is_virtual: bool) {
        if ui
            .small_button(egui_phosphor::fill::TRASH.to_string())
            .on_hover_text("Try to remove (will be removed if not referenced)")
            .clicked()
        {
            self.submit_post_draw_cmd(GuiCmd::DeviceMatcherRemove(GuiCmdDeviceMatcherRemove {
                device_key: dmk.to_owned(),
                is_virtual,
            }));
        }
    }

    // ================================================================================
    // ================================================================================
    // ================================================================================
    // ================================================================================

    #[cfg(feature = "midi")]
    fn draw_device_and_matchers_midi(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        draw_collapsing_ui(ui, None::<()>, Some("Available"), |_| {}).body(|ui| {
            ui.group(|ui| {
                if let Some(ref mut list) = self.available_midi {
                    for (ad_idx, ad) in list.iter_mut().enumerate() {
                        ui.separator();
                        ui.collapsing(format!("({}) {}", ad_idx, ad.name), |ui| {
                            ad.egui((), ui);
                        });
                    }
                } else {
                    self.available_midi = Some(self.midi_mgr.enumerate_available_devices(None));
                }
            });
        });
        ui.separator();
        draw_collapsing_ui(ui, None::<()>, Some("Matchers"), |ui| {
            ui.separator();
            if ui
                .button(egui_phosphor::bold::LIST_PLUS.to_string())
                .on_hover_text("add midi matcher")
                .clicked()
            {
                self.cfg.devices.midi.insert(
                    get_item_name_with_random_suffix("MIDI matcher", self.cfg.devices.midi.len() + 1),
                    MidiMatcherCfg::default(),
                );
                self.submit_post_draw_cmd(GuiCmd::ConfigChangeSimple);
            }
        })
        .body(|ui| {
            if !self.cfg.devices.midi.is_empty() {
                ui.group(|ui| {
                    let keys = self.cfg.devices.midi.keys().cloned().collect::<Vec<_>>();
                    for dmk in keys {
                        ui.separator();
                        draw_collapsing_ui(ui, None::<()>, Some(&dmk), |ui| {
                            ui.separator();
                            self.draw_key_edit_gui(ui, &dmk, false);
                            ui.separator();
                            self.draw_device_remove_button(ui, &dmk, false);
                        })
                        .body(|ui| {
                            if let Some(cmd) = self
                                .cfg
                                .devices
                                .midi
                                .get_mut(&dmk)
                                .unwrap()
                                .egui(GuiInDeviceCfg::Edit { device_key: &dmk }, ui)
                            {
                                // dbg!(&cmd);
                                self.submit_post_draw_cmd(cmd);
                            }
                        });
                    }
                });
            }
        });
    }

    // ================================================================================
    // ================================================================================
    // ================================================================================
    // ================================================================================

    fn draw_device_and_matchers_hid(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        draw_collapsing_ui(ui, None::<()>, Some("Available"), |_| {}).body(|ui| {
            if let Some(ref mut list) = self.available_hid {
                for (ad_idx, ad) in list.iter_mut().enumerate() {
                    ui.separator();
                    ui.collapsing(format!("({}) {}", ad_idx, ad.name), |ui| {
                        ad.egui((), ui);
                    });
                }
            } else {
                self.available_hid = Some(self.hid_mgr.enumerate_available_devices(Some(
                    DeviceKind::Mouse | DeviceKind::Keyboard | DeviceKind::Joystick | DeviceKind::Gamepad, //  HidDeviceKind::Misc |
                                                                                                           //  HidDeviceKind::MiscMappable
                )))
            }
        });
        ui.separator();
        draw_collapsing_ui(ui, None::<()>, Some("Virtual"), |ui| {
            ui.separator();
            if ui
                .button(egui_phosphor::bold::LIST_PLUS.to_string())
                .on_hover_text("add virtual device")
                .clicked()
            {
                let dk = get_item_name_with_random_suffix("Virtual HID", self.cfg.devices.hid.len() + 1);
                self.cfg
                    .devices
                    .hid
                    .insert(dk.clone(), HidDeviceCfg::new_virtual(dk.as_str()));
                self.submit_post_draw_cmd(GuiCmd::ConfigChangeSimple);
            }
        })
        .body(|ui| {
            if !self.cfg.devices.hid.is_empty() {
                ui.group(|ui| {
                    let keys: Vec<String> = self
                        .cfg
                        .devices
                        .hid
                        .iter()
                        .filter(|d| d.1.is_a_virtual())
                        .map(|(k, _)| k.clone())
                        .collect();
                    for dmk in keys {
                        ui.separator();
                        draw_collapsing_ui(ui, None::<()>, Some(&dmk), |ui| {
                            ui.separator();
                            self.draw_key_edit_gui(ui, &dmk, true);
                            ui.separator();
                            self.draw_device_remove_button(ui, &dmk, true);
                        })
                        .body(|ui| {
                            if let Some(cmd) = self
                                .cfg
                                .devices
                                .hid
                                .get_mut(&dmk)
                                .unwrap()
                                .egui(GuiInDeviceCfg::Edit { device_key: &dmk }, ui)
                            {
                                self.submit_post_draw_cmd(GuiCmd::CmdSeqence(vec![
                                    cmd,
                                    GuiCmd::BreakOnErr,
                                    GuiCmd::SubmitPending(
                                        GuiCmd::VirtualDeviceChange(GuiCmdVirtualDeviceChange {
                                            restart_persistent: true,
                                        })
                                        .into(),
                                    ),
                                ]));
                            };
                        });
                    }
                });
            }
        });
        ui.separator();
        draw_collapsing_ui(ui, None::<()>, Some("Matchers"), |ui| {
            ui.separator();
            if ui
                .button(egui_phosphor::bold::LIST_PLUS.to_string())
                .on_hover_text("add device matcher")
                .clicked()
            {
                self.cfg.devices.hid.insert(
                    get_item_name_with_random_suffix("HID matcher", self.cfg.devices.hid.len() + 1),
                    HidDeviceCfg::new_matcher(),
                );
                self.submit_post_draw_cmd(GuiCmd::ConfigChangeSimple);
            }
        })
        .body(|ui| {
            let mut hid_matcher_update_cmd = None;
            if !self.cfg.devices.hid.is_empty() {
                ui.group(|ui| {
                    let keys: Vec<String> = self
                        .cfg
                        .devices
                        .hid
                        .iter()
                        .filter(|d| !d.1.is_a_virtual())
                        .map(|(k, _)| k.clone())
                        .collect();
                    for dmk in keys {
                        ui.separator();
                        draw_collapsing_ui(ui, None::<()>, Some(&dmk), |ui| {
                            ui.separator();
                            self.draw_key_edit_gui(ui, &dmk, false);
                            ui.separator();
                            self.draw_device_remove_button(ui, &dmk, false);
                        })
                        .body(|ui| {
                            let dm = self.cfg.devices.hid.get_mut(&dmk).unwrap();
                            ui.separator();
                            ui.label(format!("Matcher classification: {}", dm.get_classification()));
                            if let Some(cmd) = dm.egui(GuiInDeviceCfg::Edit { device_key: &dmk }, ui) {
                                dm.update_classification();
                                hid_matcher_update_cmd = Some(cmd);
                            }
                        });
                    }
                });
            }
            if let Some(command) = &hid_matcher_update_cmd {
                self.submit_post_draw_cmd(command.clone());
            }
        });
    }

    fn reset_available_devices_caches(&mut self) {
        self.available_hid = None;
        #[cfg(feature = "midi")]
        {
            self.available_midi = None;
        }
    }

    fn draw_variables(&mut self, ui: &mut egui::Ui) {
        let mut variable_value_changed = false;
        let mut variable_edit_to_submit = None;
        let mut variable_remove_to_submit = None;
        if ui
            .button(egui_phosphor::bold::LIST_PLUS.to_string())
            .on_hover_text("add variable")
            .clicked()
        {
            self.cfg.variables.insert(
                get_item_name_with_random_suffix("New Variable", self.cfg.variables.len() + 1),
                VariableState::default(),
            );
            self.submit_post_draw_cmd(GuiCmd::ConfigChangeSimple);
        }

        ui.separator();
        for v in &mut self.cfg.variables {
            ui.separator();
            ui.group(|ui| {
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
                        variable_value_changed |= v.1.egui(GuiInKinds::Display, ui);
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
            });
            ui.separator();
        }

        if let Some(variable_edit_to_submit) = variable_edit_to_submit {
            self.submit_post_draw_cmd(GuiCmd::VariableChange(variable_edit_to_submit.clone()));
            self.transient_states_variable_edit
                .remove(&variable_edit_to_submit.old_key);
        } else if let Some(variable_remove_to_submit) = variable_remove_to_submit {
            self.submit_post_draw_cmd(GuiCmd::VariableRemove(variable_remove_to_submit));
        } else if variable_value_changed {
            self.submit_post_draw_cmd(GuiCmd::ConfigChangeSimple);
        }
    }

    fn draw_and_handle_pending_changes_button(&mut self, ui: &mut egui::Ui) {
        if self.pending_cmds.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} apply pending", egui_phosphor::bold::CHECK_SQUARE))
                .highlight()
                .clicked()
            {
                self.process_pending_commands();
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
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut format!("{ui_axis_monitor_cfg:#?}"))
                                        .font(egui::TextStyle::Monospace)
                                        .interactive(true)
                                        // .background_color(Color32::BLACK)
                                        // .text_color(Color32::GRAY)
                                        .desired_width(f32::INFINITY)
                                        .hint_text("--"),
                                )
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
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut format!("{m:#?}"))
                                .font(egui::TextStyle::Monospace)
                                .interactive(true)
                                // .background_color(Color32::BLACK)
                                // .text_color(Color32::GRAY)
                                .desired_width(f32::INFINITY)
                                .hint_text("--"),
                        )
                    });
                });
            }
        });
        ui.separator();
        ui.collapsing("Device and control matchers state:", |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut format!("{:#?}", self.cfg.devices))
                        .font(egui::TextStyle::Monospace)
                        .interactive(false)
                        // .background_color(Color32::BLACK)
                        // .text_color(Color32::GRAY)
                        .desired_width(f32::INFINITY)
                        .hint_text("--"),
                )
            });
        });
        ui.separator();
        ui.collapsing("Global params:", |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut format!("{:#?}", self.cfg.global))
                        .font(egui::TextStyle::Monospace)
                        // .background_color(Color32::BLACK)
                        // .text_color(Color32::GRAY)
                        .desired_width(f32::INFINITY)
                        .hint_text("--"),
                )
            });
        });
        ui.separator();
        ui.collapsing("Pre-configured (predefined) controls configuration (static):", |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut format!("{:#?}", crate::config::PREDEF_CONTROLS))
                        .font(egui::TextStyle::Monospace)
                        // .background_color(Color32::BLACK)
                        // .text_color(Color32::GRAY)
                        .desired_width(f32::INFINITY)
                        .hint_text("--"),
                )
            });
        });
    }

    pub(crate) fn submit_pending_cmd(&mut self, cmd: GuiCmd) {
        self.pending_cmds.push(PendingCommand { cmd })
    }

    pub(crate) fn submit_post_draw_cmd(&mut self, cmd: GuiCmd) {
        self.post_draw_cmds.push(PendingCommand { cmd })
    }
}
