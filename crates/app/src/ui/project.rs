//! Project JSON save, load, copy, and platform file controls.

use eframe::egui::{self, Align2, RichText, TextEdit, Ui, Vec2};

use crate::document::Document;
use crate::export;
use crate::ui::{muted_text_color, Status};

/// Project JSON dialog shared by native and web builds.
pub struct ProjectDialog {
    open: bool,
    path: String,
    text: String,
    error: Option<String>,
    loaded: Option<Document>,
}

impl Default for ProjectDialog {
    fn default() -> Self {
        Self {
            open: false,
            path: "circuitsim-project.json".to_owned(),
            text: String::new(),
            error: None,
            loaded: None,
        }
    }
}

impl ProjectDialog {
    /// Opens the dialog with the current project serialized.
    pub fn open_with(&mut self, document: &Document) {
        self.open = true;
        self.error = None;
        self.text = document.to_project_json().unwrap_or_default();
        if self.path.trim().is_empty() {
            self.path = "circuitsim-project.json".to_owned();
        }
    }

    /// Returns and clears one loaded project.
    pub fn take_loaded(&mut self) -> Option<Document> {
        self.loaded.take()
    }

    /// Draws the dialog.
    pub fn show(&mut self, context: &egui::Context, document: &Document, status: &mut Status) {
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Projet")
            .open(&mut open)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .show(context, |ui| {
                ui.label(
                    "Format JSON versionné. Copiez le texte pour partager le schéma ou \
                     chargez-le dans une autre session.",
                );
                if let Some(error) = &self.error {
                    ui.colored_label(egui::Color32::from_rgb(240, 128, 128), error);
                }
                ui.horizontal(|ui| {
                    ui.label("Fichier");
                    ui.add(
                        TextEdit::singleline(&mut self.path)
                            .desired_width(360.0)
                            .hint_text("circuitsim-project.json"),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Rafraîchir depuis le schéma").clicked() {
                        match document.to_project_json() {
                            Ok(text) => {
                                self.text = text;
                                self.error = None;
                                status.info("Projet sérialisé.");
                            }
                            Err(error) => self.error = Some(error.user_message()),
                        }
                    }
                    if ui.button("Copier le JSON").clicked() {
                        ui.ctx().copy_text(self.text.clone());
                        status.success("Projet copié dans le presse-papiers.");
                    }
                    if ui.button("Charger ce JSON").clicked() {
                        match Document::from_project_json(&self.text) {
                            Ok(loaded) => {
                                self.loaded = Some(loaded);
                                self.open = false;
                                status.success("Projet chargé.");
                            }
                            Err(error) => self.error = Some(error.user_message()),
                        }
                    }
                });
                ui.separator();
                self.file_controls(ui, status);
                ui.add_space(6.0);
                ui.label(RichText::new("Contenu JSON").strong());
                egui::ScrollArea::vertical()
                    .max_height(340.0)
                    .id_salt("project_json")
                    .show(ui, |ui| {
                        ui.add(
                            TextEdit::multiline(&mut self.text)
                                .desired_width(f32::INFINITY)
                                .desired_rows(14)
                                .code_editor(),
                        );
                    });
                ui.label(
                    RichText::new(
                        "Un JSON invalide ou d'une version future est refusé sans modifier le schéma courant.",
                    )
                    .small()
                    .color(muted_text_color(ui)),
                );
                if ui.button("Fermer").clicked() {
                    self.open = false;
                }
            });
        if !open {
            self.open = false;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn file_controls(&mut self, ui: &mut Ui, status: &mut Status) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Lire le fichier").clicked() {
                match std::fs::read_to_string(self.path.trim()) {
                    Ok(text) => {
                        self.text = text;
                        self.error = None;
                        status.info("Fichier lu.");
                    }
                    Err(error) => self.error = Some(format!("Lecture impossible : {error}")),
                }
            }
            if ui.button("Enregistrer le fichier").clicked() {
                match export::save_text(&self.path, &self.text) {
                    Ok(()) => {
                        self.error = None;
                        status.success("Projet enregistré.");
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn file_controls(&mut self, ui: &mut Ui, status: &mut Status) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Télécharger le fichier").clicked() {
                match export::download_text(
                    "circuitsim-project.json",
                    "application/json;charset=utf-8",
                    &self.text,
                ) {
                    Ok(()) => {
                        self.error = None;
                        status.success("Projet téléchargé.");
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        });
    }
}
