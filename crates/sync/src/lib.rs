mod caching_pool;
mod environment;
mod host;
mod model;
mod plugin;
mod progress;
mod shared;

pub use crate::{
    environment::{Environment, EnvironmentPlugin},
    host::PluginHost,
    plugin::get_data_path_for_url,
    progress::Progress,
    shared::{TagSet, TagStatus},
};

use crate::{
    host::{maintain_plugins, startup_plugins},
    model::{MetadataPlugin, MetadataSet},
};
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, SystemSet)]
pub enum SyncSet {
    StartupPlugins,
    MaintainPlugins,
}

pub struct SyncPlugin;
impl Plugin for SyncPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MetadataPlugin)
            .add_systems(
                Startup,
                startup_plugins
                    .in_set(SyncSet::StartupPlugins)
                    .after(MetadataSet::Connect),
            )
            .add_systems(
                FixedUpdate,
                maintain_plugins.in_set(SyncSet::MaintainPlugins),
            );
    }
}
