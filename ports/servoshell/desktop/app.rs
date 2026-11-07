/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Application entry point, runs the event loop.

use std::path::Path;
use std::rc::Rc;
use std::time::Instant;
use std::{env, fs};

use servo::protocol_handler::ProtocolRegistry;
use servo::{
    EventLoopWaker, Opts, Preferences, ServoBuilder, ServoUrl, UserContentManager, UserScript,
};
use url::Url;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::WindowId;

use super::event_loop::AppEvent;
use crate::desktop::event_loop::ServoShellEventLoop;
use crate::desktop::headed_window::HeadedWindow;
use crate::desktop::headless_window::HeadlessWindow;
use crate::desktop::protocols;
use crate::desktop::tracing::trace_winit_event;
use crate::parser::get_default_url;
use crate::prefs::ServoShellPreferences;
use crate::running_app_state::RunningAppState;
#[cfg(feature = "gamepad")]
use crate::running_app_state::ServoshellGamepadDelegate;
use crate::window::{PlatformWindow, ServoShellWindowId, TopLevelWebViewCreationRequest};

pub(crate) enum AppState {
    Initializing,
    Running(Rc<RunningAppState>),
    ShuttingDown,
}

pub struct App {
    opts: Opts,
    preferences: Preferences,
    servoshell_preferences: ServoShellPreferences,
    waker: Box<dyn EventLoopWaker>,
    event_loop_proxy: Option<EventLoopProxy<AppEvent>>,
    initial_url: ServoUrl,
    session_plan: Option<rust_browser_core::session::SessionPlan>,
    t_start: Instant,
    t: Instant,
    state: AppState,
    next_browser_tick: Instant,
    ui_repaints: std::collections::HashMap<ServoShellWindowId, Instant>,
}

impl App {
    pub fn new(
        opts: Opts,
        mut preferences: Preferences,
        servo_shell_preferences: ServoShellPreferences,
        event_loop: &ServoShellEventLoop,
    ) -> Self {
        if !servo_shell_preferences.headless {
            super::browser::initialize(&mut preferences, event_loop.event_loop_proxy());
        }
        let mut initial_url = get_default_url(
            servo_shell_preferences.url.as_deref(),
            env::current_dir().unwrap(),
            |path| fs::metadata(path).is_ok(),
            &servo_shell_preferences,
        );

        let mut session_plan = servo_shell_preferences.session_fixture.as_ref().map(|fixture| fixture.plan.clone());
        if !servo_shell_preferences.headless
            && servo_shell_preferences.url.is_none()
            && super::browser::with(|b| b.profile.appearance.restore_session)
        {
            session_plan = super::browser::with(|b| rust_browser_core::session::SessionPlan::from_profile(&b.profile));
        }
        if let Some(plan) = &session_plan {
            initial_url = ServoUrl::parse(&plan.pages[plan.active].url).expect("Session plan contains parsed URLs");
        }
        let t = Instant::now();
        App {
            opts,
            preferences,
            servoshell_preferences: servo_shell_preferences,
            waker: event_loop.create_event_loop_waker(),
            event_loop_proxy: event_loop.event_loop_proxy(),
            initial_url,
            session_plan,
            t_start: t,
            t,
            state: AppState::Initializing,
            next_browser_tick: t,
            ui_repaints: Default::default(),
        }
    }

