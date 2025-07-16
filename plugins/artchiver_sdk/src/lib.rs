use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[macro_export]
macro_rules! import_section {
    () => {
        #[extism_pdk::host_fn]
        extern "ExtismHost" {
            fn progress_spinner();
            fn progress_percent(current: i32, total: i32);
            fn progress_clear();
            fn progress_message(message: &str);
            fn progress_trace(message: &str);
            fn fetch_text(url: &str) -> Json<HttpTextResult>;
            fn fetch_large_text(url: &str) -> Json<HttpTextResult>;
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
            pub fn message<S: AsRef<str>>(msg: S) -> extism_pdk::FnResult<()> {
                Ok(unsafe { progress_message(msg.as_ref()) }?)
            }
            pub fn trace<S: AsRef<str>>(msg: S) -> extism_pdk::FnResult<()> {
                Ok(unsafe { progress_trace(msg.as_ref()) }?)
            }
        }

        pub struct Web;
        impl Web {
            pub fn fetch_text<S: AsRef<str>>(url: S) -> extism_pdk::FnResult<String> {
                match unsafe { fetch_text(url.as_ref()) }?.into_inner() {
                    HttpTextResult::Ok(text) => Ok(text),
                    HttpTextResult::Err {
                        status_code,
                        message,
                    } => {
                        // FIXME: give this a useful type
                        Err(extism_pdk::Error::msg(format!("{status_code}: {message}")).into())
                    }
                }
            }

            pub fn fetch_large_text<S: AsRef<str>>(url: S) -> extism_pdk::FnResult<String> {
                match unsafe { fetch_large_text(url.as_ref()) }?.into_inner() {
                    HttpTextResult::Ok(text) => Ok(text),
                    HttpTextResult::Err {
                        status_code,
                        message,
                    } => {
                        // FIXME: give this a useful type
                        Err(extism_pdk::Error::msg(format!("{status_code}: {message}")).into())
                    }
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum HttpTextResult {
    Ok(String),
    Err { status_code: u16, message: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TagInfo {
    name: String,
    work_count: u64,
}

impl TagInfo {
    pub fn new<S: ToString>(name: S, work_count: u64) -> Self {
        Self {
            name: name.to_string(),
            work_count,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn work_count(&self) -> u64 {
        self.work_count
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Work {
    name: String,
    artist_id: i64,
    date: Date,
    preview_url: String,
    screen_url: String,
    archive_url: Option<String>,
}

impl Work {
    pub fn new<S: ToString>(
        name: S,
        artist_id: i64,
        date: Date,
        preview_url: S,
        screen_url: S,
        archive_url: Option<S>,
    ) -> Self {
        Self {
            name: name.to_string(),
            artist_id,
            date,
            preview_url: preview_url.to_string(),
            screen_url: screen_url.to_string(),
            archive_url: archive_url.map(|s| s.to_string()),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn artist_id(&self) -> i64 {
        self.artist_id
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
}
