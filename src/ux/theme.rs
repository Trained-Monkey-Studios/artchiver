use eframe::epaint::Color32;
use egui_aesthetix::Aesthetix as _;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ColorTheme {
    Light,
    Dark,
    NordicLight,
    NordicDark,
    TokioNight,
    TokioNightStorm,
    CatppuccinLatte,
    #[default]
    CatppuccinFrappe,
    CatppuccinMacchiato,
    CatppuccinMocha,
    SolarizedLight,
    SolarizedDark,
    TutorialLight,
    TutorialDark,
}

impl ColorTheme {
    pub fn ui(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let mut selected = match self {
            Self::Light => 0,
            Self::Dark => 1,
            Self::NordicLight => 2,
            Self::NordicDark => 3,
            Self::TokioNight => 4,
            Self::TokioNightStorm => 5,
            Self::CatppuccinLatte => 6,
            Self::CatppuccinFrappe => 7,
            Self::CatppuccinMacchiato => 8,
            Self::CatppuccinMocha => 9,
            Self::SolarizedLight => 10,
            Self::SolarizedDark => 11,
            _ => panic!("tutorial themes cannot be selected"),
        };
        let labels = [
            "Egui Light",
            "Egui Dark",
            "Nordic Light",
            "Nordic Dark",
            "Tokio Night",
            "Tokio Night (Storm)",
            "Catppuccin Latte",
            "Catppuccin Frappe",
            "Catppuccin Macchiato",
            "Catppuccin Mocha",
            "Solarized Light",
            "Solarized Dark",
        ];
        let resp = egui::ComboBox::new("color_theme_selection_dropdown", "")
            .wrap_mode(egui::TextWrapMode::Extend)
            .show_index(ui, &mut selected, labels.len(), |i| labels[i]);
        *self = match selected {
            0 => Self::Light,
            1 => Self::Dark,
            2 => Self::NordicLight,
            3 => Self::NordicDark,
            4 => Self::TokioNight,
            5 => Self::TokioNightStorm,
            6 => Self::CatppuccinLatte,
            7 => Self::CatppuccinFrappe,
            8 => Self::CatppuccinMacchiato,
            9 => Self::CatppuccinMocha,
            10 => Self::SolarizedLight,
            11 => Self::SolarizedDark,
            _ => panic!("invalid column selected"),
        };
        resp
    }

    pub fn style(&self) -> egui::style::Style {
        match self {
            Self::Light => egui::style::Style {
                visuals: egui::Visuals::light(),
                ..Default::default()
            },
            Self::Dark => egui::style::Style {
                visuals: egui::Visuals::dark(),
                ..Default::default()
            },
            Self::NordicLight => {
                Self::tweak_aesthetix(egui_aesthetix::themes::NordLight.custom_style())
            }
            Self::NordicDark => {
                Self::tweak_aesthetix(egui_aesthetix::themes::NordDark.custom_style())
            }
            Self::TokioNight => {
                Self::tweak_aesthetix(egui_aesthetix::themes::TokyoNight.custom_style())
            }
            Self::TokioNightStorm => {
                Self::tweak_aesthetix(egui_aesthetix::themes::TokyoNightStorm.custom_style())
            }
            Self::CatppuccinLatte => {
                let mut style = egui::style::Style::default();
                catppuccin_egui::set_style_theme(&mut style, catppuccin_egui::LATTE);
                Self::tweak_catppuccin(&mut style);
                style
            }
            Self::CatppuccinFrappe => {
                let mut style = egui::style::Style::default();
                catppuccin_egui::set_style_theme(&mut style, catppuccin_egui::FRAPPE);
                Self::tweak_catppuccin(&mut style);
                style
            }
            Self::CatppuccinMacchiato => {
                let mut style = egui::style::Style::default();
                catppuccin_egui::set_style_theme(&mut style, catppuccin_egui::MACCHIATO);
                Self::tweak_catppuccin(&mut style);
                style
            }
            Self::CatppuccinMocha => {
                let mut style = egui::style::Style::default();
                catppuccin_egui::set_style_theme(&mut style, catppuccin_egui::MOCHA);
                Self::tweak_catppuccin(&mut style);
                style
            }
            Self::SolarizedLight => egui::style::Style {
                visuals: egui_solarized::Theme::solarized_light().into(),
                ..Default::default()
            },
            Self::SolarizedDark => egui::style::Style {
                visuals: egui_solarized::Theme::solarized_dark().into(),
                ..Default::default()
            },
            Self::TutorialLight => Self::SolarizedLight.style(),
            Self::TutorialDark => Self::SolarizedDark.style(),
        }
    }

    pub fn apply(&self, ctx: &egui::Context) {
        ctx.set_style(self.style());
    }