    /// Initialize Application once event loop start running.
    pub fn init(&mut self, active_event_loop: Option<&ActiveEventLoop>) {
        let mut protocol_registry = ProtocolRegistry::default();
        let _ = protocol_registry.register(
            "urlinfo",
            protocols::urlinfo::UrlInfoProtocolHander::default(),
        );
        let _ =
            protocol_registry.register("servo", protocols::servo::ServoProtocolHandler::default());
        let _ = protocol_registry.register(
            "resource",
            protocols::resource::ResourceProtocolHandler::default(),
        );

        let servo_builder = ServoBuilder::default()
            .opts(self.opts.clone())
            .preferences(self.preferences.clone())
            .protocol_registry(protocol_registry)
            .event_loop_waker(self.waker.clone());

        let url = self.initial_url.as_url().clone();

        let servo = servo_builder.build();
        let platform_window = self.create_platform_window(url, active_event_loop);

        #[cfg(feature = "webxr")]
        servo.register_webxr_registry(super::webxr::XrDiscoveryWebXrRegistry::new_boxed(
            platform_window.clone(),
            active_event_loop,
            &self.preferences,
        ));

        servo.setup_logging();

        let user_content_manager = Rc::new(UserContentManager::new(&servo));
        for script in load_userscripts(self.servoshell_preferences.userscripts_directory.as_deref())
            .expect("Loading userscripts failed")
        {
            user_content_manager.add_script(Rc::new(script));
        }

        for user_stylesheet in &self.servoshell_preferences.user_stylesheets {
            user_content_manager.add_stylesheet(user_stylesheet.clone());
        }

        let running_state = Rc::new(RunningAppState::new(
            servo,
            self.servoshell_preferences.clone(),
            self.waker.clone(),
            user_content_manager,
            self.preferences.clone(),
            #[cfg(feature = "gamepad")]
            self.event_loop_proxy
                .clone()
                .map(ServoshellGamepadDelegate::new)
                .map(Rc::new),
        ));
        let initial_url = self.initial_url.as_url().clone();
        let browser_window = running_state.open_window(
            platform_window,
            TopLevelWebViewCreationRequest::WithUrl(initial_url),
        );
        if let Some(plan) = &self.session_plan {
            let active_id = browser_window.active_webview().expect("Initial view exists").id();
            browser_window.track_loading_webview(active_id, plan.pages[plan.active].clone());
            let eager = self.servoshell_preferences.session_fixture.as_ref().is_some_and(|fixture| fixture.eager);
            for (index, page) in plan.pages.iter().enumerate().filter(|(index, _)| *index != plan.active) {
                let url = Url::parse(if eager { &page.url } else { "about:blank" }).expect("Session plan URL");
                let view = browser_window.create_toplevel_webview(running_state.clone(), TopLevelWebViewCreationRequest::WithUrl(url));
                if !eager { browser_window.defer_webview(view.id(), page.clone()); }
                log::trace!("Restored background session slot {index}");
            }
            browser_window.move_webview_to_index(active_id, plan.active);
            browser_window.activate_webview(active_id);
            log::info!("Session restored: {} tabs, {} deferred, active index {}", plan.pages.len(), if eager { 0 } else { plan.pages.len().saturating_sub(1) }, plan.active);
            if let Some(fixture) = &self.servoshell_preferences.session_fixture {
                let mut snapshot = rust_browser_core::Profile::default();
                snapshot.remember_session(browser_window.webviews().into_iter().map(|(id, view)| (id == active_id, browser_window.tab_page(&view))));
                let valid = snapshot.session.iter().eq(plan.pages.iter().map(|p| &p.url)) && snapshot.active_tab == plan.active;
                log::info!("Session fixture early snapshot valid: {valid}");
                log::info!("Session fixture storage: persistent config={}, temporary={}", self.opts.config_dir.is_some(), self.opts.temporary_storage);
                if let Some(index) = fixture.activate_immediately { browser_window.activate_webview_by_index(index); }
                if let Some(url) = &fixture.navigate_immediately {
                    browser_window.navigate_webview(&browser_window.active_webview().expect("Fixture active view"), url.clone());
                }
            }
        }

        self.state = AppState::Running(running_state);
    }

    #[servo::servo_tracing::instrument(level = "debug", skip_all)]
    fn create_platform_window(
        &self,
        url: Url,
        active_event_loop: Option<&ActiveEventLoop>,
    ) -> Rc<dyn PlatformWindow> {
        assert_eq!(
            self.servoshell_preferences.headless,
            active_event_loop.is_none()
        );

        let Some(active_event_loop) = active_event_loop else {
            return HeadlessWindow::new(&self.servoshell_preferences);
        };

        HeadedWindow::new(
            &self.servoshell_preferences,
            active_event_loop,
            self.event_loop_proxy
                .clone()
                .expect("Should always have event loop proxy in headed mode."),
            url,
            None,
        )
    }

