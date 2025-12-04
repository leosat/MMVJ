use crate::gui_common::DrawEgui;
use crate::schemas_cfg::Config;

#[derive(Clone, Copy, Default)]
pub(crate) enum GuiInCfg {
    #[default]
    YAMLDump,
}

impl Config {}

impl DrawEgui<'_> for Config {
    type In = GuiInCfg;
    type Out = ();

    fn egui(&mut self, state: Self::In, _ui: &mut egui::Ui) -> Self::Out {
        match state {
            GuiInCfg::YAMLDump => todo!(),
        }
    }
}
