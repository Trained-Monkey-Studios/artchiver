use bevy::{
    diagnostic::FrameTimeDiagnosticsPlugin,
    ecs::schedule::{LogLevel, ScheduleBuildSettings},
    prelude::*,
    window::{Window, WindowMode, WindowResolution},
};
use bevy_egui::{EguiContexts, EguiPlugin};
use clap::Parser;
use std::{env, path::PathBuf};
use sync::{EnvironmentPlugin, SyncPlugin};
use ux::UxPlugin;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// The prefix to use for storage.
    #[arg(short, long)]
    prefix: Option<String>,

    /// Turn on system ordering debugging.
    #[arg(short, long)]
    debug_order: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let prefix = cli
        .prefix
        .map(|s| {
            let mut p = PathBuf::new();
            p.push(s);
            p
        })
        .or_else(|| env::current_dir().ok())
        .expect("a prefix path");

    let mut app = App::new();
    if cli.debug_order {
        app.edit_schedule(Startup, |schedule| {
            schedule.set_build_settings(ScheduleBuildSettings {
                ambiguity_detection: LogLevel::Warn,
                ..default()
            });
        })
        .edit_schedule(PreUpdate, |schedule| {
            schedule.set_build_settings(ScheduleBuildSettings {
                ambiguity_detection: LogLevel::Warn,
                ..default()
            });
        })
        .edit_schedule(Update, |schedule| {
            schedule.set_build_settings(ScheduleBuildSettings {
                ambiguity_detection: LogLevel::Warn,
                ..default()
            });
        })
        .edit_schedule(PostUpdate, |schedule| {
            schedule.set_build_settings(ScheduleBuildSettings {
                ambiguity_detection: LogLevel::Warn,
                ..default()
            });
        })
        .edit_schedule(FixedPreUpdate, |schedule| {
            schedule.set_build_settings(ScheduleBuildSettings {
                ambiguity_detection: LogLevel::Warn,
                ..default()
            });
        })
        .edit_schedule(FixedUpdate, |schedule| {
            schedule.set_build_settings(ScheduleBuildSettings {
                ambiguity_detection: LogLevel::Warn,
                ..default()
            });
        })
        .edit_schedule(FixedPostUpdate, |schedule| {
            schedule.set_build_settings(ScheduleBuildSettings {
                ambiguity_detection: LogLevel::Warn,
                ..default()
            });
        });
    }
    app.add_plugins((
        (
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    mode: WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                    resolution: WindowResolution::new(1920., 1080.),
                    ..default()
                }),
                ..default()
            }),
            // .set(AssetPlugin {
            //     file_path: "assets/art".into(),
            //     processed_file_path: "pkg/art".into(),
            //     ..default()
            // }),
            FrameTimeDiagnosticsPlugin::default(),
            bevy_framepace::FramepacePlugin, // reduces input lag
            EguiPlugin::default(),
        ),
        (EnvironmentPlugin::new(prefix), SyncPlugin, UxPlugin),
    ))
    .add_systems(Startup, do_egui_setup);

    app.world_mut().spawn((Name::new("Camera"), Camera2d));

    app.run();
    Ok(())
}

// fn do_camera_setup(mut commands: Commands) {
//     commands.spawn((Name::new("Camera"), Camera2d));
// }

fn do_egui_setup(mut contexts: EguiContexts) -> Result {
    egui_extras::install_image_loaders(contexts.ctx_mut()?);
    Ok(())
}
