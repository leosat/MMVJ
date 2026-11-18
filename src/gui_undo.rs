// ----------------------------------

use egui::util::undoer::{Settings as UndoerSettings, Undoer};
use egui::{Key, KeyboardShortcut, Modifiers};
use std::collections::HashMap;

use crate::config::WithSelfSanitize;
use crate::gui_common::GuiCmd;
use crate::gui_main::GuiMain;
use crate::schemas_value::CfgSanitizeCtx;
use crate::{gui_main::PendingCommand, schemas_cfg::Config};

const DEBUG: bool = false;

// ---------------------------------------
const CFG_UNDO_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
static CFG_REDO_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);

const CFG_UNDOER_SETTINGS: UndoerSettings = UndoerSettings {
    max_undos: 100,
    stable_time: 0.2,
    auto_save_interval: 10.0,
};

pub(super) struct GuiCfgUndo {
    pub(super) undoer: Undoer<Config>,
    pub(super) last_fully_applied_epoch: usize,
    pub(super) pending_cmds_history: HashMap<usize, HistoricPendingCommands>,
    pub(super) in_progress: bool,
}

impl GuiCfgUndo {
    pub(super) fn new_with_cfg(cfg: &Config, ui_time: f64) -> Self {
        let mut tmp: Self = Self {
            last_fully_applied_epoch: cfg.edit_epoch,
            ..Default::default()
        };
        tmp.undoer.feed_state(ui_time, &cfg);
        tmp
    }
}

