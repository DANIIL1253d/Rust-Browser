/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::AtomicU64;

use euclid::Scale;
use log::warn;
use servo::{
    AuthenticationRequest, BluetoothDeviceSelectionRequest, ConsoleLogLevel,
    CreateNewWebViewRequest, Cursor, DeviceIndependentIntRect, DeviceIndependentPixel,
    DeviceIntPoint, DeviceIntSize, DevicePixel, EmbedderControl, EmbedderControlId, InputEventId,
    InputEventResult, MediaSessionEvent, PermissionRequest, RenderingContext, ScreenGeometry,
    WebView, WebViewBuilder, WebViewId,
};
use url::Url;

use crate::parser::location_bar_input_to_url;
use crate::running_app_state::{RunningAppState, UserInterfaceCommand, WebViewCollection};

// This should vary by zoom level and maybe actual text size (focused or under cursor)
#[cfg_attr(any(target_os = "android", target_env = "ohos"), expect(dead_code))]
pub(crate) const LINE_HEIGHT: f32 = 76.0;
#[cfg_attr(any(target_os = "android", target_env = "ohos"), expect(dead_code))]
pub(crate) const LINE_WIDTH: f32 = 76.0;

/// <https://github.com/web-platform-tests/wpt/blob/9320b1f724632c52929a3fdb11bdaf65eafc7611/webdriver/tests/classic/set_window_rect/set.py#L287-L290>
/// "A window size of 10x10px shouldn't be supported by any browser."
#[cfg_attr(any(target_os = "android", target_env = "ohos"), expect(dead_code))]
pub(crate) const MIN_WINDOW_INNER_SIZE: DeviceIntSize = DeviceIntSize::new(100, 100);

static SERVOSHELL_WINDOW_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub(crate) struct ServoShellWindowId(u64);

impl From<u64> for ServoShellWindowId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl ServoShellWindowId {
    #[cfg_attr(not(any(target_os = "android", target_env = "ohos")), expect(unused))]
    pub(crate) fn next() -> ServoShellWindowId {
        ServoShellWindowId(SERVOSHELL_WINDOW_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

pub(crate) struct ServoShellWindow {
    /// The [`WebView`]s that have been added to this window.
    pub(crate) webview_collection: RefCell<WebViewCollection>,
    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    deferred_tabs: RefCell<rust_browser_core::session::DeferredTabs<WebViewId>>,
    /// A handle to the [`PlatformWindow`] that servoshell is rendering in.
    platform_window: Rc<dyn PlatformWindow>,
    /// Whether or not this window should be closed at the end of the spin of the next event loop.
    close_scheduled: Cell<bool>,
    /// Whether or not the application interface needs to be updated.
    needs_update: Cell<bool>,
    /// Whether or not Servo needs to repaint its display. Currently this is global
    /// because every `WebView` shares a `RenderingContext`.
    needs_repaint: Cell<bool>,
    /// List of webviews that have favicon textures which are not yet uploaded
    /// to the GPU by egui.
    pending_favicon_loads: RefCell<Vec<WebViewId>>,
    /// Pending [`UserInterfaceCommand`] that have yet to be processed by the main loop.
    pending_commands: RefCell<Vec<UserInterfaceCommand>>,
}

pub(crate) enum TopLevelWebViewCreationRequest {
    WithUrl(Url),
    WithCreateRequest(CreateNewWebViewRequest),
}

impl ServoShellWindow {
    pub(crate) fn new(platform_window: Rc<dyn PlatformWindow>) -> Self {
        Self {
            webview_collection: Default::default(),
            #[cfg(not(any(target_os = "android", target_env = "ohos")))]
            deferred_tabs: Default::default(),
            platform_window,
            close_scheduled: Default::default(),
            needs_update: Default::default(),
            needs_repaint: Default::default(),
            pending_favicon_loads: Default::default(),
            pending_commands: Default::default(),
        }
    }

    pub(crate) fn id(&self) -> ServoShellWindowId {
        self.platform_window().id()
    }

    /// Must be called *after* `self` is in `state.windows`, otherwise it will panic.
    pub(crate) fn create_and_activate_toplevel_webview(
        self: &Rc<Self>,
        state: Rc<RunningAppState>,
        creation_type: TopLevelWebViewCreationRequest,
    ) -> WebView {
        let webview = self.create_toplevel_webview(state, creation_type);
        self.activate_webview(webview.id());
        webview
    }

    /// Must be called *after* `self` is in `state.windows`, otherwise it will panic.
    #[servo::servo_tracing::instrument(skip(self, state, creation_type))]
    pub(crate) fn create_toplevel_webview(
        &self,
        state: Rc<RunningAppState>,
        creation_type: TopLevelWebViewCreationRequest,
    ) -> WebView {
        #[cfg(not(any(target_os = "android", target_env = "ohos")))]
        let intended = match &creation_type { TopLevelWebViewCreationRequest::WithUrl(url) => Some(url.to_string()), _ => None };
        let mut webview_builder = match creation_type {
            TopLevelWebViewCreationRequest::WithUrl(url) => {
                WebViewBuilder::new(state.servo(), self.platform_window.rendering_context())
                    .url(url)
            },
            TopLevelWebViewCreationRequest::WithCreateRequest(request) => {
                request.builder(self.platform_window.rendering_context())
            },
        };
        webview_builder = webview_builder
            .hidpi_scale_factor(self.platform_window.hidpi_scale_factor())
            .user_content_manager(state.user_content_manager.clone())
            .delegate(state.clone());

        #[cfg(all(
            feature = "gamepad",
            not(any(target_os = "android", target_env = "ohos"))
        ))]
        if let Some(gamepad_delegate) = state.gamepad_delegate() {
            webview_builder = webview_builder.gamepad_delegate(gamepad_delegate);
        }

        let webview = webview_builder.build();
        self.add_webview(webview.clone());
        #[cfg(not(any(target_os = "android", target_env = "ohos")))]
        if let Some(url) = intended {
            self.track_loading_webview(webview.id(), rust_browser_core::Page { title: url.clone(), url });
        }

        // If `self` is not in `state.windows`, our notify_accessibility_tree_update() will panic.
        if state.accessibility_active() {
            // Activate accessibility in the WebView.
            // There are two sites like this; this is the WebView creation site.
            webview.set_accessibility_active(true);
        }
        webview
    }

