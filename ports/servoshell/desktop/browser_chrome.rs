use super::{
    browser,
    icons::{self, Icon},
};
use crate::{
    running_app_state::{RunningAppState, UserInterfaceCommand},
    window::ServoShellWindow,
};
use egui::{Color32, FontId, Key, Modifiers, RichText, Vec2};
use rust_browser_core::{BrowserTheme, RouteMode, SearchEngine};

pub const TAB_BG: Color32 = Color32::from_rgb(20, 20, 21);
pub const BAR_BG: Color32 = Color32::from_rgb(35, 35, 37);
pub const ACCENT: Color32 = Color32::from_rgb(247, 112, 67);
const ACCENTS: [Color32; 4] = [
    ACCENT,
    Color32::from_rgb(70, 142, 235),
    Color32::from_rgb(43, 166, 135),
    Color32::from_rgb(185, 109, 221),
];

#[derive(Default)]
pub struct Chrome {
    settings: bool,
    customization: bool,
    history: bool,
    bookmarks: bool,
    tab_search: bool,
    tab_query: String,
    focus_tab_search: bool,
    subscription: String,
}
pub fn configure(ctx: &egui::Context) {
    let appearance = browser::with(|b| b.profile.appearance.clone());
    let style_id = egui::Id::new("rust-browser-appearance");
    if ctx
        .data(|data| data.get_temp::<rust_browser_core::Appearance>(style_id))
        .as_ref()
        == Some(&appearance)
    {
        return;
    }
    ctx.data_mut(|data| data.insert_temp(style_id, appearance.clone()));
    let dark = appearance.theme == BrowserTheme::Dark;
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    if dark {
        visuals.panel_fill = BAR_BG;
        visuals.window_fill = Color32::from_rgb(25, 25, 27);
        visuals.extreme_bg_color = TAB_BG;
        visuals.widgets.active.weak_bg_fill = Color32::from_rgb(52, 52, 55);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(46, 46, 49);
        visuals.override_text_color = Some(Color32::from_rgb(237, 237, 237));
        visuals.weak_text_color = Some(Color32::from_rgb(161, 161, 166));
    } else {
        visuals.panel_fill = Color32::from_rgb(242, 243, 247);
        visuals.window_fill = Color32::from_rgb(250, 250, 252);
        visuals.extreme_bg_color = Color32::from_rgb(225, 227, 233);
    }
    visuals.hyperlink_color = ACCENTS[appearance.accent.min(ACCENTS.len() - 1)];
    if !dark {
        visuals.hyperlink_color = [
            Color32::from_rgb(185, 71, 32),
            Color32::from_rgb(32, 93, 176),
            Color32::from_rgb(17, 117, 95),
            Color32::from_rgb(128, 66, 160),
        ][appearance.accent.min(3)];
    }
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.menu_corner_radius = egui::CornerRadius::same(10);
    visuals.widgets.inactive.weak_bg_fill = if dark {
        Color32::from_rgb(35, 35, 37)
    } else {
        Color32::from_gray(240)
    };
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(
        1.0_f32,
        if dark {
            Color32::from_gray(52)
        } else {
            Color32::from_gray(220)
        },
    );
    visuals.selection.bg_fill = visuals.hyperlink_color.gamma_multiply(0.4);
    if appearance.glass {
        visuals.window_fill = Color32::from_rgba_unmultiplied(
            if dark { 28 } else { 248 },
            if dark { 28 } else { 248 },
            if dark { 32 } else { 252 },
            228,
        );
        visuals.window_stroke = egui::Stroke::new(
            1.0_f32,
            Color32::from_white_alpha(if dark { 36 } else { 160 }),
        );
        visuals.popup_shadow = egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(64),
        };
    }
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.interact_size = Vec2::new(32.0, 28.0);
        s.spacing.menu_margin = egui::Margin::same(8);
        s.animation_time = if appearance.animations { 0.18 } else { 0.0 };
        s.text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        s.text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(24.0));
        s.text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.0));
    });
}
fn icon(ui: &mut egui::Ui, label: &str, tip: &str, enabled: bool) -> egui::Response {
    let kind = match label {
        "←" => Icon::Back,
        "→" => Icon::Forward,
        "↻" => Icon::Reload,
        "⌂" => Icon::Home,
        "★" => Icon::StarFilled,
        _ => Icon::Star,
    };
    icons::button(ui, kind, tip, enabled)
}

