/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::collections::HashMap;
#[cfg(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
))]
use std::fs;
#[cfg(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
))]
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use accesskit::Affine;
use dpi::PhysicalSize;
#[cfg(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
))]
use egui::{FontData, FontFamily};
use egui::{
    FontDefinitions, Id, Label, LayerId, Order, PaintCallback, Panel, Vec2, WidgetInfo, WidgetType,
    pos2,
};
use egui_glow::{CallbackFn, EguiGlow};
use egui_winit::EventResponse;
use euclid::{Length, Point2D, Rect, Scale, Size2D};
#[cfg(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
))]
use log::info;
use servo::{
    DeviceIndependentPixel, DevicePixel, Image, LoadStatus, OffscreenRenderingContext, PixelFormat,
    RenderingContext, WebView, WebViewId,
};
use url::Url;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::Window;

use crate::desktop::event_loop::AppEvent;
use crate::desktop::headed_window;
use crate::running_app_state::{RunningAppState, UserInterfaceCommand};
use crate::window::ServoShellWindow;

/// The user interface of a headed servoshell. Currently this is implemented via
/// egui.
pub struct Gui {
    password_manager: Option<(
        crate::window::ServoShellWindowId,
        super::passwords::PasswordUi,
    )>,
    browser_chrome: super::browser_chrome::Chrome,
    rendering_context: Rc<OffscreenRenderingContext>,
    context: EguiGlow,
    toolbar_height: Length<f32, DeviceIndependentPixel>,

    location: String,

    /// Whether the location has been edited by the user without clicking Go.
    location_dirty: bool,

    /// The [`LoadStatus`] of the active `WebView`.
    load_status: LoadStatus,

    /// The text to display in the status bar on the bottom of the window.
    status_text: Option<String>,

    /// Whether or not the current `WebView` can navigate backward.
    can_go_back: bool,

    /// Whether or not the current `WebView` can navigate forward.
    can_go_forward: bool,

    /// Handle to the GPU texture of the favicon.
    ///
    /// These need to be cached across egui draw calls.
    favicon_textures: HashMap<WebViewId, (egui::TextureHandle, egui::load::SizedTexture)>,

    /// AccessKit tree updates pending the next egui tick.
    /// This allows us to ensure that graft nodes are sent before the subtrees they graft.
    pending_accesskit_updates: Vec<accesskit::TreeUpdate>,
}

#[cfg(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
))]
fn load_cjk_fonts(font_candidates: &[(&str, &str)]) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut loaded_font_names = Vec::new();

    for (path_str, font_name) in font_candidates.iter() {
        let font_path = Path::new(path_str);
        if font_path.exists() {
            match fs::read(font_path) {
                Ok(bytes) => {
                    if !fonts.font_data.contains_key(*font_name) {
                        fonts
                            .font_data
                            .insert(font_name.to_string(), Arc::new(FontData::from_owned(bytes)));
                        loaded_font_names.push(font_name.to_string());
                        info!("Loaded font: {}", font_name);
                    }
                },
                Err(error) => {
                    info!("Failed to read font {}: {}", font_name, error);
                },
            }
        }
    }

    if !loaded_font_names.is_empty() &&
        let Some(proportional) = fonts.families.get_mut(&FontFamily::Proportional)
    {
        proportional.append(&mut loaded_font_names);
    }

    fonts
}