    pub fn pump_servo_event_loop(&mut self, active_event_loop: Option<&ActiveEventLoop>) -> bool {
        let AppState::Running(state) = &self.state else {
            return false;
        };

        let create_platform_window = |url: Url| self.create_platform_window(url, active_event_loop);
        if !self.servoshell_preferences.headless {
            super::browser::with(|b| b.snapshot(state));
        }
        if !state.spin_event_loop(Some(&create_platform_window)) {
            if !self.servoshell_preferences.headless {
                super::browser::with(|b| b.capture(state));
            }
            self.state = AppState::ShuttingDown;
            return false;
        }
        if !self.servoshell_preferences.headless {
            super::browser::with(|b| b.snapshot(state));
        }
        true
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if let AppState::Running(state) = &self.state {
            self.ui_repaints.retain(|id, deadline| {
                if *deadline <= now {
                    if let Some(window) = state.window(*id) {
                        window.set_needs_update();
                        window.set_needs_repaint();
                    }
                    false
                } else {
                    state.window(*id).is_some()
                }
            });
        }
        if !self.servoshell_preferences.headless && Instant::now() >= self.next_browser_tick {
            if let AppState::Running(state) = &self.state {
                if super::browser::with(|b| b.poll(state)) {
                    for window in state.windows().iter() {
                        window.set_needs_update();
                        window.set_needs_repaint();
                    }
                }
            }
            self.next_browser_tick = Instant::now()
                + super::browser::with(|b| {
                    if b.busy() {
                        std::time::Duration::from_millis(150)
                    } else {
                        std::time::Duration::from_secs(3)
                    }
                });
        }
        if !self.pump_servo_event_loop(Some(event_loop)) {
            event_loop.exit();
            return;
        }
        let browser_deadline = if self.servoshell_preferences.headless {
            Instant::now() + std::time::Duration::from_secs(60)
        } else {
            self.next_browser_tick
        };
        let deadline = self
            .ui_repaints
            .values()
            .copied()
            .min()
            .unwrap_or(browser_deadline)
            .min(browser_deadline);
        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.init(Some(event_loop));
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        window_event: WindowEvent,
    ) {
        let now = Instant::now();
        trace_winit_event!(
            window_event,
            "@{:?} (+{:?}) {window_event:?}",
            now - self.t_start,
            now - self.t
        );
        self.t = now;

        let AppState::Running(state) = &self.state else {
            return;
        };

        if matches!(window_event, WindowEvent::CloseRequested) {
            super::browser::with(|b| b.capture(state));
        }

        if let Some(window) = state.window(ServoShellWindowId::from(u64::from(window_id)))
            && let Some(headed_window) = window.platform_window().as_headed_window()
        {
            headed_window.handle_winit_window_event(state.clone(), window, window_event);
        }

        if !self.pump_servo_event_loop(event_loop.into()) {
            event_loop.exit();
        }
        // Block until the window gets an event
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, app_event: AppEvent) {
        let AppState::Running(state) = &self.state else {
            return;
        };

        match app_event {
            AppEvent::Waker => (),
            AppEvent::GuiRepaint(id, delay) => {
                let deadline = Instant::now() + delay.min(std::time::Duration::from_secs(3600));
                self.ui_repaints
                    .entry(id)
                    .and_modify(|current| *current = (*current).min(deadline))
                    .or_insert(deadline);
            },
            AppEvent::OpenPasswordManager(source) => {
                let existing = state
                    .windows()
                    .iter()
                    .find(|w| {
                        w.platform_window()
                            .as_headed_window()
                            .is_some_and(|h| h.is_password_manager())
                    })
                    .cloned();
                if let Some(existing) = existing {
                    if let Some(headed) = existing.platform_window().as_headed_window() {
                        headed.set_password_source(source);
                        headed.winit_window().set_minimized(false);
                        headed.winit_window().focus_window();
                        existing.set_needs_update();
                        existing.set_needs_repaint();
                    }
                } else {
                    let url = Url::parse("about:blank").unwrap();
                    let platform = HeadedWindow::new(
                        &self.servoshell_preferences,
                        event_loop,
                        self.event_loop_proxy.clone().unwrap(),
                        url.clone(),
                        Some(source),
                    );
                    state.open_window(platform, TopLevelWebViewCreationRequest::WithUrl(url));
                }
            },
            AppEvent::BrowserChanged => {
                super::browser::with(|b| b.poll(state));
                for window in state.windows().iter() {
                    window.set_needs_update();
                    window.set_needs_repaint();
                }
            },
            AppEvent::Accessibility(ref event) => {
                if let Some(window) =
                    state.window(ServoShellWindowId::from(u64::from(event.window_id)))
                    && let Some(headed_window) = window.platform_window().as_headed_window()
                {
                    headed_window.handle_winit_app_event(state.clone(), app_event);
                }
            },
            #[cfg(feature = "gamepad")]
            AppEvent::Gamepad(event, gamepad_name, gamepad_index) => {
                state.handle_gamepad_events(event, gamepad_name, gamepad_index);
            },
        }

        if !self.pump_servo_event_loop(event_loop.into()) {
            event_loop.exit();
        }

        // Block until the window gets an event
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

fn load_userscripts(userscripts_directory: Option<&Path>) -> std::io::Result<Vec<UserScript>> {
    let mut userscripts = Vec::new();
    if let Some(userscripts_directory) = &userscripts_directory {
        let mut files = std::fs::read_dir(userscripts_directory)?
            .map(|e| e.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        files.sort_unstable();
        for file in files {
            let script = std::fs::read_to_string(&file)?;
            userscripts.push(UserScript::new(script, Some(file)));
        }
    }
    Ok(userscripts)
}
