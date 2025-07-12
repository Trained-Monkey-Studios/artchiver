use bevy::prelude::*;
use platform_dirs::AppDirs;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct EnvironmentPlugin {
    prefix: PathBuf,
}
impl EnvironmentPlugin {
    pub fn new(prefix: PathBuf) -> Self {
        Self { prefix }
    }
}

#[derive(Debug, Resource)]
pub struct Environment {
    prefix: PathBuf,
    app_dirs: AppDirs,
}
impl Environment {
    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    pub fn data_dir(&self) -> PathBuf {
        self.prefix.join("data")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.prefix.join("cache")
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

impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        let prefix = self.prefix.clone();
        app.add_systems(PreStartup, move |world: &mut World| -> Result {
            let env = Environment {
                prefix: prefix.clone(),
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

            world.insert_resource(env);
            Ok(())
        });
    }
}
