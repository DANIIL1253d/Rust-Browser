//! Event-driven native suggestions. Saved credentials never enter discovery scripts.
use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::Sender;

use rust_browser_core::passwords::{Credential, discovery_script, fill_script_for_target};
use servo::{ConnectionSecurity, EmbedderControlId, JSValue, WebView, WebViewId};

use super::{browser, icons};
use crate::window::ServoShellWindow;

#[derive(Clone, serde::Deserialize)]
struct Probe {
    origin: String,
    token: String,
    role: String,
    rect: [f32; 4],
}

#[derive(Clone)]
struct Request {
    generation: u64,
    view: WebViewId,
    url: String,
    origin: String,
    token: String,
}

struct PendingFill {
    request: Request,
    probe: Probe,
    entry: Credential,
    sender: Sender<bool>,
}

#[derive(Default)]
pub(crate) struct Autofill {
    generation: u64,
    request: Option<Request>,
    probe: Option<Probe>,
    open: bool,
    status: String,
    control: Option<EmbedderControlId>,
    pending_fill: Option<PendingFill>,
}

impl Autofill {
    pub(crate) fn invalidate(&mut self) {
        if let Some(pending) = self.pending_fill.take() {
            let _ = pending.sender.send(false);
        }
        self.generation = self.generation.wrapping_add(1);
        self.request = None;
        self.probe = None;
        self.open = false;
        self.status.clear();
        self.control = None;
    }

    fn current(&self, request: &Request) -> bool {
        self.request
            .as_ref()
            .is_some_and(|r| r.generation == request.generation && r.view == request.view)
    }
}

/// Fail closed for HTTP, certificate exceptions and missing/stale TLS evidence.
pub(crate) fn verified_origin(view: &WebView) -> Option<String> {
    let url = view.url()?;
    if url.scheme() != "https" {
        return None;
    }
    let info = view.connection_info()?;
    (info.security == ConnectionSecurity::Secure && info.url.origin() == url.origin())
        .then(|| url.origin().ascii_serialization())
}

fn still_current(view: &WebView, request: &Request) -> bool {
    view.id() == request.view
        && view.url().is_some_and(|u| u.as_str() == request.url)
        && verified_origin(view).as_deref() == Some(request.origin.as_str())
}

fn wake(flags: &(Rc<Cell<bool>>, Rc<Cell<bool>>)) {
    flags.0.set(true);
    flags.1.set(true);
}

fn new_request(window: &ServoShellWindow, view: &WebView, expected: String) -> Request {
    let mut state = window.autofill.borrow_mut();
    state.invalidate();
    let request = Request {
        generation: state.generation,
        view: view.id(),
        url: view.url().expect("Verified URL").to_string(),
        origin: expected,
        token: format!("{:?}-{}", view.id(), state.generation),
    };
    state.request = Some(request.clone());
    request
}

/// Called on an input-focus notification, and once at document completion for autofocus.
pub(crate) fn focused(
    window: &ServoShellWindow,
    view: WebView,
    control: Option<EmbedderControlId>,
) {
    if window.active_webview().is_none_or(|v| v.id() != view.id())
        || window.has_restoring_target(view.id())
    {
        return;
    }
    // The completion hook must not reopen a menu already discovered on focus.
    if control.is_none()
        && window
            .autofill
            .borrow()
            .request
            .as_ref()
            .is_some_and(|r| still_current(&view, r))
    {
        return;
    }
    discover(window, view, true, control);
}

fn discover(
    window: &ServoShellWindow,
    view: WebView,
    focused: bool,
    control: Option<EmbedderControlId>,
) {
    let Some(expected) = verified_origin(&view) else {
        let mut state = window.autofill.borrow_mut();
        state.invalidate();
        if !focused {
            state.open = true;
            state.status =
                "Заполнение доступно на HTTPS-странице с проверенным сертификатом.".into();
        }
        return;
    };
    let matching = browser::with(|b| b.vault.matching(&expected).len());
    if matching == 0 && focused {
        window.autofill.borrow_mut().invalidate();
        return;
    }
    let request = new_request(window, &view, expected);
    {
        let mut state = window.autofill.borrow_mut();
        state.control = control;
        state.open = !focused;
        state.status = if matching == 0 {
            "Для этого точного адреса сайта нет сохранённых записей."
        } else {
            "Ищем поле входа…"
        }
        .into();
    }
    let Ok(script) = discovery_script(&request.origin, &request.token, focused) else {
        return;
    };
    let state = window.autofill.clone();
    let flags = window.autofill_update_flags();
    let checked_view = view.clone();
    view.evaluate_javascript(script, move |value| {
        let mut state = state.borrow_mut();
        if !state.current(&request) || !still_current(&checked_view, &request) { return; }
        let probe = match value {
            Ok(JSValue::String(text)) => serde_json::from_str::<Probe>(&text).ok(),
            _ => None,
        }.filter(|p| p.origin == request.origin && p.token == request.token &&
            matches!(p.role.as_str(), "username" | "password") && p.rect.iter().all(|n| n.is_finite()));
        if let Some(probe) = probe {
            state.probe = Some(probe);
            state.open = true;
            state.status.clear();
        } else {
            state.probe = None;
            state.open = !focused;
            state.status = "Выбери поле логина или текущего пароля. Поля регистрации и одноразовых кодов исключены.".into();
        }
        wake(&flags);
    });
}

pub(crate) fn key_button(ui: &mut egui::Ui, window: &ServoShellWindow) {
    if icons::button(
        ui,
        icons::Icon::Key,
        "Сохранённые пароли для этого сайта",
        true,
    )
    .clicked()
    {
        if let Some(view) = window.active_webview() {
            discover(window, view, false, None);
        }
    }
}