    fn tweak_aesthetix(mut style: egui::style::Style) -> egui::style::Style {
        style.spacing = Theme::spacing_style();
        let alpha = if style.visuals.dark_mode { 0x60 } else { 0x19 };
        style.visuals.window_shadow = egui::Shadow {
            offset: [10, 20],
            blur: 15,
            spread: 0,
            color: Color32::from_black_alpha(alpha),
        };
        style.visuals.popup_shadow = egui::Shadow {
            offset: [10, 20],
            blur: 15,
            spread: 0,
            color: Color32::from_black_alpha(alpha),
        };
        style
    }

    fn tweak_catppuccin(style: &mut egui::style::Style) {
        style.visuals.selection.bg_fill = style.visuals.selection.bg_fill.gamma_multiply(2.0);
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Theme {
    color: ColorTheme,
    text_scale: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            color: ColorTheme::default(),
            text_scale: 125.0,
        }
    }
}

impl Theme {
    pub fn new(color: ColorTheme, text_scale: f32) -> Self {
        Self { color, text_scale }
    }

    pub fn style_for_tutorial(&self) -> egui::Style {
        let current_is_dark_mode = self.color.style().visuals.dark_mode;
        let color = if current_is_dark_mode {
            ColorTheme::TutorialDark
        } else {
            ColorTheme::TutorialLight
        };
        Self::new(color, self.text_scale).style()
    }

    pub fn text_scale(&self) -> f32 {
        self.text_scale
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        egui::Grid::new("theme_selection_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Theme");
                if self.color.ui(ui).changed() {
                    changed = true;
                }
                ui.end_row();

                ui.label("Text Scale");
                let val = egui::DragValue::new(&mut self.text_scale)
                    .clamp_existing_to_range(true)
                    .range(75.0..=200.0)
                    .fixed_decimals(0)
                    .suffix("%")
                    .speed(25.0);
                if ui.add(val).changed() {
                    changed = true;
                }
                ui.end_row();
            });

        if changed {
            self.apply(ui.ctx());
        }
    }

    pub fn style(&self) -> egui::style::Style {
        let mut style = self.color.style();

        use egui::FontFamily::{Monospace as FFMonospace, Proportional};
        use egui::FontId;
        use egui::TextStyle as TS;
        let s = self.text_scale / 100.0;
        let text_styles: BTreeMap<_, _> = [
            (
                TS::Name("Title".into()),
                FontId::new(18.0 * s, Proportional),
            ),
            (TS::Heading, FontId::new(15.0 * s, Proportional)),
            (TS::Body, FontId::new(12.5 * s, Proportional)),
            (TS::Monospace, FontId::new(12.0 * s, FFMonospace)),
            (TS::Button, FontId::new(12.5 * s, Proportional)),
            (TS::Small, FontId::new(9.0 * s, Proportional)),
        ]
        .into();
        style.text_styles = text_styles.clone();

        style
    }

    pub fn apply(&self, ctx: &egui::Context) {
        ctx.set_style(self.style());
    }

    fn spacing_style() -> egui::style::Spacing {
        egui::style::Spacing {
            item_spacing: [8.0, 3.0].into(),
            window_margin: 6i8.into(),
            button_padding: [4.0, 1.0].into(),
            menu_margin: 6i8.into(),
            indent: 18.0,
            interact_size: [40.0, 18.0].into(),
            slider_width: 100.0,
            slider_rail_height: 8.0,
            combo_width: 100.0,
            text_edit_width: 280.0,
            icon_width: 14.0,
            icon_width_inner: 8.0,
            icon_spacing: 4.0,
            default_area_size: [600.0, 400.0].into(),
            tooltip_width: 500.0,
            menu_width: 400.0,
            menu_spacing: 2.0,
            indent_ends_with_horizontal_line: false,
            combo_height: 200.0,
            scroll: egui::style::ScrollStyle {
                floating: true,
                bar_width: 10.0,
                handle_min_length: 12.0,
                bar_inner_margin: 4.0,
                bar_outer_margin: 0.0,
                floating_width: 2.0,
                floating_allocated_width: 0.0,
                foreground_color: true,
                dormant_background_opacity: 0.0,
                active_background_opacity: 0.4,
                interact_background_opacity: 0.7,
                dormant_handle_opacity: 0.0,
                active_handle_opacity: 0.6,
                interact_handle_opacity: 1.0,
            },
        }
    }
}

pub fn rgb(v: u32) -> Color32 {
    assert!(v <= 0xFFFFFF, "too big for rgb value");
    Color32::from_rgba_unmultiplied(
        ((v >> 16) & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        (v & 0xFF) as u8,
        255,
    )
}

pub fn rgba(v: u32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        ((v >> 24) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        (v & 0xFF) as u8,
    )
}
