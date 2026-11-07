//! Native password-manager window; capture/fill is always an explicit action.
use super::browser;
use crate::window::ServoShellWindow;
use rust_browser_core::passwords::{
    CAPTURE_SCRIPT, Credential, export_csv, fill_script, origin, parse_csv,
};
use std::io::Read;
use std::sync::mpsc::{self, Receiver};

#[derive(Default)]
pub struct PasswordUi {
    site: String,
    username: String,
    password: String,
    search: String,
    reveal: bool,
    status: String,
    capture: Option<Receiver<Result<Credential, String>>>,
    fill: Option<Receiver<bool>>,
    delete: Option<usize>,
    editing: Option<(String, String)>,
    import_dialog: egui_file_dialog::FileDialog,
    export_dialog: egui_file_dialog::FileDialog,
    pending_import: Option<Vec<Credential>>,
    export_warning: bool,
}
impl PasswordUi {
    pub fn show_manager(&mut self, ctx: &mut egui::Ui, window: Option<&ServoShellWindow>) {
        if let Some(result) = self.capture.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.capture = None;
            match result {
                Ok(entry) => {
                    self.site = entry.origin;
                    self.username = entry.username;
                    self.password = entry.password;
                    self.reveal = false;
                    self.editing = None;
                    self.status = "Проверь данные и нажми «Сохранить».".into();
                },
                Err(e) => self.status = e,
            }
        }
        if let Some(success) = self.fill.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.fill = None;
            self.status = if success {
                "Данные заполнены. Вход подтверди на сайте."
            } else {
                "Не найдена форма входа или сайт сменил адрес."
            }
            .into();
        }
        let current = window
            .and_then(|w| w.active_webview())
            .and_then(|v| v.url())
            .map(|u| u.to_string())
            .unwrap_or_default();
        if self.site.is_empty() {
            self.site = current_origin_for(&current);
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(ctx.visuals().window_fill).inner_margin(24)).show_inside(ctx, |ui| {
            ui.add_space(8.0);
            ui.heading("Пароли");
            ui.weak("На этом компьютере · защита Windows DPAPI");
            if let Some(error) = browser::with(|b| b.vault_error.clone()) { ui.colored_label(super::browser_chrome::ACCENT, error); }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Добавить пароль").clicked() {
                    self.editing = None; self.password.clear(); self.username.clear(); self.reveal = false; self.site = current_origin_for(&current);
                }
                if ui.button("Импорт CSV…").clicked() {
                    self.import_dialog = egui_file_dialog::FileDialog::new().add_file_filter_extensions("CSV", vec!["csv"]);
                    self.import_dialog.pick_file();
                }
                if ui.button("Экспорт CSV…").clicked() { self.export_warning = true; }
                if ui.add_enabled(window.is_some(), egui::Button::new("Взять с текущей страницы")).clicked() {
                    if let Some(window) = window { self.capture_form(window); }
                }
            });
            if self.export_warning {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label("CSV содержит открытые пароли. Сохрани его в защищённом месте и удали после переноса.");
                    ui.horizontal(|ui| {
                        if ui.button("Выбрать файл для экспорта…").clicked() {
                            self.export_warning = false;
                            self.export_dialog = egui_file_dialog::FileDialog::new().default_file_name("passwords.csv").add_file_filter_extensions("CSV", vec!["csv"]);
                            self.export_dialog.save_file();
                        }
                        if ui.button("Отмена").clicked() { self.export_warning = false; }
                    });
                });
            }
            if let Some(entries) = &self.pending_import {
                let count = entries.len();
                ui.label(format!("Готово к импорту: {count}. Существующие записи с тем же сайтом и логином сохранятся."));
                ui.horizontal(|ui| {
                    if ui.button("Импортировать записи").clicked() {
                        let entries = self.pending_import.take().unwrap_or_default();
                        let result = browser::with(|b| {
                            let mut next = b.vault.clone();
                            let counts = next.import_entries(entries);
                            b.replace_vault(next)?;
                            Ok::<_, String>(counts)
                        });
                        self.status = match result { Ok((added, skipped)) => format!("Добавлено: {added}. Уже существовали: {skipped}."), Err(error) => error };
                    }
                    if ui.button("Отменить импорт").clicked() { self.pending_import = None; }
                });
            }
            ui.separator();
            ui.columns(2, |columns| {
            {
            let ui = &mut columns[0];
            ui.label(if self.editing.is_some() { "Изменить запись" } else { "Новая запись" });
            ui.add_space(8.0);
            let field_width = (ui.available_width()-80.0).max(160.0);
            egui::Grid::new("password_fields").num_columns(2).spacing([12.0, 9.0]).show(ui, |ui| {
                ui.label("Сайт"); ui.add(egui::TextEdit::singleline(&mut self.site).desired_width(field_width).hint_text("https://example.org")); ui.end_row();
                ui.label("Логин"); ui.add(egui::TextEdit::singleline(&mut self.username).desired_width(field_width)); ui.end_row();
                ui.label("Пароль"); ui.add(egui::TextEdit::singleline(&mut self.password).password(!self.reveal).desired_width(field_width)); ui.end_row();
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.reveal, "Показать пароль");
                if ui.button(if self.editing.is_some() { "Сохранить изменения" } else { "Сохранить" }).clicked() {
                    let result = browser::with(|b| {
                        let mut next = b.vault.clone();
                        if let Some((site, user)) = &self.editing { next.replace(site, user, &self.site, &self.username, &self.password)?; }
                        else { next.upsert(&self.site, &self.username, &self.password)?; }
                        b.replace_vault(next)
                    });
                    self.status = match result { Ok(()) => { self.password.clear(); self.reveal = false; self.editing = None; "Пароль сохранён.".into() }, Err(e) => e };
                }
            });
            if !self.status.is_empty() { ui.add_space(8.0); ui.label(&self.status); }
            }
            let ui = &mut columns[1];
            ui.add(egui::TextEdit::singleline(&mut self.search).desired_width(f32::INFINITY).hint_text("Найти сайт или логин"));
            let entries = browser::with(|b| b.vault.entries.iter().enumerate().map(|(i, e)| (i, e.origin.clone(), e.username.clone())).collect::<Vec<_>>());
            ui.weak(format!("Сохранено · {}", entries.len()));
            if entries.is_empty() { ui.add_space(24.0); ui.label("Паролей пока нет"); ui.weak("Импортируй CSV или добавь запись слева"); }
            let current_origin = origin(&current).ok();
            let query = self.search.to_lowercase();
            if let Some(index) = self.delete {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Удалить выбранную запись?");
                    if ui.button("Удалить запись").clicked() {
                        let result = browser::with(|b| { let mut next = b.vault.clone(); if index < next.entries.len() { next.entries.remove(index); } b.replace_vault(next) });
                        self.status = match result { Ok(()) => "Запись удалена.".into(), Err(e) => e };
                        self.delete = None; self.editing = None; self.password.clear(); self.reveal = false;
                    }
                    if ui.button("Отмена").clicked() { self.delete = None; }
                });
            }
            egui::ScrollArea::vertical().max_height((ui.available_height() - 36.0).max(80.0)).show(ui, |ui| {
                for (index, site, user) in entries.into_iter().filter(|(_, site, user)| query.is_empty() || site.to_lowercase().contains(&query) || user.to_lowercase().contains(&query)) {
                    ui.horizontal_wrapped(|ui| {
                        ui.vertical(|ui| { ui.label(&site); ui.weak(&user); });
                        if ui.add_enabled(window.is_some() && current_origin.as_deref() == Some(site.as_str()), egui::Button::new("Заполнить")).clicked() {
                            if let Some(window) = window { self.fill_form(window, index); }
                        }
                        if ui.small_button("Изменить").clicked() {
                            if let Some(e) = browser::with(|b| b.vault.entries.get(index).cloned()) {
                                self.editing = Some((e.origin.clone(), e.username.clone())); self.site = e.origin; self.username = e.username; self.password = e.password; self.reveal = false;
                            }
                        }
                        if ui.small_button("Удалить").clicked() { self.delete = Some(index); }
                    });
                    ui.separator();
                }
            });
            });
            ui.add_space(12.0);
            ui.separator();
            ui.small("Заполнение только на сохранённом сайте. Вход подтверждается на странице");
        });
        self.import_dialog.update(ctx);
        self.export_dialog.update(ctx);
        if let Some(path) = self.import_dialog.take_picked() {
            let result = (|| {
                let file = std::fs::File::open(path).map_err(|_| "Не удалось открыть CSV.")?;
                let mut bytes = Vec::new();
                file.take(16 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "Не удалось прочитать CSV.")?;
                parse_csv(&bytes)
            })();
            match result {
                Ok(entries) => {
                    self.pending_import = Some(entries);
                    self.status.clear();
                },
                Err(error) => self.status = error,
            }
        }
        if let Some(path) = self.export_dialog.take_picked() {
            let result = browser::with(|b| export_csv(&b.vault.entries)).and_then(|bytes| {
                std::fs::write(path, bytes).map_err(|_| "Не удалось сохранить CSV.".into())
            });
            self.status = match result {
                Ok(()) => "CSV экспортирован без шифрования.".into(),
                Err(error) => error,
            };
        }
        if self.capture.is_some() || self.fill.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
    }
    fn capture_form(&mut self, window: &ServoShellWindow) {
        let Some(view) = window.active_webview() else {
            return;
        };
        let expected = view.url().and_then(|u| origin(u.as_str()).ok());
        let Some(expected) = expected else {
            self.status = "Открой страницу входа на сайте.".into();
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.capture = Some(rx);
        view.evaluate_javascript(CAPTURE_SCRIPT, move |value| {
            let result = match value {
                Ok(servo::JSValue::String(text)) => serde_json::from_str::<Credential>(&text)
                    .ok()
                    .filter(|e| e.origin == expected)
                    .ok_or_else(|| "На странице не найден заполненный пароль.".into()),
                _ => Err("Не удалось прочитать форму входа.".into()),
            };
            let _ = tx.send(result);
        });
    }
    fn fill_form(&mut self, window: &ServoShellWindow, index: usize) {
        let Some(view) = window.active_webview() else {
            return;
        };
        let Some(url) = view.url() else {
            return;
        };
        let Some(entry) = browser::with(|b| b.vault.entries.get(index).cloned()) else {
            return;
        };
        match fill_script(url.as_str(), &entry) {
            Ok(script) => {
                let (tx, rx) = mpsc::channel();
                self.fill = Some(rx);
                view.evaluate_javascript(script, move |v| {
                    let _ = tx.send(matches!(v, Ok(servo::JSValue::Boolean(true))));
                });
            },
            Err(e) => self.status = e,
        }
    }
}
fn current_origin_for(url: &str) -> String {
    origin(url).unwrap_or_default()
}