impl Default for GuiCfgUndo {
    fn default() -> Self {
        Self {
            undoer: Undoer::with_settings(CFG_UNDOER_SETTINGS),
            last_fully_applied_epoch: Default::default(),
            pending_cmds_history: Default::default(),
            in_progress: Default::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum HistoricPendingCommands {
    Applied(Vec<PendingCommand>),
    Unapplied(Vec<PendingCommand>),
}

// ---------------------------------------
impl GuiCmd {
    fn get_undo_cmd(&self) -> Option<GuiCmd> {
        match self {
            GuiCmd::ChangeVirtualHid(..) => Some(self.clone()),
            GuiCmd::CmdSeqence(cmds) => GuiCmd::CmdSeqence(
                cmds.iter()
                    .map(|cmd| cmd.get_undo_cmd())
                    .filter_map(|cmd| cmd)
                    .collect::<Vec<_>>(),
            )
            .into(),
            GuiCmd::BreakOnErr
            | GuiCmd::_SubmitPending(..)
            | GuiCmd::LoadCfgFile(..)
            | GuiCmd::LoadCfgDemo(..)
            | GuiCmd::LoadCfgBlank(..)
            | GuiCmd::SaveCfg(..)
            | GuiCmd::VariableChange(..)
            | GuiCmd::VariableRemove(..)
            | GuiCmd::DeviceMatcherRename(..)
            | GuiCmd::DeviceMatcherRemove(..)
            | GuiCmd::ControlMatcherChange(..)
            | GuiCmd::ControlMatcherRemove(..)
            | GuiCmd::ScriptAuxRename(..)
            | GuiCmd::MappingChange(..)
            | GuiCmd::ConfigChangeMiscNoUndoCheckpoint
            | GuiCmd::ConfigChangeSimple
            | GuiCmd::ConfigChangeDriverRestart
            | GuiCmd::BaseTickRateChange
            | GuiCmd::DragAndDrop(..)
            | GuiCmd::LocalItemRemove(..) => None,
        }
    }
}

impl PendingCommand {
    fn get_undo_cmd(&self) -> Option<PendingCommand> {
        self.cmd.get_undo_cmd().map(|cmd| Self { cmd })
    }
}

// ------------------------------------------
impl GuiMain {
    pub(super) fn store_applied_pending_commands_for_undo(&mut self, _ui: &mut egui::Ui) {
        self.cfg_undo.pending_cmds_history.insert(
            self.cfg_undo.last_fully_applied_epoch,
            HistoricPendingCommands::Applied(
                self.pending_cmds
                    .clone()
                    .iter()
                    .filter_map(|cmd| cmd.get_undo_cmd())
                    .rev()
                    .collect::<Vec<_>>(),
            ),
        );

        self.cfg_undo.pending_cmds_history.insert(
            self.cfg.edit_epoch,
            HistoricPendingCommands::Applied(self.pending_cmds.clone()),
        );
    }

    fn on_history_saved(&mut self, _ui: &mut egui::Ui) {
        if self.pending_cmds.is_empty() {
            if DEBUG {
                dbg!("Store last fully applied eposh as", self.cfg.edit_epoch);
            }
            self.cfg_undo.last_fully_applied_epoch = self.cfg.edit_epoch;
        } else {
            if DEBUG {
                dbg!("Saved unapplied pending commands");
            }
            self.cfg_undo.pending_cmds_history.insert(
                self.cfg.edit_epoch,
                HistoricPendingCommands::Unapplied(self.pending_cmds.clone()),
            );
        }
        if DEBUG {
            dbg!("Retaining only pending commands after ", self.cfg.edit_epoch);
        }
        self.cfg_undo
            .pending_cmds_history
            .retain(|epoch, _| *epoch <= self.cfg.edit_epoch);
    }

    pub(super) fn cfg_undo_checkpoint_start(&mut self, ui: &mut egui::Ui) {
        self.cfg.edit_epoch = self.cfg.edit_epoch.wrapping_add(1);
        if DEBUG {
            dbg!("Checkpinted epoch is ", self.cfg.edit_epoch);
            dbg!("Undo checkpoint start");
        }
        self.cfg_undo.in_progress = true;
        self.cfg_undo_checkpoint_try_finish(ui);
    }

    pub(super) fn cfg_undo_checkpoint_try_finish(&mut self, ui: &mut egui::Ui) {
        if self.cfg_undo.in_progress {
            if DEBUG {
                dbg!("Undo checkpoint in progress ... feeding state.");
            }
            self.cfg_undo.undoer.feed_state(ui.ctx().time(), &self.cfg);
            if !self.cfg_undo.undoer.is_in_flux() {
                if DEBUG {
                    dbg!("Finishing undo checkpoint");
                }
                self.cfg_undo.in_progress = false;
                self.on_history_saved(ui);
            }
        }
    }

    pub(super) fn apply_cfg_undo_redo(&mut self, redo: bool, ui: &mut egui::Ui) -> Result<bool, String> {
        self.cfg = {
            let Some(maybe_restored_cfg) = (if redo {
                self.cfg_undo.undoer.redo(&mut self.cfg)
            } else {
                self.cfg_undo.undoer.undo(&mut self.cfg)
            }) else {
                return Ok(false);
            };
            maybe_restored_cfg.clone()
        };

        self.reset_runtime_state();

        log::info!("Restored configuration, edit epoch {}", self.cfg.edit_epoch);

        if DEBUG {
            dbg!(self.cfg.edit_epoch);
        }
        self.cfg_undo
            .pending_cmds_history
            .get(&self.cfg.edit_epoch)
            .cloned()
            .inspect(|cmds| {
                if DEBUG {
                    dbg!("Processing memorized pending commands!");
                }
                match cmds {
                    HistoricPendingCommands::Applied(pending_commands) => {
                        log::info!("Applying associated pending commands");
                        if DEBUG {
                            dbg!("Historic pending commands applied!");
                        }
                        self.pending_cmds.extend_from_slice(pending_commands);
                        self.process_pending_commands(ui);
                    }
                    HistoricPendingCommands::Unapplied(pending_commands) => {
                        log::info!("Restoring unapplied associated pending commands");
                        if DEBUG {
                            dbg!("Historic pending commands restored unapplied!");
                        }
                        self.pending_cmds.extend_from_slice(pending_commands);
                    }
                }
            });

        if self.pending_cmds.is_empty() {
            if DEBUG {
                dbg!("Associating last fully applied epoch with ", self.cfg.edit_epoch);
            }
            log::info!(
                "Setting {} as last fully applied epoch (has no associated pending commands)",
                self.cfg.edit_epoch
            );
            self.cfg_undo.last_fully_applied_epoch = self.cfg.edit_epoch;
        }

        // TODO: be more fine-grained.
        self.execute_gui_command(GuiCmd::ConfigChangeDriverRestart, ui)?;

        // TODO: support for whole config in case it's required for any other parts of it.
        // TODO: or change variables to static/dynamic, removing the below state restoration.
        self.cfg
            .variables
            .values_mut()
            .for_each(|var| var.sanitize_inplace(CfgSanitizeCtx::PostRestore));

        self.update_cfg_yaml();

        Ok(true)
    }

    pub(super) fn cfg_undo_reset(&mut self, ui: &mut egui::Ui) {
        self.cfg_undo.undoer = Undoer::with_settings(CFG_UNDOER_SETTINGS);
        self.cfg_undo.undoer.feed_state(ui.ctx().time(), &self.cfg);
        self.cfg_undo.last_fully_applied_epoch = self.cfg.edit_epoch;
        self.cfg_undo.pending_cmds_history.clear();
    }

    pub(super) fn draw_undo_redo_edit_menu(&mut self, ui: &mut egui::Ui) {
        let undo_txt = ui.ctx().format_shortcut(&CFG_UNDO_SHORTCUT);
        let redo_txt = ui.ctx().format_shortcut(&CFG_REDO_SHORTCUT);

        let can_undo = self.cfg_undo.undoer.has_undo(&self.cfg);
        let can_redo = self.cfg_undo.undoer.has_redo(&self.cfg);

        ui.separator();
        let undo_clicked = ui
            .add_enabled(
                can_undo,
                egui::Button::new(format!(
                    "{} {} Undo config change",
                    egui_phosphor::regular::ARROW_U_UP_LEFT,
                    egui_phosphor::regular::DOTS_SIX_VERTICAL
                ))
                .shortcut_text(undo_txt),
            )
            .clicked();
        ui.separator();
        let redo_clicked = ui
            .add_enabled(
                can_redo,
                egui::Button::new(format!(
                    "{} {} Redo config change",
                    egui_phosphor::regular::ARROW_U_UP_RIGHT,
                    egui_phosphor::regular::DOTS_SIX_VERTICAL
                ))
                .shortcut_text(redo_txt),
            )
            .clicked();
        ui.separator();

        if undo_clicked {
            let _ = self.apply_cfg_undo_redo(false, ui);
        } else if redo_clicked {
            let _ = self.apply_cfg_undo_redo(true, ui);
        }
    }

    pub(super) fn draw_undo_redo_buttons(&mut self, ui: &mut egui::Ui) {
        let can_undo = self.cfg_undo.undoer.has_undo(&self.cfg);
        let can_redo = self.cfg_undo.undoer.has_redo(&self.cfg);
        let undo_resp = ui
            .add_enabled(
                can_undo,
                egui::Button::new(egui_phosphor::bold::ARROW_U_UP_LEFT.to_string()),
            )
            .on_disabled_hover_text("Nothing to undo")
            .on_hover_ui(|ui| {
                ui.label("Undo last config change");
                ui.label(ui.ctx().format_shortcut(&CFG_UNDO_SHORTCUT));
            });
        let redo_resp = ui
            .add_enabled(
                can_redo,
                egui::Button::new(egui_phosphor::bold::ARROW_U_UP_RIGHT.to_string()),
            )
            .on_disabled_hover_text("Nothing to redo")
            .on_hover_ui(|ui| {
                ui.label("Redo last undone config change");
                ui.label(ui.ctx().format_shortcut(&CFG_REDO_SHORTCUT));
            });
        if undo_resp.clicked() {
            let _ = self.apply_cfg_undo_redo(false, ui);
        } else if redo_resp.clicked() {
            let _ = self.apply_cfg_undo_redo(true, ui);
        }
    }

    pub(super) fn handle_cfg_undo_redo_pressed(&mut self, ui: &mut egui::Ui) {
        // let text_edit_in_progress = ui.ctx().memory(|m| m.focused().is_some());
        // if !text_edit_in_progress {
        let undo_pressed = ui.ctx().input_mut(|i| i.consume_shortcut(&CFG_UNDO_SHORTCUT));
        let redo_pressed = ui.ctx().input_mut(|i| i.consume_shortcut(&CFG_REDO_SHORTCUT));

        if undo_pressed || redo_pressed {
            if undo_pressed && self.cfg_undo.undoer.is_in_flux() {
                log::warn!("Skipped applying undo while in flux");
                // self.cfg.edit_epoch = self.cfg.edit_epoch.wrapping_add(1);
                // self.undoer.add_undo(&self.cfg);
            } else {
                let _ = self
                    .apply_cfg_undo_redo(redo_pressed, ui)
                    .inspect_err(|e| log::error!("Config undo/redo failed: {e}"));
            }
        }
        // }
    }
}
