use crate::{
    gui_common::DrawEgui,
    status::{Status, StatusIface, StatusKind, StatusRetentionPolicy},
};

fn status_kind_to_frame_color(kind: StatusKind) -> egui::Color32 {
    match kind {
        StatusKind::Healthy => egui::Color32::BLUE,
        StatusKind::Misconfigured => egui::Color32::LIGHT_RED,
        StatusKind::Malfunctioning => egui::Color32::RED,
        StatusKind::AttentionRequired => egui::Color32::YELLOW,
    }
}

impl<'s> DrawEgui<'s> for Status {
    type In = ();
    type Out = ();

    fn egui(&mut self, _gui_in: Self::In, ui: &mut egui::Ui) -> Self::Out {
        egui::Frame::default()
            .corner_radius(5.0)
            .inner_margin(3.0)
            .outer_margin(3.0)
            .shadow(egui::Shadow::default())
            .stroke(egui::Stroke::new(
                3.0,
                status_kind_to_frame_color(self.status_get_kind()),
            ))
            .show(ui, |ui| {
                ui.label(match self.status_get_kind() {
                    StatusKind::Healthy => " Information ",
                    StatusKind::Misconfigured => " ~ Configuration error detected ~",
                    StatusKind::Malfunctioning => " ~ Misbehavior detected ~",
                    StatusKind::AttentionRequired => "! Attention please !",
                });
                // ui.label(format!(" ({})", self.status_get_kind()));
                ui.separator();
                ui.add_space(5.0);
                ui.label(self.status_message_ref());
                ui.add_space(5.0);
                match self.status_get_retention_policy() {
                    StatusRetentionPolicy::Permanent => {}
                    StatusRetentionPolicy::Temporary { expiring_at } => {
                        ui.separator();
                        ui.label(format!(
                            "Expiring in {:02.01} seconds...",
                            (expiring_at - std::time::Instant::now()).as_secs_f32()
                        ));
                    }
                    StatusRetentionPolicy::Interactive { acknowledged, .. } => {
                        ui.separator();
                        if !acknowledged
                            && ui
                                .button(egui::RichText::new(egui_phosphor::regular::CHECK_FAT).size(16.0))
                                .clicked()
                        {
                            self.status_acknowledge();
                        }
                    }
                };
                ui.separator();
            });
    }
}
