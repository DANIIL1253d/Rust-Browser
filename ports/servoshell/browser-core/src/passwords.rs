use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
pub struct Credential {
    pub origin: String,
    pub username: String,
    pub password: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Vault {
    pub entries: Vec<Credential>,
}
pub fn origin(site: &str) -> Result<String, String> {
    let url = url::Url::parse(site).map_err(|_| "Введи полный адрес сайта: https://example.org")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Пароли привязываются к HTTP или HTTPS-сайту.".into());
    }
    Ok(url.origin().ascii_serialization())
}
impl Vault {
    /// Existing site/login pairs always win; imports never replace a password.
    pub fn import_entries(&mut self, entries: Vec<Credential>) -> (usize, usize) {
        let mut added = 0;
        let mut skipped = 0;
        for entry in entries {
            if self
                .entries
                .iter()
                .any(|e| e.origin == entry.origin && e.username == entry.username)
            {
                skipped += 1;
            } else {
                self.entries.push(entry);
                added += 1;
            }
        }
        (added, skipped)
    }
    pub fn replace(
        &mut self,
        original_origin: &str,
        original_username: &str,
        site: &str,
        username: &str,
        password: &str,
    ) -> Result<(), String> {
        let index = self
            .entries
            .iter()
            .position(|e| e.origin == original_origin && e.username == original_username)
            .ok_or("Запись уже изменена или удалена.")?;
        let mut candidate = Vault::default();
        candidate.upsert(site, username, password)?;
        let entry = candidate.entries.remove(0);
        if self
            .entries
            .iter()
            .enumerate()
            .any(|(i, e)| i != index && e.origin == entry.origin && e.username == entry.username)
        {
            return Err("Для этого сайта и логина уже есть другая запись.".into());
        }
        self.entries[index] = entry;
        Ok(())
    }
    pub fn upsert(&mut self, site: &str, username: &str, password: &str) -> Result<(), String> {
        let origin = origin(site)?;
        if password.is_empty() {
            return Err("Введи пароль.".into());
        }
        if username.len() > 2048 || password.len() > 16384 {
            return Err("Слишком длинный логин или пароль.".into());
        }
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.origin == origin && e.username == username)
        {
            entry.password = password.into();
        } else {
            self.entries.push(Credential {
                origin,
                username: username.into(),
                password: password.into(),
            });
        }
        Ok(())
    }
    pub fn matching(&self, site: &str) -> Vec<usize> {
        let Ok(origin) = origin(site) else {
            return vec![];
        };
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.origin == origin)
            .map(|(i, _)| i)
            .collect()
    }
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let bytes = serde_json::to_vec(self).map_err(|_| "Не удалось сохранить пароли.")?;
        let encrypted = crate::secrets::protect(&bytes)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| "Не удалось создать папку паролей.")?;
        }
        let temp = path.with_extension("dat.tmp");
        std::fs::write(&temp, encrypted).map_err(|_| "Не удалось записать хранилище паролей.")?;
        std::fs::rename(temp, path).map_err(|_| "Не удалось заменить хранилище паролей.".into())
    }
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(_) => return Err("Не удалось прочитать хранилище паролей.".into()),
        };
        let bytes = crate::secrets::unprotect(&bytes)?;
        serde_json::from_slice(&bytes)
            .map_err(|_| "Хранилище паролей повреждено; исходный файл сохранён.".into())
    }
}

/// Read Chromium/Brave and Firefox CSV exports. Error text never includes a row.
pub fn parse_csv(bytes: &[u8]) -> Result<Vec<Credential>, String> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("CSV больше 16 МБ.".into());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "CSV должен быть в кодировке UTF-8.")?
        .trim_start_matches('\u{feff}');
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|_| "Не удалось прочитать заголовок CSV.")?
        .clone();
    let column = |name: &str| {
        headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
    };
    let (site, user, password) = (
        column("url").ok_or("В CSV нет столбца url.")?,
        column("username").ok_or("В CSV нет столбца username.")?,
        column("password").ok_or("В CSV нет столбца password.")?,
    );
    let mut result = Vault::default();
    for (index, row) in reader.records().enumerate() {
        if index >= 10000 {
            return Err("В CSV больше 10 000 записей.".into());
        }
        let row = row.map_err(|_| format!("Не удалось разобрать строку {} CSV.", index + 2))?;
        let mut candidate = Vault::default();
        candidate.upsert(row.get(site).unwrap_or_default(), row.get(user).unwrap_or_default(), row.get(password).unwrap_or_default())
            .map_err(|_| format!("В строке {} CSV нет корректного сайта, логина или пароля. Нужен HTTP/HTTPS-сайт.", index + 2))?;
        let entry = candidate.entries.remove(0);
        if let Some(previous) = result
            .entries
            .iter()
            .find(|e| e.origin == entry.origin && e.username == entry.username)
        {
            if previous.password != entry.password {
                return Err(format!(
                    "Строка {} CSV конфликтует с другой записью: одинаковый сайт и логин, разные пароли. Импорт отменён; раздели эти записи перед переносом.",
                    index + 2
                ));
            }
        } else {
            result.entries.push(entry);
        }
    }
    if result.entries.is_empty() {
        return Err("В CSV нет записей паролей.".into());
    }
    Ok(result.entries)
}

pub fn export_csv(entries: &[Credential]) -> Result<Vec<u8>, String> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer
        .write_record(["name", "url", "username", "password"])
        .map_err(|_| "Не удалось создать CSV.")?;
    for entry in entries {
        writer
            .write_record([
                &entry.origin,
                &entry.origin,
                &entry.username,
                &entry.password,
            ])
            .map_err(|_| "Не удалось записать CSV.")?;
    }
    writer
        .into_inner()
        .map_err(|_| "Не удалось завершить CSV.".into())
}
pub fn fill_script(site: &str, entry: &Credential) -> Result<String, String> {
    fill_script_for_target(site, entry, None)
}

/// A discovery request contains no saved login or password. The target token binds
/// the eventual explicit fill to the same document and input element.
pub fn discovery_script(site: &str, token: &str, focused: bool) -> Result<String, String> {
    let site = origin(site)?;
    let args =
        serde_json::json!({"mode":"discover", "origin":site, "token":token, "focused":focused});
    Ok(format!("{}({args})", include_str!("autofill.js")))
}

pub fn fill_script_for_target(
    site: &str,
    entry: &Credential,
    token: Option<&str>,
) -> Result<String, String> {
    let site = origin(site)?;
    if site != entry.origin {
        return Err("Эта запись сохранена для другого сайта.".into());
    }
    let args = serde_json::json!({"mode":"fill", "origin":site, "username":entry.username,
        "password":entry.password, "token":token});
    Ok(format!("{}({args})", include_str!("autofill.js")))
}

/// Called only after the user explicitly chooses to read the current login form.
pub const CAPTURE_SCRIPT: &str = r#"(function(){
  if(location.protocol!=='https:'&&location.protocol!=='http:')return '';
  var p=Array.from(document.querySelectorAll('input[type="password"]')).find(function(e){return !e.disabled&&e.offsetParent!==null&&e.value;});
  if(!p)return '';
  var f=p.form||document;
  var u=f.querySelector('input[autocomplete="username"],input[type="email"],input[name="username"],input[name="login"],input[type="text"]');
  return JSON.stringify({origin:location.origin,username:u?u.value.slice(0,2048):'',password:p.value.slice(0,16384)});
})()"#;
