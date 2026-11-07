//! Rust Browser's desktop profile and connection lifetime, shared between windows.
use super::event_loop::AppEvent;
use crate::running_app_state::RunningAppState;
use rust_browser_core::passwords::Vault;
use rust_browser_core::{
    Node, Profile, RouteMode, parse_subscription, proxy_config, secrets, subscription_input,
};
use servo::{PrefValue, Preferences};
use std::{
    cell::RefCell,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
use winit::event_loop::EventLoopProxy;

thread_local! { static BROWSER: RefCell<Browser> = RefCell::new(Browser::load()); }
pub fn with<R>(f: impl FnOnce(&mut Browser) -> R) -> R {
    BROWSER.with(|b| f(&mut b.borrow_mut()))
}
pub fn profile_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("RUST_BROWSER_PROFILE") {
        return PathBuf::from(path);
    }
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("RustBrowser")
}
pub fn open_password_manager(source: crate::window::ServoShellWindowId) {
    with(|b| {
        if let Some(wake) = &b.wake {
            let _ = wake.send_event(AppEvent::OpenPasswordManager(source));
        }
    });
}

pub struct RunningProxy {
    child: Child,
    pub verified: bool,
}
#[cfg(windows)]
struct KillJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for KillJob {}
#[cfg(windows)]
unsafe impl Sync for KillJob {}
#[cfg(windows)]
impl Drop for KillJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(windows)]
impl KillJob {
    fn new() -> Result<Self, String> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err("Не удалось создать группу сетевого процесса.".into());
            }
            let job = Self(handle);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                return Err("Не удалось ограничить время жизни сетевого процесса.".into());
            }
            Ok(job)
        }
    }
    fn assign(&self, child: &Child) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        if unsafe {
            windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(
                self.0,
                child.as_raw_handle(),
            )
        } == 0
        {
            Err("Не удалось привязать сетевой процесс к браузеру.".into())
        } else {
            Ok(())
        }
    }
}
#[cfg(not(windows))]
struct KillJob;
#[cfg(not(windows))]
impl KillJob {
    fn new() -> Result<Self, String> {
        Ok(Self)
    }
    fn assign(&self, _: &Child) -> Result<(), String> {
        Ok(())
    }
}
impl Drop for RunningProxy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
type ProxyResult = Result<RunningProxy, String>;

