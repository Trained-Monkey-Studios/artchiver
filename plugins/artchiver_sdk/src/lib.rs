use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};
use thiserror::Error;

#[macro_export]
macro_rules! import_section {
    () => {
        #[extism_pdk::host_fn]
        extern "ExtismHost" {
            fn progress_spinner();
            fn progress_percent(current: i32, total: i32);
            fn progress_clear();
            fn log_message(level: u32, message: &str);
            fn fetch_text(req: Json<Request>) -> Json<TextResponse>;
        }

        pub struct Progress;
        impl Progress {
            pub fn spinner() -> extism_pdk::FnResult<()> {
                Ok(unsafe { progress_spinner() }?)
            }
            pub fn percent(current: i32, total: i32) -> extism_pdk::FnResult<()> {
                Ok(unsafe { progress_percent(current, total) }?)
            }
            pub fn clear() -> extism_pdk::FnResult<()> {
                Ok(unsafe { progress_clear() }?)
            }
        }

        pub struct Log;
        impl Log {
            pub fn trace<S: AsRef<str>>(msg: S) -> extism_pdk::FnResult<()> {
                Ok(unsafe { log_message(0, msg.as_ref()) }?)
            }

            pub fn debug<S: AsRef<str>>(msg: S) -> extism_pdk::FnResult<()> {
                Ok(unsafe { log_message(1, msg.as_ref()) }?)
            }

            pub fn info<S: AsRef<str>>(msg: S) -> extism_pdk::FnResult<()> {
                Ok(unsafe { log_message(2, msg.as_ref()) }?)
            }

            pub fn warn<S: AsRef<str>>(msg: S) -> extism_pdk::FnResult<()> {
                Ok(unsafe { log_message(3, msg.as_ref()) }?)
            }

            pub fn error<S: AsRef<str>>(msg: S) -> extism_pdk::FnResult<()> {
                Ok(unsafe { log_message(4, msg.as_ref()) }?)
            }
        }

        pub struct Web;
        impl Web {
            pub fn fetch_text(req: Request) -> TextResponse {
                // Unwrap the outer plugin transit error and wrap it back into the inner error
                // so that the caller only has to deal with one layer of errors.
                match unsafe { fetch_text(Json(req)) } {
                    Ok(Json(Ok(text))) => Ok(text),
                    Ok(Json(Err(e))) => Err(e),
                    Err(e) => Err(TextFetchError::HostError(e.to_string())),
                }
            }
        }
    };
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginMetadata {
    name: String,
    version: String,
    description: String,
    rate_limit_n: u32,   // requests per window
    rate_window_ms: u32, // window time in milliseconds
    configurations: Vec<(String, String)>,
}

impl PluginMetadata {
    pub fn new<S: ToString>(name: S, version: S, description: S) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            description: description.to_string(),
            rate_limit_n: 1,
            rate_window_ms: 1,
            configurations: Vec::new(),
        }
    }

    pub fn with_rate_limit(mut self, rate_limit_n: u32, window_sec: f32) -> Self {
        self.rate_limit_n = rate_limit_n;
        self.rate_window_ms = (window_sec * 1000.) as u32;
        self
    }

    pub fn with_configuration(mut self, name: &str) -> Self {
        self.configurations.push((name.to_string(), String::new()));
        self
    }

    pub fn set_config_value(&mut self, key: &str, value: &str) {
        for (k, v) in self.configurations_mut() {
            if key == k {
                *v = value.to_string();
            }
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn rate_limit(&self) -> usize {
        self.rate_limit_n as usize
    }

    pub fn rate_window(&self) -> Duration {
        Duration::from_millis(self.rate_window_ms.into())
    }

    pub fn configurations(&self) -> &[(String, String)] {
        &self.configurations
    }

    pub fn configurations_mut(&mut self) -> impl Iterator<Item = (&str, &mut String)> {
        self.configurations.iter_mut().map(|(k, v)| (k.as_str(), v))
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub enum TagKind {
    #[default]
    Default,
    Artist,
    Character,
    Series,
    Copyright,
    Meta,
    Deprecated,
}

impl FromStr for TagKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "artist" => TagKind::Artist,
            "character" => TagKind::Character,
            "series" => TagKind::Series,
            "copyright" => TagKind::Copyright,
            "meta" => TagKind::Meta,
            _ => TagKind::Default,
        })
    }
}

impl fmt::Display for TagKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TagKind::Artist => write!(f, "artist"),
            TagKind::Character => write!(f, "character"),
            TagKind::Series => write!(f, "series"),
            TagKind::Copyright => write!(f, "copyright"),
            TagKind::Meta => write!(f, "meta"),
            _ => write!(f, "default"),
        }
    }
}

// An API sourced tag
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tag {
    name: String,
    kind: TagKind,
    presumed_work_count: Option<u64>,
    wiki_url: Option<String>,
}

impl PartialEq for Tag {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Tag {}

impl PartialOrd for Tag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.name.partial_cmp(&other.name)
    }
}