fn connection_button(ui: &mut egui::Ui, window: &ServoShellWindow) {
    use servo::ConnectionSecurity as Security;
    let view = window.active_webview();
    let restoring = view.as_ref().is_some_and(|v| window.has_restoring_target(v.id()));
    // Use the committed document, never the editable address field.
    let url = view.as_ref().and_then(|v| if restoring { url::Url::parse(&window.tab_page(v).url).ok() } else { v.url() });
    let info = view.as_ref().and_then(|v| if restoring { None } else { v.connection_info() });
    let pending = restoring || (info.is_none()
        && view.as_ref().is_some_and(|v| v.load_status() == servo::LoadStatus::Started));
    let security = info.as_ref().map(|i| i.security).or_else(|| {
        url.clone()
            .map(|u| servo::ConnectionInfo::from_response(u, None, false).security)
    });
    let (kind, title, description) = match security {
        Some(Security::Secure) => (
            Icon::Lock,
            "Соединение защищено",
            "Основная страница получена по HTTPS. TLS-проверка сертификата пройдена. Это подтверждает шифрование соединения, но не надёжность самого сайта.",
        ),
        Some(Security::Unverified) => (
            Icon::LockOpen,
            "Сертификат не проверен",
            "Используется HTTPS, но проверка сертификата отключена, обойдена или не подтверждена. Не вводи здесь пароли и платёжные данные.",
        ),
        Some(Security::Insecure) => (
            Icon::LockOpen,
            "Соединение не защищено",
            "Сайт использует HTTP. Передаваемые данные не защищены шифрованием HTTPS.",
        ),
        Some(Security::Failed) => (
            Icon::Warning,
            "Не удалось установить соединение",
            "Загрузка основной страницы завершилась ошибкой. Защищённое соединение с сайтом не подтверждено.",
        ),
        Some(Security::Local) => (
            Icon::Info,
            "Локальная страница",
            "Это внутренняя страница браузера или файл на компьютере. Сетевое соединение для неё не проверяется.",
        ),
        _ if pending => (
            Icon::Info,
            "Проверяем соединение…",
            "Страница загружается. Статус защиты появится после получения сведений о соединении основной страницы.",
        ),
        _ => (
            Icon::Info,
            "Нет подтверждения защиты",
            "Браузер ещё не получил подтверждённые сведения о соединении основной страницы. Один адрес https:// не означает успешную TLS-проверку.",
        ),
    };
    let color = match security {
        Some(Security::Secure) => Color32::from_rgb(78, 177, 137),
        Some(Security::Failed | Security::Unverified | Security::Insecure) => {
            Color32::from_rgb(224, 155, 86)
        },
        _ => ui.visuals().weak_text_color(),
    };
    let response = ui.menu_button("  ", |ui| {
        ui.set_width(320.0);
        ui.strong(title);
        if let Some(url) = &url {
            if let Some(host) = url.host_str() { ui.weak(host); }
        }
        ui.separator();
        ui.label(description);
        if let Some(protocol) = info.as_ref().and_then(|i| i.protocol.as_ref()) {
            ui.weak(format!("Протокол: {protocol}"));
        }
        ui.small("Состояние относится к основной странице. Полная проверка смешанного содержимого пока не отображается.");
    }).response;
    icons::paint(
        ui,
        egui::Rect::from_center_size(response.rect.center(), Vec2::splat(18.0)),
        kind,
        color,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    response.on_hover_text(title);
}

pub fn window_controls(
    ui: &mut egui::Ui,
    native: &winit::window::Window,
    window: &ServoShellWindow,
    state: &RunningAppState,
) {
    ui.spacing_mut().item_spacing.x = 0.0;
    for (kind, title) in [
        (0, "Свернуть"),
        (
            1,
            if native.is_maximized() {
                "Восстановить"
            } else {
                "Развернуть"
            },
        ),
        (2, "Закрыть"),
    ] {
        let (rect, response) = ui.allocate_exact_size(Vec2::new(44.0, 32.0), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
        if response.hovered() || response.has_focus() {
            ui.painter().rect_filled(
                rect,
                3,
                if kind == 2 {
                    Color32::from_rgb(196, 43, 52)
                } else {
                    ui.visuals().widgets.hovered.weak_bg_fill
                },
            );
        }
        let color = if kind == 2 && response.hovered() {
            Color32::WHITE
        } else {
            ui.visuals().text_color()
        };
        let stroke = egui::Stroke::new(1.2_f32, color);
        let center = rect.center();
        let square = egui::Rect::from_center_size(center, Vec2::splat(10.0));
        let painter = ui.painter();
        match kind {
            0 => {
                painter.line_segment(
                    [center + Vec2::new(-5.0, 0.0), center + Vec2::new(5.0, 0.0)],
                    stroke,
                );
            },
            1 => {
                if native.is_maximized() {
                    painter.rect_stroke(
                        square.translate(Vec2::new(2.0, -2.0)),
                        0,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                    painter.rect_filled(square, 0, ui.visuals().extreme_bg_color);
                }
                painter.rect_stroke(square, 0, stroke, egui::StrokeKind::Inside);
            },
            _ => {
                painter.line_segment([square.left_top(), square.right_bottom()], stroke);
                painter.line_segment([square.right_top(), square.left_bottom()], stroke);
            },
        }
        if response.on_hover_text(title).clicked() {
            match kind {
                0 => native.set_minimized(true),
                1 => native.set_maximized(!native.is_maximized()),
                _ => {
                    browser::with(|b| b.capture(state));
                    window.schedule_close();
                },
            }
        }
    }
}
impl Chrome {
    pub(crate) fn open_tab_search(&mut self) {
        self.tab_search = true;
        self.focus_tab_search = true;
    }
    pub fn navigation(
        &mut self,
        ui: &mut egui::Ui,
        state: &RunningAppState,
        window: &ServoShellWindow,
        location: &mut String,
        dirty: &mut bool,
        back: bool,
        forward: bool,
        loading: bool,
    ) {
        browser::with(|b| b.poll(state));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(
                if browser::with(|b| b.busy()) {
                    150
                } else {
                    3000
                },
            ));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.set_min_height(if browser::with(|b| b.profile.appearance.compact) {
                31.0
            } else {
                39.0
            });
            if icon(ui, "←", "Назад · Alt+←", back).clicked() {
                *dirty = false;
                window.queue_user_interface_command(UserInterfaceCommand::Back);
            }
            if icon(ui, "→", "Вперёд · Alt+→", forward).clicked() {
                *dirty = false;
                window.queue_user_interface_command(UserInterfaceCommand::Forward);
            }
            if loading {
                ui.add_sized([32.0, 28.0], egui::Spinner::new().size(17.0));
            } else if icon(ui, "↻", "Обновить · Ctrl+R", true).clicked() {
                window.queue_user_interface_command(UserInterfaceCommand::Reload);
            }
            if icon(ui, "⌂", "Новая вкладка", true).clicked() {
                window
                    .queue_user_interface_command(UserInterfaceCommand::Go("servo:newtab".into()));
            }
            let width = (ui.available_width() - 80.0).max(100.0);
            egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(
                    12,
                    if browser::with(|b| b.profile.appearance.compact) {
                        4
                    } else {
                        8
                    },
                ))
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        connection_button(ui, window);
                        let id = egui::Id::new("location_input");
                        let response = ui.add_sized(
                            [width - 56.0, 28.0],
                            egui::TextEdit::singleline(location)
                                .id(id)
                                .vertical_align(egui::Align::Center)
                                .margin(egui::Vec2::ZERO)
                                .frame(egui::Frame::NONE)
                                .font(egui::TextStyle::Body)
                                .hint_text("Поиск или адрес сайта"),
                        );
                        if response.changed() {
                            *dirty = true;
                        }
                        if ui.input_mut(|i| {
                            i.consume_key(Modifiers::COMMAND, Key::L)
                                || i.consume_key(Modifiers::ALT, Key::D)
                        }) {
                            response.request_focus();
                        }
                        if response.gained_focus() {
                            if let Some(mut edit) =
                                egui::text_edit::TextEditState::load(ui.ctx(), id)
                            {
                                edit.cursor
                                    .set_char_range(Some(egui::text::CCursorRange::two(
                                        egui::text::CCursor::new(0),
                                        egui::text::CCursor::new(location.chars().count()),
                                    )));
                                edit.store(ui.ctx(), id);
                            }
                        }
                        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            window.queue_user_interface_command(UserInterfaceCommand::Go(
                                location.clone(),
                            ));
                        }
                    });
                });
            let bookmarked =
                browser::with(|b| b.profile.bookmarks.iter().any(|p| p.url == *location));
            let bookmark_shortcut = ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::D));
            if icon(
                ui,
                if bookmarked { "★" } else { "☆" },
                "Добавить закладку · Ctrl+D",
                true,
            )
            .clicked()
                || bookmark_shortcut
            {
                let title = window
                    .active_webview()
                    .and_then(|v| v.page_title())
                    .unwrap_or_else(|| location.clone());
                browser::with(|b| {
                    b.profile.toggle_bookmark(location, &title);
                    b.save();
                });
            }
            let menu = ui.menu_button("    ", |ui| {
                ui.set_min_width(272.0);
                if icons::menu_row(ui, Icon::Plus, "Новая вкладка", "Ctrl+T").clicked()
                {
                    window.queue_user_interface_command(UserInterfaceCommand::NewWebView);
                    ui.close();
                }
                if icons::menu_row(ui, Icon::Bookmarks, "Закладки", "").clicked() {
                    self.bookmarks = true;
                    ui.close();
                }
                if icons::menu_row(ui, Icon::Search, "Поиск по вкладкам", "Ctrl+Shift+A").clicked()
                {
                    self.tab_search = true;
                    self.focus_tab_search = true;
                    ui.close();
                }
                if ui
                    .add_enabled_ui(
                        browser::with(|b| !b.profile.recently_closed.is_empty()),
                        |ui| {
                            icons::menu_row(
                                ui,
                                Icon::History,
                                "Восстановить вкладку",
                                "Ctrl+Shift+T",
                            )
                        },
                    )
                    .inner
                    .clicked()
                {
                    window.queue_user_interface_command(UserInterfaceCommand::ReopenClosedWebView);
                    ui.close();
                }
                if icons::menu_row(ui, Icon::History, "История", "").clicked() {
                    self.history = true;
                    ui.close();
                }
                if icons::menu_row(ui, Icon::Key, "Менеджер паролей", "").clicked() {
                    browser::open_password_manager(window.id());
                    ui.close();
                }
                ui.separator();
                if icons::menu_row(ui, Icon::Network, "Сеть и подписка", "").clicked()
                {
                    self.settings = true;
                    ui.close();
                }
                if icons::menu_row(ui, Icon::Settings, "Настройки", "").clicked() {
                    self.customization = true;
                    ui.close();
                }
                ui.separator();
                let (mode, connected) = browser::with(|b| (b.profile.route_mode, b.connected()));
                ui.weak(match mode {
                    RouteMode::Direct => "Сайты открываются напрямую",
                    RouteMode::Auto if connected => "Авто · подписка подключена",
                    RouteMode::Proxy if connected => "Все сайты через подписку",
                    _ => "Подписка не подключена",
                });
                ui.weak("Rust Browser 0.1 · Servo 0.7");
            });
            icons::paint(
                ui,
                egui::Rect::from_center_size(menu.response.rect.center(), Vec2::splat(18.0)),
                Icon::Menu,
                ui.visuals().weak_text_color(),
            );
            menu.response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Меню браузера")
            });
            menu.response.on_hover_text("Меню браузера");
        });
    }
    pub fn error_page(&mut self, ctx: &egui::Context, window: &ServoShellWindow, rect: egui::Rect) {
        let Some(view) = window.active_webview() else {
            return;
        };
        if !view
            .page_title()
            .is_some_and(|title| title == "Error loading page")
        {
            return;
        }
        let (mode, connected) = browser::with(|b| (b.profile.route_mode, b.connected()));
        egui::Area::new(egui::Id::new("network-error"))
            .fixed_pos(rect.min).movable(false).order(egui::Order::Middle)
            .show(ctx, |ui| {
                ui.set_min_size(rect.size()); ui.set_max_size(rect.size());
                ui.painter().rect_filled(rect, 0, ui.visuals().panel_fill);
                ui.add_space((rect.height()*0.28).max(24.0));
                ui.vertical_centered(|ui| {
                    ui.heading("Не удалось открыть сайт");
                    ui.add_space(12.0);
                    ui.label(if mode == RouteMode::Direct { "Сейчас сайты открываются напрямую. Для этого сайта может понадобиться подписка" }
                        else if !connected { "Подписка не подключена. Импортируй ссылку и выбери сервер" }
                        else { "Выбранный сервер не открыл сайт. Проверь соединение или попробуй другой сервер" });
                    ui.add_space(20.0);
                    if ui.button("Сеть и подписка").clicked() { self.settings = true; }
                    if ui.button("Повторить загрузку").clicked() { window.queue_user_interface_command(UserInterfaceCommand::Reload); }
                });
            });
    }
    pub fn windows(
        &mut self,
        ctx: &egui::Context,
        state: &RunningAppState,
        window: &ServoShellWindow,
    ) {
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.customization = false;
            self.settings = false;
            self.history = false;
            self.bookmarks = false;
            self.tab_search = false;
        }
        {
            let mut open = self.tab_search;
            let mut selected = None;
            egui::Window::new("Поиск по вкладкам")
                .open(&mut open)
                .default_width(560.0)
                .show(ctx, |ui| {
                    let input = ui.add(
                        egui::TextEdit::singleline(&mut self.tab_query)
                            .desired_width(f32::INFINITY)
                            .hint_text("Название или адрес сайта"),
                    );
                    if self.focus_tab_search {
                        input.request_focus();
                        self.focus_tab_search = false;
                    }
                    let enter = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    let mut count = 0;
                    egui::ScrollArea::vertical()
                        .max_height(420.0)
                        .show(ui, |ui| {
                            for (id, view) in window.webviews() {
                                let page = window.tab_page(&view);
                                if !page.matches(&self.tab_query) {
                                    continue;
                                }
                                count += 1;
                                if ui
                                    .selectable_label(
                                        window.active_webview().is_some_and(|v| v.id() == id),
                                        &page.title,
                                    )
                                    .on_hover_text(&page.url)
                                    .clicked()
                                    || (enter && count == 1)
                                {
                                    selected = Some(id);
                                }
                                ui.small(&page.url);
                                ui.add_space(6.0);
                            }
                            if count == 0 {
                                ui.weak("Подходящих вкладок нет");
                            }
                        });
                });
            if let Some(id) = selected {
                window.activate_webview(id);
                open = false;
            }
            self.tab_search = open;
        }
        {
            let mut open = self.customization;
            let mut done = false;
            egui::Window::new("Настройки и внешний вид")
                .frame(
                    egui::Frame::window(&ctx.style())
                        .inner_margin(24)
                        .corner_radius(12),
                )
                .open(&mut open)
                .default_width(560.0)
                .resizable(false)
                .show(ctx, |ui| {
                    browser::with(|b| {
                        let before =
                            serde_json::to_string(&b.profile.appearance).unwrap_or_default();
                        let a = &mut b.profile.appearance;
                        ui.heading("Внешний вид");
                        ui.label("Тема панелей браузера");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut a.theme, BrowserTheme::Dark, "Тёмная");
                            ui.selectable_value(&mut a.theme, BrowserTheme::Light, "Светлая");
                        });
                        ui.add_space(8.0);
                        ui.label("Цвет акцента");
                        ui.horizontal(|ui| {
                            for (i, (color, name)) in ACCENTS
                                .iter()
                                .zip(["Оранжевый", "Синий", "Зелёный", "Сиреневый"])
                                .enumerate()
                            {
                                if ui
                                    .selectable_label(
                                        a.accent == i,
                                        RichText::new("●").size(25.0).color(*color),
                                    )
                                    .on_hover_text(name)
                                    .clicked()
                                {
                                    a.accent = i;
                                }
                            }
                        });
                        ui.checkbox(&mut a.glass, "Эффект стекла в меню и окнах");
                        ui.checkbox(&mut a.animations, "Анимации интерфейса");
                        ui.checkbox(&mut a.compact, "Компактная панель навигации");
                        ui.separator();
                        ui.heading("Основные");
                        ui.checkbox(
                            &mut a.restore_session,
                            "Восстанавливать вкладки при запуске",
                        );
                        ui.label("Поиск из адресной строки");
                        ui.horizontal_wrapped(|ui| {
                            for (engine, name) in [
                                (SearchEngine::Brave, "Brave"),
                                (SearchEngine::Google, "Google"),
                                (SearchEngine::Yandex, "Яндекс"),
                                (SearchEngine::DuckDuckGo, "DuckDuckGo"),
                            ] {
                                ui.selectable_value(&mut a.search_engine, engine, name);
                            }
                        });
                        if ui.button("Вернуть настройки внешнего вида").clicked()
                        {
                            *a = Default::default();
                        }
                        if before != serde_json::to_string(a).unwrap_or_default() {
                            b.save();
                            ctx.request_repaint();
                        }
                    });
                    ui.separator();
                    if ui
                        .button("Импортировать закладки и историю Brave")
                        .clicked()
                    {
                        browser::with(|b| b.import_brave());
                    }
                    if ui.button("Дополнительные настройки движка").clicked()
                    {
                        window.queue_user_interface_command(UserInterfaceCommand::Go(
                            "servo:preferences".into(),
                        ));
                    }
                    ui.small("Изменения сохраняются автоматически");
                    if ui.button("Готово").clicked() {
                        done = true;
                    }
                });
            self.customization = open && !done;
        }
        {
            let mut open = self.settings;
            let mut done = false;
            egui::Window::new("Подключение").open(&mut open).default_width(560.0).resizable(false).show(ctx,|ui| {
                browser::with(|b| {
                    ui.heading("Как открывать сайты");
                    ui.horizontal(|ui| {
                        for (mode,label) in [(RouteMode::Direct,"Напрямую"),(RouteMode::Auto,"Авто"),(RouteMode::Proxy,"Через подписку")] {
                            if ui.selectable_value(&mut b.profile.route_mode,mode,label).changed(){
                                if mode==RouteMode::Direct{b.disconnect();}else if !b.connected() && !b.busy(){b.connect();}
                                b.save();
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.label("В «Авто» сначала пробуем напрямую. Если соединение не установилось за 3 секунды — через подписку.");
                    ui.separator();
                    ui.label("Подписка из Happ или ссылка сервера");
                    ui.add(egui::TextEdit::singleline(&mut self.subscription).password(true).desired_width(f32::INFINITY).hint_text("https://… или vless://…"));
                    if ui.add_enabled(!b.busy() && !self.subscription.trim().is_empty(),egui::Button::new("Импортировать")).clicked(){b.import_subscription(std::mem::take(&mut self.subscription));}
                    ui.small("Ссылка не сохраняется. Ключи серверов защищены Windows DPAPI.");
                    if !b.nodes.is_empty(){
                        let selected=b.nodes.get(b.selected).map(|n|n.name.clone()).unwrap_or_else(||"Выбери сервер".into());
                        egui::ComboBox::from_id_salt("subscription_node").width(400.0).selected_text(selected).show_ui(ui,|ui|{
                            for (i,node) in b.nodes.iter().enumerate(){ui.selectable_value(&mut b.selected,i,&node.name);}
                        });
                        ui.horizontal(|ui|{
                            if ui.add_enabled(!b.busy(),egui::Button::new("Подключить")).clicked(){
                                if b.profile.route_mode==RouteMode::Direct{b.profile.route_mode=RouteMode::Auto;}
                                b.save_nodes();b.save();b.connect();
                            }
                            if ui.button("Отключить").clicked(){b.profile.route_mode=RouteMode::Direct;b.disconnect();b.save();}
                        });
                    }
                    ui.horizontal(|ui|{if b.busy(){ui.spinner();}ui.label(&b.status);});
                    if let Some(error)=&b.error {ui.colored_label(ACCENT,error);}
                    ui.separator();
                    ui.collapsing("Исключения для сайтов",|ui|{
                        ui.label("Всегда напрямую — домены через запятую");
                        ui.text_edit_multiline(&mut b.profile.direct_domains);
                        ui.label("Всегда через подписку — домены через запятую");
                        ui.text_edit_multiline(&mut b.profile.proxy_domains);
                        ui.small("Пример: example.org. Правило включает поддомены. Прямое правило имеет приоритет.");
                        if ui.button("Сохранить правила").clicked(){b.save();}
                    });
                    ui.separator();
                    ui.small("Автоматический выбор обрабатывает ошибки DNS, TCP и TLS. Для блокировок после загрузки страницы добавь домен в исключения.");
                    b.poll(state);
                });
                if ui.button("Готово").clicked() { done = true; }
                if browser::with(|b| b.busy()) { ctx.request_repaint_after(std::time::Duration::from_millis(150)); }
            });
            self.settings = open && !done;
        }
        for (open, title, is_history) in [
            (&mut self.bookmarks, "Закладки", false),
            (&mut self.history, "История", true),
        ] {
            if !*open {
                continue;
            }
            egui::Window::new(title)
                .open(open)
                .default_width(540.0)
                .show(ctx, |ui| {
                    let pages = browser::with(|b| {
                        if is_history {
                            b.profile.history.clone()
                        } else {
                            b.profile.bookmarks.clone()
                        }
                    });
                    if is_history && ui.button("Очистить историю").clicked() {
                        browser::with(|b| {
                            b.profile.history.clear();
                            b.save();
                        });
                    }
                    if pages.is_empty() {
                        ui.weak("Здесь пока пусто");
                    }
                    egui::ScrollArea::vertical()
                        .max_height(440.0)
                        .show(ui, |ui| {
                            for page in pages {
                                ui.horizontal(|ui| {
                                    if ui
                                        .selectable_label(false, short(&page.title, 55))
                                        .on_hover_text(&page.url)
                                        .clicked()
                                    {
                                        window.queue_user_interface_command(
                                            UserInterfaceCommand::Go(page.url.clone()),
                                        );
                                    }
                                    if !is_history && ui.small_button("×").clicked() {
                                        browser::with(|b| {
                                            b.profile.toggle_bookmark(&page.url, &page.title);
                                            b.save();
                                        });
                                    }
                                });
                            }
                        });
                });
        }
    }
}
fn short(text: &str, count: usize) -> String {
    if text.chars().count() > count {
        format!("{}…", text.chars().take(count - 1).collect::<String>())
    } else {
        text.into()
    }
}
