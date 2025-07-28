use crate::{
    shared::{environment::Environment, progress::ProgressMonitor},
    sync::{
        db::handle::{DbHandle, DbThreads, connect_or_create},
        plugin::host::PluginHost,
    },
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
    db_handle: DbHandle,
    #[serde(skip)]
    db_threads: DbThreads,

    // Rebuild plugins on each run as we don't know where we'll be running from.
    #[serde(skip)]
    host: PluginHost,

    // The main ux container.
    toplevel: UxToplevel,
}

impl Default for ArtchiverApp {
    fn default() -> Self {
        let pwd = std::env::current_dir().expect("failed to get working directory");
        let env = Environment::new(&pwd).expect("failed to create environment");
        let progress_mon = ProgressMonitor::default();
        let (db_handle, db_threads) =
            connect_or_create(&env, &progress_mon).expect("failed to connect to database");
        let host = PluginHost::new(&env, &progress_mon, db_handle.clone())
            .expect("failed to set up plugins");
        let toplevel = UxToplevel::default();

        Self {
            env,
            progress_mon,
            db_handle,
            db_threads,
            host,
            toplevel,
        }
    }
}

impl ArtchiverApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load or create a new app.
        let mut app: Self = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        };
        app.toplevel
            .startup(&app.environment().data_dir(), &app.db_handle);
        app
    }

    pub fn environment(&self) -> &Environment {
        &self.env
    }
}

impl eframe::App for ArtchiverApp {
    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let updates = self.progress_mon.read();
        self.db_handle.handle_updates(&updates);
        self.host.handle_updates(&updates);
        self.toplevel.handle_updates(&updates, &self.db_handle);

        self.toplevel
            .main(&self.env, &self.db_handle, &mut self.host, ctx)
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
        self.db_handle.send_exit_request();
        self.db_threads.wait_for_exit();
    }
}
