use crate::{
    db::{
        model::DbCancellation,
        reader::DbReadHandle,
        sync::{DbSyncHandle, connect_or_create},
        writer::DbWriteHandle,
    },
    plugin::host::PluginHost,
    shared::{environment::Environment, progress::ProgressMonitor},
    ux::dock::UxToplevel,
};
use eframe::glow;

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct ArtchiverApp {
    // Recreate the environment each run as we need to know where we are.
    #[serde(skip)]
    env: Environment,

    #[serde(skip)]
    progress_mon: ProgressMonitor,

    // Reconnect to the database each run
    #[serde(skip)]
    db_sync: DbSyncHandle,
    #[serde(skip)]
    db_write: DbWriteHandle,
    #[serde(skip)]
    db_read: DbReadHandle,
    #[serde(skip)]
    db_cancel: DbCancellation,

    // Rebuild plugins on each run as we don't know where we'll be running from.
    host: PluginHost,

    // The main ux container.
    toplevel: UxToplevel,
}

impl Default for ArtchiverApp {
    fn default() -> Self {
        let pwd = std::env::current_dir().expect("failed to get working directory");
        let env = Environment::new(&pwd).expect("failed to create environment");
        let progress_mon = ProgressMonitor::default();
        let (db_sync, db_write, db_read, db_cancel) =
            connect_or_create(&env, &progress_mon).expect("failed to connect to database");
        let host = PluginHost::default();
        let toplevel = UxToplevel::default();

        Self {
            env,
            progress_mon,
            db_sync,
            db_write,
            db_read,
            db_cancel,
            host,
            toplevel,
        }
    }
}

impl ArtchiverApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Note: we have to set a theme preference here or our style choices get overridden
        //       between here and the first update somehow.
        cc.egui_ctx.set_theme(egui::Theme::from_dark_mode(false));

        // Load or create a new app.
        let mut app: Self = if let Some(storage) = cc.storage {
            let mut app: Self = eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
            app.host
                .initialize(&app.env, &app.progress_mon, &app.db_sync, &app.db_write)
                .expect("failed to initialize app");
            app
        } else {
            Default::default()
        };

        app.toplevel.startup(
            &cc.egui_ctx,
            &app.environment().data_dir(),
            &app.db_read,
            cc,
        );
        app
    }

    pub fn environment(&self) -> &Environment {
        &self.env
    }
}

impl eframe::App for ArtchiverApp {
    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let updates = self.progress_mon.read();
        self.host.handle_updates(&updates);
        self.toplevel.handle_updates(&updates, &self.db_read);

        self.toplevel
            .draw(&self.db_read, &self.db_write, &mut self.host, ctx, frame)
            .expect("ux update error");
    }

    /// Called by the framework to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn on_exit(&mut self, _gl: Option<&glow::Context>) {
        self.host
            .cleanup_for_exit()
            .expect("failed to cleanup plugins on exit");

        // Try to shut down the database cleanly.
        self.db_cancel.cancel();
        self.db_write.send_exit_request();
        self.db_read.wait_for_exit();
    }
}
