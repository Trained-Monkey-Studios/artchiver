use crate::ux::theme::Theme;
use egui::Color32;
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
    TagsViewGeneral,
    TagsViewAdd,
    TagsViewSubtract,
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
            Self::TagsRefresh => Self::TagsViewGeneral,
            Self::TagsViewGeneral => Self::TagsViewAdd,
            Self::TagsViewAdd => Self::TagsViewSubtract,
            Self::TagsViewSubtract => Self::WorksIntro,
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

    pub fn with_style<R>(
        &self,
        active: bool,
        ui: &mut egui::Ui,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let prior = ui.style().as_ref().clone();
        if active {
            let mut style = self.theme.style_for_tutorial();
            let mixin = Color32::from_rgb(0x00, 0x61, 0xCF).gamma_multiply(1.5);
            style.visuals.widgets.inactive.weak_bg_fill =
                style.visuals.widgets.inactive.weak_bg_fill.blend(mixin);
            style.visuals.override_text_color = Some(Color32::from_rgb(0xFF, 0xFF, 0xFF));
            ui.set_style(style);
        }

        let resp = add_contents(ui);

        if active {
            ui.set_style(prior);
        }

        resp
    }

    pub fn add(
        &mut self,
        active: bool,
        ui: &mut egui::Ui,
        widget: impl egui::Widget,
    ) -> egui::Response {
        let resp = self.with_style(active, ui, |ui| ui.add(widget));
        if active && resp.clicked() {
            self.next();
        }
        resp
    }

    pub fn add_step(
        &mut self,
        active_in_step: TutorialStep,
        ui: &mut egui::Ui,
        widget: impl egui::Widget,
    ) -> egui::Response {
        let fix_spacing = matches!(
            active_in_step,
            TutorialStep::TagsRefresh
                | TutorialStep::TagsViewGeneral
                | TutorialStep::TagsViewAdd
                | TutorialStep::TagsViewSubtract
        );
        let active = *self.step == active_in_step;
        let resp = self.with_style(active, ui, |ui| {
            if fix_spacing {
                ui.style_mut().spacing.item_spacing.x = 0.0;
            }
            ui.add(widget)
        });
        if active && resp.clicked() {
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
        let highlight = self.theme.style_for_tutorial();
        egui::Frame::canvas(&highlight)
            .shadow(egui::Shadow {
                offset: [4, 4],
                spread: 4,
                blur: 25,
                color: Color32::from_rgb(0, 0, 0),
            })
            .outer_margin(12)
            .inner_margin(8)
            .corner_radius(12.0)
            .show(ui, |ui| {
                let prior = ui.style().as_ref().clone();
                ui.set_style(highlight);
                let resp = add_contents(ui, self);
                ui.set_style(prior);
                resp
            })
    }
}
