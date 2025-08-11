use anyhow::Result;
use log::info;
use platform_dirs::AppDirs;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct Environment {
    prefix: PathBuf,
    app_dirs: AppDirs,
}

impl Environment {
    pub fn new(prefix: &Path) -> Result<Self> {
        let env = Self {
            prefix: prefix.to_owned(),
            app_dirs: AppDirs::new(Some("artchiver"), false).expect("Failed to create AppDirs"),
        };

        info!(
            "Global plugin directory: {}",
            env.global_plugin_dir().display()
        );
        fs::create_dir_all(env.global_plugin_dir())?;
        info!(
            "Local plugin directory: {}",
            env.local_plugin_dir().display()
        );
        fs::create_dir_all(env.local_plugin_dir())?;
        info!("Data directory: {}", env.data_dir().display());
        fs::create_dir_all(env.data_dir())?;
        info!("Cache directory: {}", env.cache_dir().display());
        fs::create_dir_all(env.cache_dir())?;
        info!("Temp directory: {}", env.tmp_dir().display());
        fs::create_dir_all(env.tmp_dir())?;

        info!("Clearing temp directory...");
        for entry in fs::read_dir(env.tmp_dir())? {
            let entry = entry?;
            fs::remove_file(entry.path())?;
        }

        Ok(env)
    }

    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    pub fn data_dir(&self) -> PathBuf {
        self.prefix.join("data")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.prefix.join("cache")
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.cache_dir().join("tmp")
    }

    pub fn metadata_file_path(&self) -> PathBuf {
        self.data_dir().join("metadata.db")
    }

    pub fn global_plugin_dir(&self) -> PathBuf {
        self.app_dirs.state_dir.join("plugins")
    }

    pub fn local_plugin_dir(&self) -> PathBuf {
        self.prefix.join("plugins")
    }
}
