//! Profile and subscription conversion. Secret-bearing types intentionally omit Debug.
use base64::Engine;
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path};
use url::Url;
pub mod passwords;
pub mod secrets;
pub mod session;
pub const MAX_HISTORY_ENTRIES: usize = 10_000;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteMode {
    #[default]
    Direct,
    Auto,
    Proxy,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTheme {
    #[default]
    Dark,
    Light,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchEngine {
    #[default]
    Brave,
    Google,
    Yandex,
    DuckDuckGo,
}
impl SearchEngine {
    pub fn template(self) -> &'static str {
        match self {
            Self::Brave => "https://search.brave.com/search?q=%s",
            Self::Google => "https://www.google.com/search?q=%s",
            Self::Yandex => "https://yandex.ru/search/?text=%s",
            Self::DuckDuckGo => "https://duckduckgo.com/?q=%s",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: BrowserTheme,
    pub accent: usize,
    pub bookmarks_bar: bool,
    pub compact: bool,
    pub restore_session: bool,
    pub search_engine: SearchEngine,
    pub glass: bool,
    pub animations: bool,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: BrowserTheme::Dark,
            accent: 0,
            bookmarks_bar: false,
            compact: false,
            restore_session: true,
            search_engine: SearchEngine::Brave,
            glass: false,
            animations: true,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub appearance: Appearance,
    pub bookmarks: Vec<Page>,
    pub history: Vec<Page>,
    pub recently_closed: Vec<Page>,
    pub session: Vec<String>,
    pub active_tab: usize,
    pub route_mode: RouteMode,
    pub direct_domains: String,
    pub proxy_domains: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Page {
    pub title: String,
    pub url: String,
}
impl Page {
    pub fn matches(&self, query: &str) -> bool {
        let text = format!("{} {}", self.title, self.url).to_lowercase();
        query
            .split_whitespace()
            .all(|word| text.contains(&word.to_lowercase()))
    }
}

impl Profile {
    pub fn remember_closed(&mut self, url: &str, title: &str) {
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return;
        }
        self.recently_closed.insert(
            0,
            Page {
                url: url.into(),
                title: title.into(),
            },
        );
        self.recently_closed.truncate(20);
    }
    pub fn take_closed(&mut self) -> Option<Page> {
        if self.recently_closed.is_empty() {
            None
        } else {
            Some(self.recently_closed.remove(0))
        }
    }
    pub fn import_brave_bookmarks(&mut self, data: &[u8]) -> Result<usize, String> {
        if data.len() > 16 * 1024 * 1024 {
            return Err("Файл закладок больше 16 МБ.".into());
        }
        let json: Value =
            serde_json::from_slice(data).map_err(|_| "Не удалось прочитать закладки Brave.")?;
        let roots = json
            .get("roots")
            .and_then(Value::as_object)
            .ok_or("Неизвестный формат закладок Brave.")?;
        let mut pending: Vec<&Value> = roots.values().collect();
        let mut added = 0;
        while let Some(value) = pending.pop() {
            if let Some(children) = value.get("children").and_then(Value::as_array) {
                pending.extend(children.iter().rev());
            }
            if value.get("type").and_then(Value::as_str) != Some("url") {
                continue;
            }
            let Some(url) = value.get("url").and_then(Value::as_str) else {
                continue;
            };
            if !url.starts_with("https://") && !url.starts_with("http://") {
                continue;
            }
            if self.bookmarks.iter().any(|page| page.url == url) {
                continue;
            }
            self.bookmarks.push(Page {
                url: url.into(),
                title: value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(url)
                    .into(),
            });
            added += 1;
        }
        Ok(added)
    }
    pub fn toggle_bookmark(&mut self, url: &str, title: &str) {
        if self.bookmarks.iter().any(|p| p.url == url) {
            self.bookmarks.retain(|p| p.url != url);
        } else if url.starts_with("https://") || url.starts_with("http://") {
            self.bookmarks.push(Page {
                title: title.into(),
                url: url.into(),
            });
        }
    }
    pub fn visit(&mut self, url: &str, title: &str) {
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return;
        }
        if self
            .history
            .first()
            .is_some_and(|p| p.url == url && p.title == title)
        {
            return;
        }
        self.history.retain(|p| p.url != url);
        self.history.insert(
            0,
            Page {
                title: title.into(),
                url: url.into(),
            },
        );
        self.history.truncate(MAX_HISTORY_ENTRIES);
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(data) => serde_json::from_slice(&data)
                .map_err(|_| "Профиль повреждён; исходный файл сохранён.".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => Err("Не удалось прочитать профиль браузера.".into()),
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| "Не удалось создать папку профиля.")?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| "Не удалось сохранить профиль.")?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, bytes).map_err(|_| "Не удалось записать профиль.")?;
        std::fs::rename(&temp, path).map_err(|_| "Не удалось заменить файл профиля.".into())
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub outbound: Value,
}
impl Node {
    pub fn uses_xray(&self) -> bool {
        self.outbound["transport"]["type"] == "xhttp"
    }
}

/// Unwrap only the outer Happ command. Percent escapes inside an ordinary URL
/// are part of the provider's token and must remain unchanged.
pub fn subscription_input(input: &str) -> Result<String, String> {
    let input = input.trim().trim_start_matches('\u{feff}');
    if let Some(inner) = input.strip_prefix("happ://add/") {
        let inner = if inner.contains("://") {
            inner.to_owned()
        } else {
            decoded(inner)
        };
        if inner.is_empty() {
            return Err("В ссылке Happ нет адреса подписки.".into());
        }
        return Ok(inner);
    }
    if input.starts_with("happ://") {
        return Err("Зашифрованная подписка Happ открывается только в Happ. Возьми у провайдера обычную HTTPS-подписку или ссылку VLESS.".into());
    }
    Ok(input.to_owned())
}
fn decoded(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().into_owned()
}
fn from_base64(text: &str) -> Result<String, String> {
    let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    [
        base64::engine::general_purpose::STANDARD,
        base64::engine::general_purpose::STANDARD_NO_PAD,
        base64::engine::general_purpose::URL_SAFE,
        base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ]
    .into_iter()
    .find_map(|e| {
        e.decode(&clean)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    })
    .ok_or_else(|| "Не удалось декодировать подписку.".into())
}
pub fn parse_subscription(text: &str) -> Result<Vec<Node>, String> {
    if text.len() > 4 * 1024 * 1024 {
        return Err("Подписка больше 4 МБ.".into());
    }
    let input = subscription_input(text)?;
    let text = input.as_str();
    let plain;
    let text = if !text.contains("://") {
        plain = from_base64(text)?;
        plain.as_str()
    } else {
        text
    };
    let mut nodes = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
    {
        if nodes.len() >= 512 {
            return Err("Слишком много серверов в подписке.".into());
        }
        nodes.push(parse_node(line)?);
    }
    if nodes.is_empty() {
        return Err("В подписке нет поддерживаемых серверов.".into());
    }
    Ok(nodes)
}
fn parse_node(text: &str) -> Result<Node, String> {
    if let Some(encoded) = text.strip_prefix("vmess://") {
        return parse_vmess(encoded);
    }
    if let Some(encoded) = text.strip_prefix("ss://") {
        return parse_shadowsocks(encoded);
    }
    let url = Url::parse(text).map_err(|_| "Неверный формат ссылки сервера.")?;
    if !matches!(url.scheme(), "vless" | "trojan") {
        return Err("Поддерживаются VLESS, VMess, Trojan и Shadowsocks.".into());
    }
    let host = url.host_str().ok_or("В ссылке сервера нет адреса.")?;
    let port = url.port().ok_or("В ссылке сервера нет порта.")?;
    let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let get = |key: &str| q.get(key).map(String::as_str).unwrap_or("");
    if get("allowInsecure") == "1" || get("insecure") == "1" {
        return Err("Сервер требует отключения проверки TLS; этот режим не поддерживается.".into());
    }
    let mut out = json!({"type":url.scheme(), "server":host, "server_port":port});
    let user = decoded(url.username());
    if user.is_empty() {
        return Err("В ссылке сервера нет ключа доступа.".into());
    }
    if url.scheme() == "vless" {
        out["uuid"] = user.into();
        if !get("flow").is_empty() {
            out["flow"] = get("flow").into();
        }
        if !get("encryption").is_empty() && get("encryption") != "none" {
            return Err("Этот вариант шифрования VLESS пока не поддерживается.".into());
        }
    } else {
        out["password"] = user.into();
    }
    let security = if get("security").is_empty() && url.scheme() == "trojan" {
        "tls"
    } else {
        get("security")
    };
    if matches!(security, "tls" | "reality") {
        let sni = if get("sni").is_empty() {
            host
        } else {
            get("sni")
        };
        let mut tls = json!({"enabled":true,"server_name":sni});
        if !get("fp").is_empty() {
            tls["utls"] = json!({"enabled":true,"fingerprint":get("fp")});
        }
        if !get("alpn").is_empty() {
            tls["alpn"] = get("alpn").split(',').collect::<Vec<_>>().into();
        }
        if security == "reality" {
            if get("pbk").is_empty() {
                return Err("Для Reality не указан публичный ключ.".into());
            }
            tls["reality"] = json!({"enabled":true,"public_key":get("pbk"),"short_id":get("sid")});
            if get("fp").is_empty() {
                tls["utls"] = json!({"enabled":true,"fingerprint":"chrome"});
            }
        }
        out["tls"] = tls;
    } else if !security.is_empty() && security != "none" {
        return Err("Этот вид защиты соединения пока не поддерживается.".into());
    }
    set_transport(
        &mut out,
        get("type"),
        get("path"),
        get("host"),
        get("serviceName"),
    )?;
    if out["transport"]["type"] == "xhttp" {
        let mode = if get("mode").is_empty() {
            "auto"
        } else {
            get("mode")
        };
        if !matches!(mode, "auto" | "packet-up" | "stream-up" | "stream-one") {
            return Err("Неизвестный режим XHTTP.".into());
        }
        out["transport"]["mode"] = mode.into();
        if !get("extra").is_empty() {
            let extra: Value = serde_json::from_str(get("extra"))
                .map_err(|_| "Неверные дополнительные настройки XHTTP.")?;
            if !extra.is_object() {
                return Err("Дополнительные настройки XHTTP должны быть объектом JSON.".into());
            }
            out["transport"]["extra"] = extra;
        }
    }
    Ok(Node {
        name: decoded(url.fragment().unwrap_or(host))
            .chars()
            .filter(|c| !c.is_control())
            .take(80)
            .collect(),
        outbound: out,
    })
}
fn set_transport(
    out: &mut Value,
    kind: &str,
    path: &str,
    host: &str,
    service: &str,
) -> Result<(), String> {
    match kind {
        "" | "tcp" | "raw" => {},
        "xhttp" | "splithttp" => {
            out["transport"] = json!({"type":"xhttp","path":if path.is_empty(){"/"}else{path},"host":host,"mode":"auto"});
        },
        "ws" | "httpupgrade" => {
            let mut t = json!({"type":kind,"path":if path.is_empty(){"/"}else{path}});
            if !host.is_empty() {
                if kind == "ws" {
                    t["headers"] = json!({"Host":host});
                } else {
                    t["host"] = host.into();
                }
            }
            out["transport"] = t;
        },
        "grpc" => out["transport"] = json!({"type":"grpc","service_name":service}),
        _ => {
            return Err(
                "Транспорт сервера пока не поддерживается. Доступны TCP, WS, gRPC, HTTPUpgrade и XHTTP."
                    .into(),
            );
        },
    }
    Ok(())
}
fn parse_vmess(encoded: &str) -> Result<Node, String> {
    let v: Value =
        serde_json::from_str(&from_base64(encoded)?).map_err(|_| "Неверная ссылка VMess.")?;
    let get = |key: &str| v[key].as_str().unwrap_or("");
    let port = v["port"]
        .as_u64()
        .or_else(|| get("port").parse().ok())
        .filter(|p| *p > 0 && *p <= 65535)
        .ok_or("Неверный порт VMess.")?;
    if get("add").is_empty() || get("id").is_empty() {
        return Err("В VMess не указан сервер или ключ.".into());
    }
    let aid = v["aid"]
        .as_u64()
        .or_else(|| get("aid").parse().ok())
        .unwrap_or(0);
    let mut out = json!({"type":"vmess","server":get("add"),"server_port":port,"uuid":get("id"),"security":if get("scy").is_empty(){"auto"}else{get("scy")},"alter_id":aid});
    if get("tls") == "tls" {
        out["tls"] = json!({"enabled":true,"server_name":if get("sni").is_empty(){get("add")}else{get("sni")}});
    }
    set_transport(&mut out, get("net"), get("path"), get("host"), get("path"))?;
    Ok(Node {
        name: if get("ps").is_empty() {
            get("add")
        } else {
            get("ps")
        }
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect(),
        outbound: out,
    })
}
fn parse_shadowsocks(encoded: &str) -> Result<Node, String> {
    let (body, name) = encoded.split_once('#').unwrap_or((encoded, "Shadowsocks"));
    let expanded = if body.contains('@') {
        body.to_owned()
    } else {
        from_base64(body)?
    };
    let (user, server) = expanded
        .rsplit_once('@')
        .ok_or("Неверная ссылка Shadowsocks.")?;
    let credentials = if user.contains(':') {
        decoded(user)
    } else {
        from_base64(user)?
    };
    let (method, password) = credentials
        .split_once(':')
        .ok_or("Неверный ключ Shadowsocks.")?;
    let url = Url::parse(&format!("http://{server}")).map_err(|_| "Неверный адрес Shadowsocks.")?;
    if url.query_pairs().any(|(k, _)| k == "plugin") {
        return Err("Плагины Shadowsocks пока не поддерживаются.".into());
    }
    Ok(Node {
        name: decoded(name).chars().take(80).collect(),
        outbound: json!({"type":"shadowsocks","server":url.host_str().ok_or("Нет адреса сервера.")?,"server_port":url.port().ok_or("Нет порта сервера.")?,"method":method,"password":password}),
    })
}
pub fn proxy_config(node: &Node, port: u16) -> Value {
    if node.uses_xray() {
        return xray_config(node, port);
    }
    let mut outbound = node.outbound.clone();
    outbound["tag"] = "subscription".into();
    json!({"log":{"level":"error","disabled":true},"inbounds":[{"type":"http","tag":"browser","listen":"127.0.0.1","listen_port":port}],"outbounds":[outbound],"route":{"final":"subscription"}})
}

fn xray_config(node: &Node, port: u16) -> Value {
    let out = &node.outbound;
    let tls = &out["tls"];
    let mut xhttp = out["transport"].clone();
    xhttp.as_object_mut().unwrap().remove("type");
    let mut stream = json!({"network":"xhttp", "security":"none", "xhttpSettings":xhttp});
    if tls["enabled"] == true {
        let fingerprint = tls["utls"]["fingerprint"].as_str().unwrap_or("chrome");
        if tls["reality"]["enabled"] == true {
            stream["security"] = "reality".into();
            stream["realitySettings"] = json!({"serverName":tls["server_name"],"fingerprint":fingerprint,"publicKey":tls["reality"]["public_key"],"shortId":tls["reality"]["short_id"]});
        } else {
            stream["security"] = "tls".into();
            let mut settings = json!({"serverName":tls["server_name"],"fingerprint":fingerprint,"allowInsecure":false});
            if tls["alpn"].is_array() {
                settings["alpn"] = tls["alpn"].clone();
            }
            stream["tlsSettings"] = settings;
        }
    }
    let protocol = out["type"].as_str().unwrap_or("vless");
    let settings = if protocol == "trojan" {
        json!({"servers":[{"address":out["server"],"port":out["server_port"],"password":out["password"]}]})
    } else {
        let mut user = json!({"id":out["uuid"],"encryption":"none"});
        if protocol == "vmess" {
            user = json!({"id":out["uuid"],"alterId":out["alter_id"],"security":out["security"]});
        }
        if out["flow"].is_string() {
            user["flow"] = out["flow"].clone();
        }
        json!({"vnext":[{"address":out["server"],"port":out["server_port"],"users":[user]}]})
    };
    json!({"log":{"loglevel":"none"},"inbounds":[{"listen":"127.0.0.1","port":port,"protocol":"http","settings":{}}],"outbounds":[{"tag":"subscription","protocol":protocol,"settings":settings,"streamSettings":stream}]})
}
pub fn domain_matches(host: &str, rules: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    rules
        .split([',', ';', '\n', ' '])
        .filter(|r| !r.is_empty())
        .any(|r| {
            let r = r
                .trim()
                .trim_start_matches("*.")
                .trim_end_matches('.')
                .to_ascii_lowercase();
            host == r || host.ends_with(&format!(".{r}"))
        })
}
