use crate::{
    shared::environment::Environment,
    sync::{db::model::MetadataPool, plugin::host::PluginHost},
    ux::dock::UxToplevel,
};
use eframe::glow;

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct ArtchiverApp {
    // The main ux container.
    toplevel: UxToplevel,

    // Recreate the environment each run as we need to know where we are.
    #[serde(skip)]
    env: Environment,

    // Rebuild plugins on each run as we don't know where we'll be running from.
    #[serde(skip)]
    host: PluginHost,
}

impl Default for ArtchiverApp {
    fn default() -> Self {
        let pwd = std::env::current_dir().expect("failed to get working directory");
        let env = Environment::new(&pwd).expect("failed to create environment");
        let pool = MetadataPool::connect_or_create(&env).expect("failed to connect to database");
        let host = PluginHost::new(pool, &env).expect("failed to set up plugins");
        Self {
            toplevel: UxToplevel::default(),
            env,
            host,
        }
    }
}

impl ArtchiverApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        }
    }

    pub fn environment(&self) -> &Environment {
        &self.env
    }

    pub fn host(&self) -> &PluginHost {
        &self.host
    }
}

impl eframe::App for ArtchiverApp {
    /// Called by the framework to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.host.maintain_plugins();
        self.toplevel
            .main(&self.env, &mut self.host, ctx)
            .expect("ux update error");

        // Put your widgets into a `SidePanel`, `TopBottomPanel`, `CentralPanel`, `Window` or `Area`.
        // For inspiration and more examples, go to https://emilk.github.io/egui

        /*
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.add_space(16.0);

                // egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("todo");
        });
         */
    }

    fn on_exit(&mut self, _gl: Option<&glow::Context>) {
        self.host
            .cleanup_for_exit()
            .expect("failed to cleanup plugins on exit");
    }
}