impl Ord for Tag {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}

impl Hash for Tag {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl Tag {
    pub fn new<N: ToString, W: ToString>(
        name: N,
        work_count: Option<u64>,
        wiki_url: Option<W>,
    ) -> Self {
        Self {
            name: name.to_string(),
            kind: TagKind::default(),
            presumed_work_count: work_count,
            wiki_url: wiki_url.map(|s| s.to_string()),
        }
    }

    pub fn with_kind(mut self, kind: TagKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn set_work_count(&mut self, count: u64) {
        self.presumed_work_count = Some(count);
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> TagKind {
        self.kind
    }

    pub fn presumed_work_count(&self) -> u64 {
        self.presumed_work_count.unwrap_or_default()
    }

    pub fn wiki_url(&self) -> Option<&str> {
        self.wiki_url.as_deref()
    }
}

// API-centered [art]work item.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Work {
    name: String,
    date: Date,
    preview_url: String,
    screen_url: String,
    tags: Vec<String>,

    remote_id: Option<String>,
    artist_name: Option<String>,
    archive_url: Option<String>,

    preview_path: Option<PathBuf>,
    screen_path: Option<PathBuf>,
    archive_path: Option<PathBuf>,
}

impl Work {
    pub fn new<N: ToString, P: ToString, S: ToString>(
        name: N,
        date: Date,
        preview_url: P,
        screen_url: S,
        tags: Vec<String>,
    ) -> Self {
        Self {
            name: name.to_string(),
            date,
            preview_url: preview_url.to_string(),
            screen_url: screen_url.to_string(),
            tags,

            remote_id: None,
            artist_name: None,
            archive_url: None,
            preview_path: None,
            screen_path: None,
            archive_path: None,
        }
    }

    pub fn with_remote_id(mut self, id: impl ToString) -> Self {
        self.remote_id = Some(id.to_string());
        self
    }

    pub fn with_archive_url(mut self, url: impl ToString) -> Self {
        self.archive_url = Some(url.to_string());
        self
    }

    pub fn remote_id(&self) -> Option<&str> {
        self.remote_id.as_deref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn artist_name(&self) -> Option<&str> {
        self.artist_name.as_deref()
    }

    pub fn date(&self) -> &Date {
        &self.date
    }

    pub fn preview_url(&self) -> &str {
        &self.preview_url
    }

    pub fn screen_url(&self) -> &str {
        &self.screen_url
    }

    pub fn archive_url(&self) -> Option<&str> {
        self.archive_url.as_deref()
    }

    pub fn preview_path(&self) -> Option<&Path> {
        self.preview_path.as_deref()
    }

    pub fn screen_path(&self) -> Option<&Path> {
        self.screen_path.as_deref()
    }

    pub fn archive_path(&self) -> Option<&Path> {
        self.archive_path.as_deref()
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    method: String,
    url: String,
    path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
}

impl Request {
    pub fn get<S: ToString>(url: S) -> Self {
        Self {
            method: "GET".to_string(),
            url: url.to_string(),
            path: "".to_string(),
            query: Vec::new(),
            headers: Vec::new(),
        }
    }

    pub fn add_header<K: ToString, V: ToString>(mut self, key: K, value: V) -> Self {
        self.headers.push((key.to_string(), value.to_string()));
        self
    }

    pub fn add_query<K: ToString, V: ToString>(mut self, key: K, value: V) -> Self {
        self.query.push((key.to_string(), value.to_string()));
        self
    }

    pub fn in_path<P: ToString>(mut self, path: P) -> Self {
        self.path = path.to_string();
        self
    }

    pub fn append_path_segment<P: AsRef<str>>(mut self, path: P) -> Self {
        if !self.path.ends_with('/') {
            self.path.push('/');
        }
        self.path.push_str(path.as_ref());
        self
    }

    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    pub fn to_url(&self) -> String {
        let query = self
            .query
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        if query.is_empty() {
            return format!("{}{}", self.url, self.path);
        }
        format!("{}{}?{}", self.url, self.path, query)
    }
}

pub type TextResponse = Result<String, TextFetchError>;

#[derive(Error, Clone, Debug, Serialize, Deserialize)]
pub enum TextFetchError {
    #[error("timeout")]
    Timeout,
    #[error("io error: {0}")]
    IoError(String),
    #[error("http error: {0}")]
    HttpError(u16),
    #[error("task was cancelled")]
    Cancellation,
    #[error("host error: {0}")]
    HostError(String),
}

impl From<std::io::Error> for TextFetchError {
    fn from(e: std::io::Error) -> Self {
        TextFetchError::IoError(e.to_string())
    }
}

impl From<ureq::Error> for TextFetchError {
    fn from(value: ureq::Error) -> Self {
        match value {
            ureq::Error::StatusCode(code) => TextFetchError::HttpError(code),
            _ => TextFetchError::HostError(value.to_string()),
        }
    }
}