pub struct Browser {
    pub profile: Profile,
    pub nodes: Vec<Node>,
    pub selected: usize,
    pub status: String,
    pub error: Option<String>,
    pub vault: Vault,
    pub vault_error: Option<String>,
    vault_writable: bool,
    writable: bool,
    import: Option<Receiver<Result<Vec<Node>, String>>>,
    connecting: Option<Receiver<ProxyResult>>,
    running: Option<RunningProxy>,
    port: Option<u16>,
    applied: String,
    last_save: Instant,
    wake: Option<EventLoopProxy<AppEvent>>,
    job: Option<std::sync::Arc<KillJob>>,
}
impl Browser {
    fn load() -> Self {
        let (profile, error, writable) = match Profile::load(&profile_dir().join("profile.json")) {
            Ok(p) => (p, None, true),
            Err(e) => (Profile::default(), Some(e), false),
        };
        let mut selected = 0;
        let (vault, vault_error, vault_writable) =
            match Vault::load(&profile_dir().join("passwords.dat")) {
                Ok(v) => (v, None, true),
                Err(e) => (Vault::default(), Some(e), false),
            };
        let nodes = std::fs::read(profile_dir().join("connections.dat"))
            .ok()
            .and_then(|b| secrets::unprotect(&b).ok())
            .and_then(|b| serde_json::from_slice::<(Vec<Node>, usize)>(&b).ok())
            .map(|(nodes, index)| {
                selected = index;
                nodes
            })
            .unwrap_or_default();
        Self {
            profile,
            nodes,
            selected,
            status: "Прямое соединение".into(),
            error,
            writable,
            vault,
            vault_error,
            vault_writable,
            import: None,
            connecting: None,
            running: None,
            port: None,
            applied: String::new(),
            last_save: Instant::now(),
            wake: None,
            job: None,
        }
    }
    pub fn save(&mut self) {
        if self.writable {
            if let Err(e) = self.profile.save(&profile_dir().join("profile.json")) {
                self.error = Some(e);
            }
        }
        self.last_save = Instant::now();
    }
    pub fn save_nodes(&mut self) {
        let result = (|| {
            let data = serde_json::to_vec(&(&self.nodes, self.selected))
                .map_err(|_| "Не удалось сохранить серверы.")?;
            let encrypted = secrets::protect(&data)?;
            std::fs::create_dir_all(profile_dir()).map_err(|_| "Не удалось создать профиль.")?;
            std::fs::write(profile_dir().join("connections.dat"), encrypted)
                .map_err(|_| "Не удалось сохранить ключи.".to_string())
        })();
        if let Err(e) = result {
            self.error = Some(e);
        }
    }
    pub fn replace_vault(&mut self, next: Vault) -> Result<(), String> {
        if !self.vault_writable {
            return Err("Хранилище повреждено; исходный файл сохранён, запись отключена.".into());
        }
        next.save(&profile_dir().join("passwords.dat"))?;
        self.vault = next;
        Ok(())
    }
    pub fn import_brave(&mut self) {
        if !self.writable {
            self.error =
                Some("Профиль повреждён; импорт отключён, чтобы сохранить исходный файл.".into());
            return;
        }
        let result = import_brave_candidate(&self.profile).and_then(|(next, status)| {
            save_imported_profile(&next)?;
            self.profile = next;
            Ok(status)
        });
        match result {
            Ok(status) => {
                self.status = status;
                self.error = None;
            },
            Err(error) => self.error = Some(error),
        }
    }
    pub fn import_subscription(&mut self, input: String) {
        if self.import.is_some() {
            return;
        }
        self.error = None;
        self.status = "Импорт подписки…".into();
        let (tx, rx) = mpsc::channel();
        self.import = Some(rx);
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let input = subscription_input(&input)?;
                let text = if input.starts_with("https://") {
                    let client = reqwest::blocking::Client::builder()
                        .no_proxy()
                        .https_only(true)
                        .redirect(reqwest::redirect::Policy::limited(4))
                        .timeout(Duration::from_secs(25))
                        .user_agent("Happ/2.0 RustBrowser/0.1")
                        .build()
                        .map_err(|_| "Не удалось создать соединение.")?;
                    // Deliberately classify errors instead of displaying reqwest's
                    // Display text, which can contain the secret subscription URL.
                    let mut response = client.get(&input).send().map_err(|e| {
                        if e.is_timeout() { "Сервер подписки не ответил за 25 секунд. Попробуй позже." }
                        else if e.is_connect() { "Не удалось соединиться с сервером подписки. Проверь доступность провайдера." }
                        else { "Не удалось загрузить подписку по HTTPS. Проверь ссылку и сертификат сервера." }
                    })?;
                    if !response.status().is_success() {
                        return Err(format!(
                            "Сервер подписки вернул HTTP {}. Проверь срок доступа и актуальность ссылки.",
                            response.status().as_u16()
                        ));
                    }
                    let mut text = String::new();
                    response
                        .by_ref()
                        .take(4 * 1024 * 1024 + 1)
                        .read_to_string(&mut text)
                        .map_err(|_| "Не удалось прочитать подписку.")?;
                    text
                } else {
                    input.to_owned()
                };
                parse_subscription(&text)
            })();
            let _ = tx.send(result);
            if let Some(wake) = wake {
                let _ = wake.send_event(AppEvent::BrowserChanged);
            }
        });
    }
    pub fn connect(&mut self) {
        let Some(node) = self.nodes.get(self.selected).cloned() else {
            self.error = Some("Сначала импортируй подписку и выбери сервер.".into());
            return;
        };
        self.disconnect();
        let port = match TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()) {
            Ok(addr) => addr.port(),
            Err(_) => {
                self.error = Some("Не удалось открыть локальный порт.".into());
                return;
            },
        };
        self.port = Some(port);
        self.status = "Подключение и проверка…".into();
        self.error = None;
        let job = match KillJob::new() {
            Ok(job) => std::sync::Arc::new(job),
            Err(e) => {
                self.error = Some(e);
                return;
            },
        };
        self.job = Some(job.clone());
        let (tx, rx) = mpsc::channel();
        self.connecting = Some(rx);
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let _ = tx.send(start_proxy(node, port, job));
            if let Some(wake) = wake {
                let _ = wake.send_event(AppEvent::BrowserChanged);
            }
        });
    }
    pub fn disconnect(&mut self) {
        self.connecting = None;
        self.running = None;
        self.port = None;
        self.job = None;
        self.status = "Прямое соединение".into();
        self.applied.clear();
    }
    pub fn busy(&self) -> bool {
        self.import.is_some() || self.connecting.is_some()
    }
    pub fn connected(&self) -> bool {
        self.running.is_some()
    }
    pub fn poll(&mut self, state: &RunningAppState) -> bool {
        let before = (
            self.status.clone(),
            self.error.clone(),
            self.busy(),
            self.connected(),
            self.nodes.len(),
        );
        let previous_routing = self.applied.clone();
        if let Some(result) = self.import.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.import = None;
            match result {
                Ok(nodes) => {
                    self.nodes = nodes;
                    self.selected = 0;
                    self.save_nodes();
                    self.status = format!("Импортировано серверов: {}", self.nodes.len());
                },
                Err(e) => {
                    self.error = Some(e);
                    self.status = "Ошибка импорта".into();
                },
            }
        }
        if let Some(result) = self.connecting.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.connecting = None;
            match result {
                Ok(proxy) => {
                    self.status = if proxy.verified {
                        "Соединение проверено"
                    } else {
                        "Ядро запущено; сервер не прошёл проверку"
                    }
                    .into();
                    self.running = Some(proxy);
                },
                Err(e) => {
                    self.error = Some(e);
                    self.status = "Ошибка подключения".into();
                },
            }
        }
        if let Some(proxy) = &mut self.running {
            if !matches!(proxy.child.try_wait(), Ok(None)) {
                self.running = None;
                self.status = "Соединение прервано".into();
                // Keep the closed proxy endpoint, so proxy mode never falls back to direct.
            }
        }
        let endpoint = self
            .port
            .map(|p| format!("http://127.0.0.1:{p}"))
            .unwrap_or_else(|| "http://127.0.0.1:9".into());
        let mode = mode_string(self.profile.route_mode);
        let identity = format!(
            "{mode}|{endpoint}|{}|{}",
            self.profile.direct_domains, self.profile.proxy_domains
        );
        if identity != self.applied {
            state
                .servo
                .set_preference("network_http_proxy_uri", PrefValue::Str(endpoint.clone()));
            state
                .servo
                .set_preference("network_https_proxy_uri", PrefValue::Str(endpoint));
            state
                .servo
                .set_preference("network_browser_route_mode", PrefValue::Str(mode.into()));
            state.servo.set_preference(
                "network_browser_direct_domains",
                PrefValue::Str(self.profile.direct_domains.clone()),
            );
            state.servo.set_preference(
                "network_browser_proxy_domains",
                PrefValue::Str(self.profile.proxy_domains.clone()),
            );
            self.applied = identity;
        }
        if self.last_save.elapsed() > Duration::from_secs(3) {
            self.capture(state);
        }
        before
            != (
                self.status.clone(),
                self.error.clone(),
                self.busy(),
                self.connected(),
                self.nodes.len(),
            )
            || previous_routing != self.applied
    }
    pub fn capture(&mut self, state: &RunningAppState) {
        self.snapshot(state);
        self.save();
    }
    pub fn snapshot(&mut self, state: &RunningAppState) {
        let tabs: Vec<_> = state
            .windows()
            .iter()
            .filter(|w| {
                !w.platform_window()
                    .as_headed_window()
                    .is_some_and(|h| h.is_password_manager())
            })
            .flat_map(|w| w.webviews().into_iter().map(|(_, v)| v))
            .collect();
        let session = tabs
            .iter()
            .filter_map(|v| v.url().map(|u| u.to_string()))
            .collect::<Vec<_>>();
        if !session.is_empty() {
            self.profile.session = session;
        }
        if let Some(active) = state
            .focused_window()
            .filter(|w| {
                !w.platform_window()
                    .as_headed_window()
                    .is_some_and(|h| h.is_password_manager())
            })
            .and_then(|w| w.active_webview())
        {
            self.profile.active_tab = tabs.iter().position(|v| v.id() == active.id()).unwrap_or(0);
            if let Some(url) = active.url() {
                self.profile
                    .visit(url.as_str(), &active.page_title().unwrap_or_default());
            }
        }
    }
}