    /// Repaint the active [`WebView`].
    pub(crate) fn repaint_webviews(&self) {
        let Some(webview) = self.active_webview() else {
            return;
        };

        self.platform_window()
            .rendering_context()
            .make_current()
            .expect("Could not make PlatformWindow RenderingContext current");
        webview.paint();
        self.platform_window().rendering_context().present();
    }

    /// Whether or not this [`ServoShellWindow`] has any [`WebView`]s.
    pub(crate) fn should_close(&self) -> bool {
        self.webview_collection.borrow().is_empty() || self.close_scheduled.get()
    }

    pub(crate) fn webview_by_id(&self, id: WebViewId) -> Option<WebView> {
        self.webview_collection.borrow().get(id).cloned()
    }

    pub(crate) fn set_needs_update(&self) {
        self.needs_update.set(true);
    }

    pub(crate) fn set_needs_repaint(&self) {
        self.needs_repaint.set(true)
    }

    #[cfg_attr(target_os = "android", expect(dead_code))]
    pub(crate) fn schedule_close(&self) {
        self.close_scheduled.set(true)
    }

    pub(crate) fn platform_window(&self) -> Rc<dyn PlatformWindow> {
        self.platform_window.clone()
    }

    pub(crate) fn add_webview(&self, webview: WebView) {
        webview.notify_theme_change(self.platform_window.theme());
        self.webview_collection.borrow_mut().add(webview);
        self.set_needs_update();
        self.set_needs_repaint();
    }

    pub(crate) fn webview_ids(&self) -> Vec<WebViewId> {
        self.webview_collection.borrow().creation_order.clone()
    }

    /// Returns all [`WebView`]s in creation order.
    pub(crate) fn webviews(&self) -> Vec<(WebViewId, WebView)> {
        self.webview_collection
            .borrow()
            .all_in_creation_order()
            .map(|(id, webview)| (id, webview.clone()))
            .collect()
    }