fn fill_bound(
    window: &ServoShellWindow,
    request: Request,
    probe: Probe,
    entry: Credential,
    sender: Option<Sender<bool>>,
) {
    let Some(view) = window
        .active_webview()
        .filter(|v| still_current(v, &request))
    else {
        if let Some(sender) = sender {
            let _ = sender.send(false);
        }
        return;
    };
    if !window.autofill.borrow().current(&request) || probe.origin != entry.origin {
        if let Some(sender) = sender {
            let _ = sender.send(false);
        }
        return;
    }
    let Ok(script) = fill_script_for_target(&request.origin, &entry, Some(&probe.token)) else {
        if let Some(sender) = sender {
            let _ = sender.send(false);
        }
        return;
    };
    let state = window.autofill.clone();
    let flags = window.autofill_update_flags();
    let checked_view = view.clone();
    view.evaluate_javascript(script, move |value| {
        let mut state = state.borrow_mut();
        let current = state.current(&request) && still_current(&checked_view, &request);
        let success = current && matches!(value, Ok(JSValue::Boolean(true)));
        if let Some(sender) = sender {
            let _ = sender.send(success);
        }
        if !current {
            return;
        }
        state.probe = None; // A second fill needs fresh discovery/consent.
        state.status = if success {
            "Данные заполнены. Продолжи вход на сайте."
        } else {
            "Поле или страница изменились. Выбери поле ещё раз."
        }
        .into();
        wake(&flags);
    });
}

/// Manager Fill is explicit too, but first binds a fresh document/target without secrets.
pub(crate) fn fill_from_manager(
    window: &ServoShellWindow,
    index: usize,
    sender: Sender<bool>,
) -> Result<(), String> {
    let view = window.active_webview().ok_or("Открой страницу входа.")?;
    let expected =
        verified_origin(&view).ok_or("Нужна HTTPS-страница с проверенным сертификатом.")?;
    let entry =
        browser::with(|b| b.vault.entries.get(index).cloned()).ok_or("Запись уже удалена.")?;
    if entry.origin != expected {
        return Err("Эта запись сохранена для другого сайта.".into());
    }
    let request = new_request(window, &view, expected);
    let script = discovery_script(&request.origin, &request.token, false)?;
    let state = window.autofill.clone();
    let flags = window.autofill_update_flags();
    let checked_view = view.clone();
    view.evaluate_javascript(script, move |value| {
        if !state.borrow().current(&request) || !still_current(&checked_view, &request) {
            let _ = sender.send(false);
            return;
        }
        let probe = match value {
            Ok(JSValue::String(text)) => serde_json::from_str::<Probe>(&text).ok(),
            _ => None,
        };
        let Some(probe) = probe.filter(|p| p.origin == request.origin && p.token == request.token)
        else {
            let _ = sender.send(false);
            return;
        };
        // JavaScriptEvaluator holds a RefMut while invoking this callback. Submit
        // the second evaluation from the next native UI update, never reentrantly.
        state.borrow_mut().pending_fill = Some(PendingFill {
            request,
            probe,
            entry,
            sender,
        });
        wake(&flags);
    });
    Ok(())
}

pub(crate) fn show(ui: &mut egui::Ui, window: &ServoShellWindow) {
    let pending = window.autofill.borrow_mut().pending_fill.take();
    if let Some(pending) = pending {
        fill_bound(
            window,
            pending.request,
            pending.probe,
            pending.entry,
            Some(pending.sender),
        );
    }
    let (mut open, request, probe, status) = {
        let state = window.autofill.borrow();
        (
            state.open,
            state.request.clone(),
            state.probe.clone(),
            state.status.clone(),
        )
    };
    if !open {
        return;
    }
    if let Some(request) = &request {
        if window
            .active_webview()
            .is_none_or(|v| !still_current(&v, request))
        {
            window.autofill.borrow_mut().invalidate();
            return;
        }
    }
    let entries = request
        .as_ref()
        .map(|r| {
            browser::with(|b| {
                b.vault
                    .entries
                    .iter()
                    .filter(|e| e.origin == r.origin)
                    .map(|e| (e.origin.clone(), e.username.clone()))
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_default();
    let mut selected = None;
    let area = ui.available_rect_before_wrap();
    egui::Window::new("Сохранённые пароли")
        .id(egui::Id::new("browser_autofill"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(320.0)
        .default_pos(egui::pos2(
            (area.right() - 340.0).max(area.left()),
            area.top() + 8.0,
        ))
        .show(ui, |ui| {
            if let Some(request) = &request {
                ui.weak(&request.origin);
            }
            if !status.is_empty() {
                ui.label(&status);
            }
            if entries.is_empty() && status.is_empty() {
                ui.label("Для этого точного адреса сайта нет сохранённых записей.");
            }
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for (site, username) in &entries {
                        if ui
                            .add_enabled(
                                probe.is_some(),
                                egui::Button::new(if username.is_empty() {
                                    "Без логина"
                                } else {
                                    username
                                })
                                .min_size(egui::vec2(280.0, 30.0)),
                            )
                            .clicked()
                        {
                            selected = Some((site.clone(), username.clone()));
                        }
                    }
                });
            ui.small("Только этот сайт · без отправки формы");
        });
    window.autofill.borrow_mut().open = open;
    if let (Some((site, username)), Some(request), Some(probe)) = (selected, request, probe) {
        // Fetch the password only after the explicit native account click.
        if let Some(entry) = browser::with(|b| {
            b.vault
                .entries
                .iter()
                .find(|e| e.origin == site && e.username == username)
                .cloned()
        }) {
            fill_bound(window, request, probe, entry, None);
        }
    }
}