/// Profile-only CLI import. Does not initialize the password vault or connection keys.
pub fn import_brave_without_window() -> Result<String, String> {
    let profile = Profile::load(&profile_dir().join("profile.json"))?;
    let (next, status) = import_brave_candidate(&profile)?;
    save_imported_profile(&next)?;
    Ok(status)
}

fn save_imported_profile(profile: &Profile) -> Result<(), String> {
    let path = profile_dir().join("profile.json");
    if path.is_file() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "Не удалось подготовить резервную копию.")?
            .as_millis();
        std::fs::copy(
            &path,
            path.with_extension(format!("before-brave-{stamp}.json")),
        )
        .map_err(|_| "Не удалось создать резервную копию; импорт отменён.")?;
    }
    profile.save(&path)
}

fn import_brave_candidate(profile: &Profile) -> Result<(Profile, String), String> {
    let source = dirs::data_local_dir()
        .ok_or("Не найдена папка пользователя.")?
        .join("BraveSoftware/Brave-Browser/User Data/Default");
    let mut next = profile.clone();
    let bookmarks = std::fs::read(source.join("Bookmarks"))
        .map_err(|_| "Не найдены закладки профиля Brave Default.")?;
    let added = next.import_brave_bookmarks(&bookmarks)?;
    let mut history_count = 0;
    let history_ok = (|| {
        let database = rusqlite::Connection::open_with_flags(source.join("History"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
        database.busy_timeout(Duration::from_millis(500)).ok()?;
        let mut statement = database.prepare("SELECT url, COALESCE(title, '') FROM urls WHERE hidden = 0 AND (url LIKE 'https://%' OR url LIKE 'http://%') ORDER BY last_visit_time DESC LIMIT 10000").ok()?;
        let rows = statement.query_map([], |row| Ok(rust_browser_core::Page { url: row.get(0)?, title: row.get(1)? })).ok()?;
        let imported = rows.collect::<Result<Vec<_>,_>>().ok()?;
        for page in imported.into_iter().rev() {
            if !next.history.iter().any(|p| p.url == page.url) { history_count += 1; }
            next.visit(&page.url, &page.title);
        }
        Some(())
    })().is_some();
    let status = format!(
        "Из Brave: закладок {added}, страниц истории {history_count}{}",
        if history_ok {
            ""
        } else {
            ". История занята или недоступна; закладки перенесены"
        }
    );
    Ok((next, status))
}
pub fn mode_string(mode: RouteMode) -> &'static str {
    match mode {
        RouteMode::Direct => "direct",
        RouteMode::Auto => "auto",
        RouteMode::Proxy => "proxy",
    }
}
pub fn initialize(preferences: &mut Preferences, wake: Option<EventLoopProxy<AppEvent>>) {
    with(|b| {
        b.wake = wake;
        if std::env::var("RUST_BROWSER_IMPORT_BRAVE").as_deref() == Ok("1") {
            b.import_brave();
        }
        if b.profile.route_mode != RouteMode::Direct {
            b.connect();
        }
        preferences.network_browser_route_mode = mode_string(b.profile.route_mode).into();
        preferences.network_browser_direct_domains = b.profile.direct_domains.clone();
        preferences.network_browser_proxy_domains = b.profile.proxy_domains.clone();
        let endpoint = b
            .port
            .map(|p| format!("http://127.0.0.1:{p}"))
            .unwrap_or_else(|| "http://127.0.0.1:9".into());
        preferences.network_http_proxy_uri = endpoint.clone();
        preferences.network_https_proxy_uri = endpoint;
        preferences.network_http_no_proxy = String::new();
    });
}
pub fn shutdown() {
    with(|b| {
        b.save();
        b.disconnect();
    });
}

fn start_proxy(node: Node, port: u16, job: std::sync::Arc<KillJob>) -> ProxyResult {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_default();
    let engine = if node.uses_xray() {
        "xray.exe"
    } else {
        "sing-box.exe"
    };
    let candidates = [
        exe_dir.join("runtime").join(engine),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../runtime")
            .join(engine),
    ];
    let exe = candidates.iter().find(|p| p.is_file()).ok_or(
        "Рядом с браузером не найдено сетевое ядро в папке runtime. Восстанови файлы браузера.",
    )?;
    let mut command = Command::new(exe);
    command
        .args(if node.uses_xray() {
            ["run", "-config", "stdin:"]
        } else {
            ["run", "-c", "stdin"]
        })
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let child = command
        .spawn()
        .map_err(|_| "Не удалось запустить сетевое ядро.")?;
    let mut proxy = RunningProxy {
        child,
        verified: false,
    };
    job.assign(&proxy.child)?;
    // The UI alone keeps the job alive after assignment. Disconnect closes it even
    // while this worker is in the remote connectivity check. OS exit closes it too.
    drop(job);
    let config =
        serde_json::to_vec(&proxy_config(&node, port)).map_err(|_| "Неверный профиль сервера.")?;
    proxy
        .child
        .stdin
        .take()
        .ok_or("Нет канала настройки ядра.")?
        .write_all(&config)
        .map_err(|_| "Сетевое ядро отклонило настройки.")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(100))
            .is_ok()
        {
            break;
        }
        if !matches!(proxy.child.try_wait(), Ok(None)) || Instant::now() > deadline {
            return Err(
                "Сетевое ядро не запустилось. Возможно, параметры сервера не поддерживаются."
                    .into(),
            );
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    let remote_check = (|| {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}")).ok()?)
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        client
            .get("https://example.com/")
            .send()
            .ok()?
            .error_for_status()
            .ok()
    })();
    proxy.verified = remote_check.is_some();
    Ok(proxy)
}
