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

    pub fn migrate_data_dir(&self) -> Result<()> {
        use std::fs;

        for entry in fs::read_dir(self.data_dir())? {
            let entry = entry?;
            if entry.metadata()?.is_file() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let name = name.as_ref();
                if name == "artchiver.ron" || name.starts_with("metadata") {
                    continue;
                }

                assert!(name.len() > 5, "expect sha shaped data files");
                assert!(name.is_ascii(), "expect ascii data paths");
                let level1 = &name[0..2];
                let level2 = &name[2..4];
                let file_name = &name[4..];
                let dir_path = self.data_dir().join(level1).join(level2);
                let file_path = dir_path.join(file_name);
                fs::create_dir_all(dir_path)?;
                fs::rename(entry.path(), file_path)?;
            }
        }

        Ok(())
    }
}