    pub(crate) fn activate_webview(&self, webview_id: WebViewId) {
        #[cfg(not(any(target_os = "android", target_env = "ohos")))]
        log::debug!("Selecting tab: deferred={}, target={}", self.is_deferred(webview_id), self.has_restoring_target(webview_id));
        #[cfg(not(any(target_os = "android", target_env = "ohos")))]
        {
            let target = self.deferred_tabs.borrow_mut().begin_activation(webview_id);
            if let Some(page) = target {
                if let (Some(view), Ok(url)) = (self.webview_by_id(webview_id), Url::parse(&page.url)) {
                    view.load(url);
                    log::debug!("Deferred tab activation started");
                }
            } else if self.has_restoring_target(webview_id) {
                log::debug!("Tab activation waiting or already loading; committed URL present={}", self.webview_by_id(webview_id).is_some_and(|v| v.url().is_some()));
            }
        }
        self.webview_collection
            .borrow_mut()
            .activate_webview(webview_id);
        self.set_needs_update();
    }

    #[cfg_attr(any(target_os = "android", target_env = "ohos"), expect(dead_code))]
    pub(crate) fn activate_webview_by_index(&self, index_to_activate: usize) {
        let id = self.webview_collection.borrow().creation_order.get(index_to_activate).copied();
        if let Some(id) = id { self.activate_webview(id); }
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    pub(crate) fn defer_webview(&self, id: WebViewId, page: rust_browser_core::Page) {
        self.deferred_tabs.borrow_mut().insert(id, page);
        if let Some(view) = self.webview_by_id(id) { self.commit_deferred_navigation(&view); }
        self.set_needs_update();
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    pub(crate) fn track_loading_webview(&self, id: WebViewId, page: rust_browser_core::Page) {
        self.deferred_tabs.borrow_mut().track_loading(id, page);
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    pub(crate) fn navigate_webview(&self, view: &WebView, url: Url) {
        let page = rust_browser_core::Page { title: url.to_string(), url: url.to_string() };
        let waiting = self.deferred_tabs.borrow_mut().retarget_waiting(view.id(), page.clone());
        if !waiting {
            self.track_loading_webview(view.id(), page);
            view.load(url);
        }
        self.set_needs_update();
        self.set_needs_repaint();
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    pub(crate) fn is_deferred(&self, id: WebViewId) -> bool {
        self.deferred_tabs.borrow().is_deferred(id)
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    pub(crate) fn has_restoring_target(&self, id: WebViewId) -> bool {
        self.deferred_tabs.borrow().get(id).is_some()
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    pub(crate) fn tab_page(&self, view: &WebView) -> rust_browser_core::Page {
        self.deferred_tabs.borrow().get(view.id()).cloned().unwrap_or_else(|| rust_browser_core::Page {
            title: view.page_title().filter(|s| !s.is_empty()).unwrap_or_else(|| view.url().map(|u| u.to_string()).unwrap_or_else(|| "Новая вкладка".into())),
            url: view.url().map(|u| u.to_string()).unwrap_or_default(),
        })
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    pub(crate) fn commit_deferred_navigation(&self, view: &WebView) {
        log::debug!("Tab history notification: intended target={}, URL present={}, blank={}", self.has_restoring_target(view.id()), view.url().is_some(), view.url().is_some_and(|u| u.as_str() == "about:blank"));
        if let Some(url) = view.url() {
            let (changed, activate) = {
                let mut tabs = self.deferred_tabs.borrow_mut();
                let had_target = tabs.get(view.id()).is_some();
                let activate = tabs.did_navigate(view.id(), url.as_str());
                (had_target && tabs.get(view.id()).is_none(), activate)
            };
            if let Some(page) = activate { view.load(Url::parse(&page.url).expect("Validated restored URL")); }
            if changed { self.set_needs_repaint(); }
        }
    }

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    pub(crate) fn move_webview_to_index(&self, id: WebViewId, index: usize) {
        let mut views = self.webview_collection.borrow_mut();
        if !views.creation_order.contains(&id) { return; }
        views.creation_order.retain(|candidate| *candidate != id);
        let index = index.min(views.creation_order.len());
        views.creation_order.insert(index, id);
    }

    #[cfg_attr(any(target_os = "android", target_env = "ohos"), expect(dead_code))]
    pub(crate) fn get_active_webview_index(&self) -> Option<usize> {
        let active_id = self.webview_collection.borrow().active_id()?;
        self.webviews()
            .iter()
            .position(|webview| webview.0 == active_id)
    }

    pub(crate) fn update_and_request_repaint_if_necessary(&self, state: &RunningAppState) {
        let updated_user_interface = self.needs_update.take()
            && self
                .platform_window
                .update_user_interface_state(state, self);

        // Delegate handlers may have asked us to present or update painted WebView contents.
        // Currently, egui-file-dialog dialogs need to be constantly redrawn or animations aren't fluid.
        let needs_repaint = self.needs_repaint.take();
        if updated_user_interface || needs_repaint {
            self.platform_window.request_repaint(self);
        }
    }

    /// Close the given [`WebView`] via its [`WebViewId`].
    ///
    /// Note: This can happen because we can trigger a close with a UI action and then get
    /// the close notification via the [`WebViewDelegate`] later.
    pub(crate) fn close_webview(&self, webview_id: WebViewId) {
        let mut webview_collection = self.webview_collection.borrow_mut();
        let Some(closed) = webview_collection.remove(webview_id) else {
            return;
        };
        drop(webview_collection);
        #[cfg(not(any(target_os = "android", target_env = "ohos")))]
        let deferred_page = self.deferred_tabs.borrow_mut().take(webview_id);
        #[cfg(not(any(target_os = "android", target_env = "ohos")))]
        let closed_page = deferred_page.unwrap_or_else(|| self.tab_page(&closed));
        #[cfg(not(any(target_os = "android", target_env = "ohos")))]
        if self
            .platform_window
            .as_headed_window()
            .is_some_and(|w| !w.is_password_manager())
        {
            crate::desktop::browser::with(|b| {
                b.profile
                    .remember_closed(&closed_page.url, &closed_page.title);
                b.save();
            });
        }
        self.platform_window
            .dismiss_embedder_controls_for_webview(webview_id);

        let next = self.webview_collection.borrow().active_id();
        if let Some(id) = next { self.activate_webview(id); }

        self.set_needs_update();
        self.set_needs_repaint();
    }

    pub(crate) fn notify_favicon_changed(&self, webview: WebView) {
        self.pending_favicon_loads.borrow_mut().push(webview.id());
        self.set_needs_repaint();
    }

    #[cfg_attr(any(target_os = "android", target_env = "ohos"), expect(dead_code))]
    pub(crate) fn hidpi_scale_factor_changed(&self) {
        let new_scale_factor = self.platform_window.hidpi_scale_factor();
        for webview in self.webview_collection.borrow().values() {
            webview.set_hidpi_scale_factor(new_scale_factor);
        }
    }

    pub(crate) fn active_webview(&self) -> Option<WebView> {
        self.webview_collection.borrow().active().cloned()
    }

    #[cfg_attr(
        not(any(target_os = "android", target_env = "ohos")),
        expect(dead_code)
    )]
    pub(crate) fn active_or_newest_webview(&self) -> Option<WebView> {
        let webview_collection = self.webview_collection.borrow();
        webview_collection
            .active()
            .or(webview_collection.newest())
            .cloned()
    }

    /// Return a list of all webviews that have favicons that have not yet been loaded by egui.
    #[cfg_attr(any(target_os = "android", target_env = "ohos"), expect(dead_code))]
    pub(crate) fn take_pending_favicon_loads(&self) -> Vec<WebViewId> {
        std::mem::take(&mut *self.pending_favicon_loads.borrow_mut())
    }

    pub(crate) fn show_embedder_control(
        &self,
        webview: WebView,
        embedder_control: EmbedderControl,
    ) {
        self.platform_window
            .show_embedder_control(webview.id(), embedder_control);
        self.set_needs_update();
        self.set_needs_repaint();
    }

    pub(crate) fn hide_embedder_control(
        &self,
        webview: WebView,
        embedder_control: EmbedderControlId,
    ) {
        self.platform_window
            .hide_embedder_control(webview.id(), embedder_control);
        self.set_needs_update();
        self.set_needs_repaint();
    }

    pub(crate) fn queue_user_interface_command(&self, command: UserInterfaceCommand) {
        self.pending_commands.borrow_mut().push(command)
    }

    /// Takes any events generated during UI updates and performs their actions.
    pub(crate) fn handle_interface_commands(
        self: &Rc<Self>,
        state: &Rc<RunningAppState>,
        create_platform_window: Option<&dyn Fn(Url) -> Rc<dyn PlatformWindow>>,
    ) {
        let commands = std::mem::take(&mut *self.pending_commands.borrow_mut());
        for event in commands {
            match event {
                UserInterfaceCommand::Go(location) => {
                    self.set_needs_update();
                    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
                    let searchpage = crate::desktop::browser::with(|b| {
                        b.profile.appearance.search_engine.template().to_owned()
                    });
                    #[cfg(any(target_os = "android", target_env = "ohos"))]
                    let searchpage = state.servoshell_preferences.searchpage.clone();
                    let Some(url) = location_bar_input_to_url(&location.clone(), &searchpage)
                    else {
                        warn!("failed to parse location");
                        break;
                    };
                    if let Some(active_webview) = self.active_webview() {
                        #[cfg(not(any(target_os = "android", target_env = "ohos")))]
                        self.navigate_webview(&active_webview, url.into_url());
                        #[cfg(any(target_os = "android", target_env = "ohos"))]
                        active_webview.load(url.into_url());
                    }
                },
                UserInterfaceCommand::Back => {
                    if let Some(active_webview) = self.active_webview() {
                        active_webview.go_back(1);
                    }
                },
                UserInterfaceCommand::Forward => {
                    if let Some(active_webview) = self.active_webview() {
                        active_webview.go_forward(1);
                    }
                },
                UserInterfaceCommand::Reload => {
                    self.set_needs_update();
                    if let Some(active_webview) = self.active_webview() {
                        active_webview.reload();
                    }
                },
                UserInterfaceCommand::ReloadAll => {
                    for window in state.windows().iter() {
                        window.set_needs_update();
                        for (_, webview) in window.webviews() {
                            #[cfg(not(any(target_os = "android", target_env = "ohos")))]
                            if window.is_deferred(webview.id()) { continue; }
                            webview.reload();
                        }
                    }
                },
                UserInterfaceCommand::NewWebView => {
                    self.set_needs_update();
                    let url = Url::parse("servo:newtab").expect("Should always be able to parse");
                    self.create_and_activate_toplevel_webview(
                        state.clone(),
                        TopLevelWebViewCreationRequest::WithUrl(url),
                    );
                },
                UserInterfaceCommand::ReopenClosedWebView => {
                    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
                    if let Some(page) = crate::desktop::browser::with(|b| {
                        b.profile.recently_closed.first().cloned()
                    }) {
                        if let Ok(url) = Url::parse(&page.url) {
                            self.create_and_activate_toplevel_webview(
                                state.clone(),
                                TopLevelWebViewCreationRequest::WithUrl(url),
                            );
                            crate::desktop::browser::with(|b| {
                                b.profile.take_closed();
                                b.save();
                            });
                        }
                    }
                },
                UserInterfaceCommand::CloseWebView(id) => {
                    self.set_needs_update();
                    self.close_webview(id);
                },
                UserInterfaceCommand::NewWindow(creation_request) => {
                    let Some(create_platform_window) = create_platform_window else {
                        continue;
                    };
                    let url = match &creation_request {
                        TopLevelWebViewCreationRequest::WithUrl(url) => url.clone(),
                        TopLevelWebViewCreationRequest::WithCreateRequest(..) => {
                            Url::parse("servo:newtab").unwrap()
                        },
                    };
                    let platform_window = create_platform_window(url);
                    state.open_window(platform_window, creation_request);
                },
            }
        }
    }
}

/// A `PlatformWindow` abstracts away the differents kinds of platform windows that might
/// be used in a servoshell execution. This currently includes headed (winit) and headless
/// windows.
pub(crate) trait PlatformWindow {
    /// Whether or not the platform manages focus. This is used to decide whether
    /// servoshell should focus another window when one closes.
    fn platform_manages_focus(&self) -> bool {
        false
    }

    fn id(&self) -> ServoShellWindowId;
    fn screen_geometry(&self) -> ScreenGeometry;
    #[cfg_attr(any(target_os = "android", target_env = "ohos"), expect(dead_code))]
    fn device_hidpi_scale_factor(&self) -> Scale<f32, DeviceIndependentPixel, DevicePixel>;
    fn hidpi_scale_factor(&self) -> Scale<f32, DeviceIndependentPixel, DevicePixel>;
    #[cfg_attr(any(target_os = "android", target_env = "ohos"), expect(dead_code))]
    fn get_fullscreen(&self) -> bool;
    /// Inform the `Window` that the state of a `WebView` has changed and that it should
    /// do an incremental update of user interface state. Returns `true` if the user
    /// interface actually changed and a rebuild  and repaint is needed, `false` otherwise.
    fn update_user_interface_state(&self, _: &RunningAppState, _: &ServoShellWindow) -> bool {
        false
    }
    /// Request that the window redraw itself. It is up to the window to do this
    /// once the windowing system is ready. If this is a headless window, the redraw
    /// will happen immediately.
    fn request_repaint(&self, _: &ServoShellWindow);
    /// Request a new outer size for the window, including external decorations.
    /// This should be the same as `window.outerWidth` and `window.outerHeight``
    fn request_resize(&self, webview: &WebView, outer_size: DeviceIntSize)
    -> Option<DeviceIntSize>;
    fn set_position(&self, _point: DeviceIntPoint) {}
    fn set_fullscreen(&self, _state: bool) {}
    fn set_cursor(&self, _cursor: Cursor) {}
    #[cfg(all(
        feature = "webxr",
        not(any(target_os = "android", target_env = "ohos"))
    ))]
    fn new_glwindow(
        &self,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> Rc<dyn servo::webxr::GlWindow>;
    /// This returns [`RenderingContext`] matching the viewport.
    fn rendering_context(&self) -> Rc<dyn RenderingContext>;
    fn theme(&self) -> servo::Theme {
        servo::Theme::Light
    }
    fn window_rect(&self) -> DeviceIndependentIntRect;
    fn maximize(&self, _: &WebView) {}
    fn has_platform_focus(&self) -> bool {
        true
    }

    fn show_embedder_control(&self, _: WebViewId, _: EmbedderControl) {}
    fn hide_embedder_control(&self, _: WebViewId, _: EmbedderControlId) {}
    fn dismiss_embedder_controls_for_webview(&self, _: WebViewId) {}
    fn show_bluetooth_device_dialog(
        &self,
        _: WebViewId,
        _request: BluetoothDeviceSelectionRequest,
    ) {
    }
    fn show_permission_dialog(&self, _: WebViewId, _: PermissionRequest) {}
    fn show_http_authentication_dialog(&self, _: WebViewId, _: AuthenticationRequest) {}

    fn notify_input_event_handled(
        &self,
        _webview: &WebView,
        _id: InputEventId,
        _result: InputEventResult,
    ) {
    }

    fn notify_media_session_event(&self, _: MediaSessionEvent) {}
    fn notify_crashed(&self, _: WebView, _reason: String, _backtrace: Option<String>) {}
    fn show_console_message(&self, _level: ConsoleLogLevel, _message: &str) {}

    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    /// If this window is a headed window, access the concrete type.
    fn as_headed_window(&self) -> Option<&crate::desktop::headed_window::HeadedWindow> {
        None
    }

    #[cfg(any(target_os = "android", target_env = "ohos"))]
    /// If this window is a headed window, access the concrete type.
    fn as_headed_window(&self) -> Option<&crate::egl::app::EmbeddedPlatformWindow> {
        None
    }

    fn notify_accessibility_tree_update(&self, _: WebView, _: accesskit::TreeUpdate) {}
}