#[cfg(target_os = "windows")]
fn configure_fonts() -> FontDefinitions {
    load_cjk_fonts(&[(r"C:\Windows\Fonts\seguisb.ttf", "Segoe UI Semibold")])
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn configure_fonts() -> FontDefinitions {
    load_cjk_fonts(&[
        (
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "Noto Sans CJK",
        ), // Ubuntu/Debian
        (
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "Noto Sans CJK",
        ), // Fedora/Arch
        // FreeBSD splits the Noto CJK fonts into regional subsets
        (
            "/usr/local/share/fonts/noto/NotoSansCJKhk-Regular.otf",
            "Noto Sans CJK HK",
        ),
        (
            "/usr/local/share/fonts/noto/NotoSansCJKjp-Regular.otf",
            "Noto Sans CJK JP",
        ),
        (
            "/usr/local/share/fonts/noto/NotoSansCJKkr-Regular.otf",
            "Noto Sans CJK KR",
        ),
        (
            "/usr/local/share/fonts/noto/NotoSansCJKsc-Regular.otf",
            "Noto Sans CJK SC",
        ),
        (
            "/usr/local/share/fonts/noto/NotoSansCJKtc-Regular.otf",
            "Noto Sans CJK TC",
        ),
        (
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "WenQuanYi Micro Hei",
        ), // common fallback
        (
            "/usr/local/share/fonts/wqy/wqy-microhei.ttc",
            "WenQuanYi Micro Hei",
        ), // FreeBSD
    ])
}

#[cfg(target_os = "macos")]
fn configure_fonts() -> FontDefinitions {
    load_cjk_fonts(&[
        (
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "Apple SD Gothic Neo",
        ), // Korean
        ("/System/Library/Fonts/STHeiti Medium.ttc", "STHeiti Medium"), // Chinese + Japanese
    ])
}

impl Drop for Gui {
    fn drop(&mut self) {
        self.rendering_context
            .make_current()
            .expect("Could not make window RenderingContext current");
        self.context.destroy();
    }
}

impl Gui {
    pub(crate) fn new(
        winit_window: &Window,
        event_loop: &ActiveEventLoop,
        event_loop_proxy: EventLoopProxy<AppEvent>,
        rendering_context: Rc<OffscreenRenderingContext>,
        initial_url: Url,
        password_source: Option<crate::window::ServoShellWindowId>,
    ) -> Self {
        rendering_context
            .make_current()
            .expect("Could not make window RenderingContext current");
        let mut context = EguiGlow::new(
            event_loop,
            rendering_context.glow_gl_api(),
            None,
            None,
            false,
        );

        let font_definitions = configure_fonts();
        context.egui_ctx.set_fonts(font_definitions);
        super::browser_chrome::configure(&context.egui_ctx);
        let repaint_proxy = event_loop_proxy.clone();
        let window_id = crate::window::ServoShellWindowId::from(u64::from(winit_window.id()));
        context.egui_ctx.set_request_repaint_callback(move |info| {
            let _ = repaint_proxy.send_event(AppEvent::GuiRepaint(window_id, info.delay));
        });

        context
            .egui_winit
            .init_accesskit(event_loop, winit_window, event_loop_proxy);
        winit_window.set_visible(true);

        context.egui_ctx.options_mut(|options| {
            // Disable the builtin egui handlers for the Ctrl+Plus, Ctrl+Minus and Ctrl+0
            // shortcuts as they don't work well with servoshell's `device-pixel-ratio` CLI argument.
            options.zoom_with_keyboard = false;

            // On platforms where winit fails to obtain a system theme, fall back to a light theme
            // since it is the more common default.
            options.fallback_theme = egui::Theme::Dark;
        });

        Self {
            password_manager: password_source.map(|source| (source, Default::default())),
            browser_chrome: Default::default(),
            rendering_context,
            context,
            toolbar_height: Default::default(),
            location: initial_url.to_string(),
            location_dirty: false,
            load_status: LoadStatus::Complete,
            status_text: None,
            can_go_back: false,
            can_go_forward: false,
            favicon_textures: Default::default(),
            pending_accesskit_updates: vec![],
        }
    }

    pub(crate) fn open_tab_search(&mut self) {
        self.browser_chrome.open_tab_search();
        self.context.egui_ctx.request_repaint();
    }
    pub(crate) fn has_keyboard_focus(&self) -> bool {
        self.context
            .egui_ctx
            .memory(|memory| memory.focused().is_some())
    }

    pub(crate) fn surrender_focus(&self) {
        self.context.egui_ctx.memory_mut(|memory| {
            if let Some(focused) = memory.focused() {
                memory.surrender_focus(focused);
            }
        });
    }

    pub(crate) fn on_window_event(
        &mut self,
        winit_window: &Window,
        event: &WindowEvent,
    ) -> EventResponse {
        self.context.on_window_event(winit_window, event)
    }

    /// The height of the top toolbar of this user inteface ie the distance from the top of the
    /// window to the position of the `WebView`.
    pub(crate) fn toolbar_height(&self) -> Length<f32, DeviceIndependentPixel> {
        self.toolbar_height
    }

    /// Return true iff the given position is over the egui toolbar.
    pub(crate) fn is_in_egui_toolbar_rect(
        &self,
        position: Point2D<f32, DeviceIndependentPixel>,
    ) -> bool {
        self.password_manager.is_some()
            || self.context.egui_ctx.wants_pointer_input()
            || self.context.egui_ctx.is_using_pointer()
            || position.y < self.toolbar_height.get()
            || self
                .context
                .egui_ctx
                .layer_id_at(pos2(position.x, position.y))
                .is_some_and(|layer| matches!(layer.order, Order::Middle | Order::Foreground))
    }

    /// Create a frameless button with square sizing, as used in the toolbar.
    fn toolbar_button(text: &str) -> egui::Button<'_> {
        egui::Button::new(text)
            .frame(false)
            .min_size(Vec2 { x: 20.0, y: 20.0 })
    }

    /// Draws a fixed-width browser tab and queues the appropriate [`UserInterfaceCommand`]s.
    fn browser_tab(
        ui: &mut egui::Ui,
        window: &ServoShellWindow,
        webview: WebView,
        favicon_texture: Option<egui::load::SizedTexture>,
    ) {
        let page = window.tab_page(&webview);
        let label = page.title;
        let deferred = window.is_deferred(webview.id());

        let inactive_bg_color = ui.visuals().extreme_bg_color;
        let active_bg_color = ui.visuals().widgets.active.weak_bg_fill;
        let active = window.active_webview().map(|webview| webview.id()) == Some(webview.id());

        // Reserve every part of the tab before painting: titles and missing favicons cannot
        // change its width, and the close target never overlaps the title target.
        let (_, rect) = ui.allocate_space(Vec2::new(208.0, 30.0));
        let close_rect = egui::Rect::from_center_size(
            pos2(rect.right() - 18.0, rect.center().y),
            Vec2::splat(24.0),
        );
        let tab_rect = egui::Rect::from_min_max(rect.min, pos2(close_rect.left(), rect.bottom()));
        let id = ui.id().with(("browser-tab", webview.id()));
        let tab = ui.interact(tab_rect, id, egui::Sense::click());
        tab.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, active, &label));
        let tab = tab.on_hover_ui(|ui| {
            ui.label(&label);
            ui.small(&page.url);
            if deferred {
                ui.weak("Загрузится при выборе вкладки");
            }
        });
        let close_button = ui
            .interact(close_rect, id.with("close"), egui::Sense::click())
            .on_hover_text("Закрыть вкладку · Ctrl+W");
        close_button
            .widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Закрыть вкладку"));

        let fill_color = if active || tab.hovered() || close_button.hovered() {
            active_bg_color
        } else {
            inactive_bg_color
        };
        ui.painter().rect_filled(rect, 8, fill_color);
        if tab.has_focus() {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                8,
                egui::Stroke::new(1.0_f32, ui.visuals().hyperlink_color),
                egui::StrokeKind::Inside,
            );
        }

        let icon_rect = egui::Rect::from_center_size(
            pos2(rect.left() + 18.0, rect.center().y),
            Vec2::splat(16.0),
        );
        let youtube = Url::parse(&page.url).is_ok_and(|url| {
            matches!(url.scheme(), "https" | "http")
                && matches!(
                    url.host_str(),
                    Some("youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com")
                )
        });
        if deferred {
            super::icons::paint(
                ui,
                icon_rect,
                super::icons::Icon::History,
                ui.visuals().weak_text_color(),
            );
        } else if youtube {
            super::icons::paint(
                ui,
                icon_rect,
                super::icons::Icon::YouTube,
                egui::Color32::WHITE,
            );
        } else if let Some(favicon) = favicon_texture {
            let scale = 16.0 / favicon.size.x.max(favicon.size.y).max(1.0);
            let image_rect = egui::Rect::from_center_size(icon_rect.center(), favicon.size * scale);
            ui.painter().image(
                favicon.id,
                image_rect,
                egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            super::icons::paint(
                ui,
                icon_rect,
                super::icons::Icon::Network,
                ui.visuals().weak_text_color(),
            );
        }

        let title_rect = egui::Rect::from_min_max(
            pos2(icon_rect.right() + 8.0, rect.top()),
            pos2(close_rect.left() - 6.0, rect.bottom()),
        );
        let text_color = if active {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let mut title = egui::text::LayoutJob::simple(
            label,
            egui::TextStyle::Button.resolve(ui.style()),
            text_color,
            title_rect.width(),
        );
        title.wrap = egui::text::TextWrapping::truncate_at_width(title_rect.width());
        title.break_on_newline = false;
        let galley = ui.painter().layout_job(title);
        ui.painter()
            .with_clip_rect(title_rect.intersect(ui.clip_rect()))
            .galley(
                pos2(
                    title_rect.left(),
                    title_rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                text_color,
            );

        if close_button.hovered() || close_button.is_pointer_button_down_on() {
            ui.painter().rect_filled(
                close_rect.shrink(1.0),
                6,
                ui.visuals().widgets.hovered.weak_bg_fill,
            );
        }
        if close_button.has_focus() {
            ui.painter().rect_stroke(
                close_rect.shrink(1.0),
                6,
                egui::Stroke::new(1.0_f32, ui.visuals().hyperlink_color),
                egui::StrokeKind::Inside,
            );
        }
        super::icons::paint(
            ui,
            egui::Rect::from_center_size(close_rect.center(), Vec2::splat(16.0)),
            super::icons::Icon::Close,
            if close_button.hovered() {
                ui.visuals().text_color()
            } else {
                ui.visuals().weak_text_color()
            },
        );
        if close_button.clicked() || close_button.middle_clicked() || tab.middle_clicked() {
            window.queue_user_interface_command(UserInterfaceCommand::CloseWebView(webview.id()));
        } else if !active && tab.clicked() {
            window.activate_webview(webview.id());
        }
    }

    /// Update the user interface, but do not paint the updated state.
    pub(crate) fn update(
        &mut self,
        state: &RunningAppState,
        window: &ServoShellWindow,
        headed_window: &headed_window::HeadedWindow,
    ) {
        self.rendering_context
            .make_current()
            .expect("Could not make RenderingContext current");
        let Self {
            password_manager,
            browser_chrome,
            rendering_context,
            context,
            toolbar_height,
            location,
            location_dirty,
            favicon_textures,
            ..
        } = self;

        let winit_window = headed_window.winit_window();
        context.run(winit_window, |ctx| {
            super::browser_chrome::configure(ctx);
            if let Some((source, passwords)) = password_manager {
                let source = state.window(*source);
                passwords.show_manager(ctx, source.as_deref(), winit_window);
                *toolbar_height = Length::new(winit_window.inner_size().height as f32);
                return;
            }
            load_pending_favicons(ctx, window, favicon_textures);

            if !headed_window.is_fullscreen_from_document() {
                Panel::top("tabs")
                    .frame(
                        egui::Frame::NONE
                            .fill(ctx.style().visuals.extreme_bg_color)
                            .inner_margin(egui::Margin::symmetric(8, 4)),
                    )
                    .show_inside(ctx, |ui| {
                        ui.set_min_height(32.0);
                        let caption_width = if winit_window.is_decorated() {
                            0.0
                        } else {
                            132.0
                        };
                        ui.horizontal_centered(|ui| {
                            let (logo_rect, logo_drag) = ui.allocate_exact_size(
                                Vec2::new(28.0, 32.0),
                                egui::Sense::click_and_drag(),
                            );
                            super::icons::paint(
                                ui,
                                egui::Rect::from_center_size(logo_rect.center(), Vec2::splat(24.0)),
                                super::icons::Icon::Logo,
                                ui.visuals().hyperlink_color,
                            );
                            if logo_drag.double_clicked() {
                                winit_window.set_maximized(!winit_window.is_maximized());
                            } else if logo_drag.drag_started_by(egui::PointerButton::Primary) {
                                let _ = winit_window.drag_window();
                            }
                            let tabs_width =
                                (ui.available_width() - caption_width - 44.0).max(80.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(tabs_width, 32.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    egui::ScrollArea::horizontal()
                                        .max_width(tabs_width)
                                        .scroll_bar_visibility(
                                            egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                        )
                                        .show(ui, |ui| {
                                            ui.horizontal_centered(|ui| {
                                                for (id, webview) in window.webviews() {
                                                    let favicon = favicon_textures
                                                        .get(&id)
                                                        .map(|(_, icon)| *icon);
                                                    Self::browser_tab(ui, window, webview, favicon);
                                                }
                                                let (_, drag) = ui.allocate_exact_size(
                                                    Vec2::new(ui.available_width().max(0.0), 32.0),
                                                    egui::Sense::click_and_drag(),
                                                );
                                                if drag.double_clicked() {
                                                    winit_window.set_maximized(
                                                        !winit_window.is_maximized(),
                                                    );
                                                } else if drag
                                                    .drag_started_by(egui::PointerButton::Primary)
                                                {
                                                    let _ = winit_window.drag_window();
                                                }
                                            });
                                        });
                                },
                            );
                            if super::icons::button(
                                ui,
                                super::icons::Icon::Plus,
                                "Новая вкладка · Ctrl+T",
                                true,
                            )
                            .clicked()
                            {
                                window
                                    .queue_user_interface_command(UserInterfaceCommand::NewWebView);
                            }
                            if !winit_window.is_decorated() {
                                super::browser_chrome::window_controls(
                                    ui,
                                    winit_window,
                                    window,
                                    state,
                                );
                            }
                        });
                    });
                Panel::top("navigation")
                    .frame(
                        egui::Frame::NONE
                            .fill(ctx.style().visuals.panel_fill)
                            .inner_margin(egui::Margin::symmetric(8, 4)),
                    )
                    .show_inside(ctx, |ui| {
                        browser_chrome.navigation(
                            ui,
                            state,
                            window,
                            location,
                            location_dirty,
                            self.can_go_back,
                            self.can_go_forward,
                            !matches!(self.load_status, LoadStatus::Complete),
                        );
                    });
                *toolbar_height = Length::new(ctx.available_rect_before_wrap().min.y);
                browser_chrome.error_page(ctx, window, ctx.available_rect_before_wrap());
                browser_chrome.windows(ctx, state, window);
                super::autofill::show(ctx, window);
            } else {
                *toolbar_height = Length::default();
            }

            let scale =
                Scale::<_, DeviceIndependentPixel, DevicePixel>::new(ctx.pixels_per_point());

            headed_window.for_each_active_dialog(window, |dialog| dialog.update(ctx));

            // If the top parts of the GUI changed size, then update the size of the WebView and also
            // the size of its RenderingContext.
            let available_rect = ctx.available_rect_before_wrap();

            // Build a graft node for each WebView.
            let affine = {
                // The grafted WebView tree reports bounds in device pixels relative to the
                // WebView's own origin, so this node supplies the offset of the WebView within
                // the window, and scales it to the same scale as the rest of the nodes in egui's
                // AccessKit tree.
                let scale = (1.0 / window.platform_window().hidpi_scale_factor().get()) as f64;
                let x = available_rect.min.x as f64;
                let y = available_rect.min.y as f64;
                Affine::new([scale, 0.0, 0.0, scale, x, y])
            };
            for (webview_id, webview) in window.webviews() {
                if let Some(tree_id) = webview.accesskit_tree_id() {
                    let id = egui::Id::new(webview_id);
                    ctx.accesskit_node_builder(id, |node| {
                        node.set_tree_id(tree_id);
                        // Only the transform is set: AccessKit consumers exclude graft nodes from
                        // the presented tree, so bounds on this node would never be read.
                        node.set_transform(affine);
                    });
                }
            }
            let size = Size2D::new(available_rect.width(), available_rect.height()) * scale;
            if let Some(webview) = window.active_webview()
                && size != webview.size()
            {
                // `rect` is sized to just the WebView viewport, which is required by
                // `OffscreenRenderingContext` See:
                // <https://github.com/servo/servo/issues/38369#issuecomment-3138378527>
                webview.resize(PhysicalSize::new(size.width as u32, size.height as u32))
            }

            if let Some(status_text) = &self.status_text {
                egui::Tooltip::always_open(
                    ctx.clone(),
                    LayerId::new(Order::Tooltip, Id::new("tooltip")),
                    "tooltip layer".into(),
                    pos2(0.0, available_rect.max.y),
                )
                .show(|ui| ui.add(Label::new(status_text.clone()).extend()));
            }

            window.repaint_webviews();

            if let Some(render_to_parent) = rendering_context.render_to_parent_callback() {
                ctx.layer_painter(LayerId::background()).add(PaintCallback {
                    rect: available_rect,
                    callback: Arc::new(CallbackFn::new(move |info, painter| {
                        let clip = info.viewport_in_pixels();
                        let rect_in_parent = Rect::new(
                            Point2D::new(clip.left_px, clip.from_bottom_px),
                            Size2D::new(clip.width_px, clip.height_px),
                        );
                        render_to_parent(painter.gl(), rect_in_parent)
                    })),
                });
            }
        });

        // If any egui widget requested a repaint, also request a repaint for our
        // containing window. This allows egui widget to animate on their own.
        if super::repaint::needs_immediate_repaint(&self.context.egui_ctx) {
            window.set_needs_repaint();
        }

        let adapter = self
            .context
            .egui_winit
            .accesskit
            .as_mut()
            .expect("guaranteed by Gui::new()");
        for tree_update in self.pending_accesskit_updates.drain(..) {
            adapter.update_if_active(|| tree_update);
        }
    }

    /// Paint the GUI, as of the last update.
    pub(crate) fn paint(&mut self, window: &Window) {
        self.rendering_context
            .make_current()
            .expect("Could not make RenderingContext current");
        self.rendering_context
            .parent_context()
            .prepare_for_rendering();
        self.context.paint(window);
        // Opt-in developer capture reads our own composed framebuffer, including
        // the browser chrome. It is disabled in ordinary launches.
        static CAPTURED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        static CAPTURE_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        if matches!(self.load_status, LoadStatus::Complete)
            && CAPTURE_START.get_or_init(std::time::Instant::now).elapsed()
                >= std::time::Duration::from_secs(3)
            && !CAPTURED.load(std::sync::atomic::Ordering::Relaxed)
            && let Some(path) = std::env::var_os("RUST_BROWSER_CAPTURE_UI")
        {
            let parent = self.rendering_context.parent_context();
            let size = parent.size();
            let rect = servo::DeviceIntRect::from_size(euclid::Size2D::new(
                size.width as i32,
                size.height as i32,
            ));
            if let Some(image) = parent.read_to_image(rect) {
                if image.save(std::path::PathBuf::from(path)).is_ok() {
                    CAPTURED.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        self.rendering_context.parent_context().present();
    }
    pub(crate) fn set_password_source(&mut self, source: crate::window::ServoShellWindowId) {
        if let Some((current, _)) = &mut self.password_manager {
            *current = source;
        }
    }

    /// Updates the location field from the given [`RunningAppState`], unless the user has started
    /// editing it without clicking Go, returning true iff it has changed (needing an egui update).
    fn update_location_in_toolbar(&mut self, window: &ServoShellWindow) -> bool {
        // User edited without clicking Go?
        if self.location_dirty {
            return false;
        }

        let current_url_string = window
            .active_webview()
            .map(|webview| window.tab_page(&webview).url);
        match current_url_string {
            Some(location) if location != self.location => {
                self.location = location;
                true
            },
            _ => false,
        }
    }

    fn update_load_status(&mut self, window: &ServoShellWindow) -> bool {
        let state_status = window
            .active_webview()
            .map(|webview| webview.load_status())
            .unwrap_or(LoadStatus::Complete);
        let old_status = std::mem::replace(&mut self.load_status, state_status);
        let status_changed = old_status != self.load_status;

        // When the load status changes, we want the new changes to the URL to start
        // being reflected in the location bar.
        if status_changed {
            self.location_dirty = false;
        }

        status_changed
    }

    fn update_status_text(&mut self, window: &ServoShellWindow) -> bool {
        let state_status = window
            .active_webview()
            .and_then(|webview| webview.status_text());
        let old_status = std::mem::replace(&mut self.status_text, state_status);
        old_status != self.status_text
    }

    fn update_can_go_back_and_forward(&mut self, window: &ServoShellWindow) -> bool {
        let (can_go_back, can_go_forward) = window
            .active_webview()
            .map(|webview| (webview.can_go_back(), webview.can_go_forward()))
            .unwrap_or((false, false));
        let old_can_go_back = std::mem::replace(&mut self.can_go_back, can_go_back);
        let old_can_go_forward = std::mem::replace(&mut self.can_go_forward, can_go_forward);
        old_can_go_back != self.can_go_back || old_can_go_forward != self.can_go_forward
    }

    /// Updates all fields taken from the given [`ServoShellWindow`], such as the location field.
    /// Returns true iff the egui needs an update.
    pub(crate) fn update_webview_data(&mut self, window: &ServoShellWindow) -> bool {
        // Note: We must use the "bitwise OR" (|) operator here instead of "logical OR" (||)
        //       because logical OR would short-circuit if any of the functions return true.
        //       We want to ensure that all functions are called. The "bitwise OR" operator
        //       does not short-circuit.
        self.update_load_status(window)
            | self.update_location_in_toolbar(window)
            | self.update_status_text(window)
            | self.update_can_go_back_and_forward(window)
    }

    /// Returns true if a redraw is required after handling the provided event.
    pub(crate) fn handle_accesskit_event(
        &mut self,
        event: &egui_winit::accesskit_winit::WindowEvent,
    ) -> bool {
        match event {
            egui_winit::accesskit_winit::WindowEvent::InitialTreeRequested => {
                self.context.egui_ctx.enable_accesskit();
                true
            },
            egui_winit::accesskit_winit::WindowEvent::ActionRequested(req) => {
                self.context
                    .egui_winit
                    .on_accesskit_action_request(req.clone());
                true
            },
            egui_winit::accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                self.context.egui_ctx.disable_accesskit();
                false
            },
        }
    }

    pub(crate) fn set_zoom_factor(&self, factor: f32) {
        self.context.egui_ctx.set_zoom_factor(factor);
    }

    pub(crate) fn notify_accessibility_tree_update(&mut self, tree_update: accesskit::TreeUpdate) {
        self.pending_accesskit_updates.push(tree_update);
    }
}

fn embedder_image_to_egui_image(image: &Image) -> egui::ColorImage {
    let width = image.width as usize;
    let height = image.height as usize;

    match image.format {
        PixelFormat::K8 => egui::ColorImage::from_gray([width, height], image.data()),
        PixelFormat::KA8 => {
            // Convert to rgba
            let (chunks, _) = image.data().as_chunks::<2>();
            let data: Vec<u8> = chunks
                .iter()
                .flat_map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
                .collect();
            egui::ColorImage::from_rgba_unmultiplied([width, height], &data)
        },
        PixelFormat::RGB8 => egui::ColorImage::from_rgb([width, height], image.data()),
        PixelFormat::RGBA8 => {
            egui::ColorImage::from_rgba_unmultiplied([width, height], image.data())
        },
        PixelFormat::BGRA8 => {
            // Convert from BGRA to RGBA
            let (chunks, _) = image.data().as_chunks::<4>();
            let data: Vec<u8> = chunks
                .iter()
                .flat_map(|chunk| [chunk[2], chunk[1], chunk[0], chunk[3]])
                .collect();
            egui::ColorImage::from_rgba_unmultiplied([width, height], &data)
        },
    }
}

/// Uploads all favicons that have not yet been processed to the GPU.
fn load_pending_favicons(
    ctx: &egui::Context,
    window: &ServoShellWindow,
    texture_cache: &mut HashMap<WebViewId, (egui::TextureHandle, egui::load::SizedTexture)>,
) {
    // Dropping closed tabs' handles releases their GPU textures too.
    texture_cache.retain(|id, _| window.webview_by_id(*id).is_some());
    for id in window.take_pending_favicon_loads() {
        let Some(webview) = window.webview_by_id(id) else {
            continue;
        };
        let Some(favicon) = webview.favicon() else {
            continue;
        };

        let egui_image = embedder_image_to_egui_image(&favicon);
        let handle = ctx.load_texture(format!("favicon-{id:?}"), egui_image, Default::default());
        let texture = egui::load::SizedTexture::new(
            handle.id(),
            egui::vec2(favicon.width as f32, favicon.height as f32),
        );

        // We don't need the handle anymore but we can't drop it either since that would cause
        // the texture to be freed.
        texture_cache.insert(id, (handle, texture));
    }
}
