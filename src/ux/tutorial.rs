use crate::ux::theme::{ColorTheme, Theme};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum TutorialStep {
    #[default]
    Beginning,
    PluginsIntro,
    PluginsRefresh,
    TagsIntro,
    TagsRefresh,
    WorksIntro,
    WorksSlideshow,
    Finished,
}

impl TutorialStep {
    pub fn next(&self) -> Self {
        match self {
            Self::Beginning => Self::PluginsIntro,
            Self::PluginsIntro => Self::PluginsRefresh,
            Self::PluginsRefresh => Self::TagsIntro,
            Self::TagsIntro => Self::TagsRefresh,
            Self::TagsRefresh => Self::WorksIntro,
            Self::WorksIntro => Self::WorksSlideshow,
            Self::WorksSlideshow => Self::Finished,
            Self::Finished => panic!("Tutorial already finished!"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NextButton {
    Next,
    Skip,
    None,
}

pub struct Tutorial<'a> {
    step: &'a mut TutorialStep,
    theme: &'a Theme,
    style: Arc<egui::Style>,
}

impl<'a> Tutorial<'a> {
    pub fn new<'b>(step: &'b mut TutorialStep, theme: &'b Theme, style: Arc<egui::Style>) -> Self
    where
        'b: 'a,
    {
        Self { step, theme, style }
    }

    pub fn is_plugin_refresh_step(&self, name: &str) -> bool {
        const TUTORIAL_PLUGIN: &str = "The National Gallery of Art";
        *self.step == TutorialStep::PluginsRefresh && name == TUTORIAL_PLUGIN
    }

    pub fn is_tag_refresh_step(&self, name: &str) -> bool {
        const TUTORIAL_TAG: &str = "Cat";
        *self.step == TutorialStep::TagsRefresh && name == TUTORIAL_TAG
    }

    pub fn step(&self) -> TutorialStep {
        *self.step
    }

    pub fn style(&self) -> &egui::Style {
        self.style.as_ref()
    }

    pub fn set_style(&mut self, active: bool, ui: &mut egui::Ui) {
        if active {
            ui.set_style(Theme::new(ColorTheme::SolarizedDark, self.theme.text_scale()).style());
        }
    }

    pub fn reset_style(&mut self, active: bool, ui: &mut egui::Ui) {
        if active {
            ui.set_style(self.theme.style());
        }
    }

    pub fn add(
        &mut self,
        active: bool,
        ui: &mut egui::Ui,
        widget: impl egui::Widget,
    ) -> egui::Response {
        self.set_style(active, ui);
        let resp = ui.add(widget);
        self.reset_style(active, ui);
        if resp.clicked() {
            self.next();
        }
        resp
    }

    pub fn next(&mut self) {
        *self.step = self.step.next();
    }

    pub fn button_area(&mut self, next: NextButton, ui: &mut egui::Ui) {
        ui.separator();
        ui.horizontal(|ui| {
            let text = match next {
                NextButton::Next => "Next",
                NextButton::Skip => "Skip",
                NextButton::None => "",
            };
            match next {
                NextButton::Next | NextButton::Skip => {
                    if ui.button(text).clicked() {
                        self.next();
                    }
                }
                NextButton::None => {}
            }
            if ui.button("Exit Tutorial").clicked() {
                *self.step = TutorialStep::Finished;
            }
        });
    }

    pub fn frame<R>(
        &mut self,
        ui: &mut egui::Ui,
        add_contents: impl FnOnce(&mut egui::Ui, &mut Self) -> R,
    ) -> egui::InnerResponse<R> {
        let highlight = ColorTheme::SolarizedDark.style();
        egui::Frame::canvas(&highlight)
            .shadow(egui::Shadow {
                offset: [4, 4],
                spread: 4,
                blur: 25,
                color: egui::Color32::from_rgb(0, 0, 0),
            })
            .outer_margin(12)
            .inner_margin(8)
            .corner_radius(12.0)
            .show(ui, |ui| {
                ui.style_mut().visuals = highlight.visuals;
                add_contents(ui, self)
            })
    }
}
