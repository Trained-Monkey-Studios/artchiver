use egui_plot::{Line, Plot, PlotPoints};
use ringbuffer::{AllocRingBuffer, RingBuffer as _};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct PerfTrack {
    start: Instant,
    perf: BTreeMap<String, AllocRingBuffer<[f64; 2]>>,
}

impl Default for PerfTrack {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            perf: BTreeMap::new(),
        }
    }
}

impl PerfTrack {
    pub fn sample(&mut self, name: &str, elapsed: Duration) {
        self.perf
            .entry(name.to_owned())
            .or_insert_with(|| {
                const NSAMPLES: usize = 128;
                let mut performance = AllocRingBuffer::new(NSAMPLES);
                for _ in 0..NSAMPLES {
                    performance.enqueue([0.; 2]);
                }
                performance
            })
            .enqueue([
                self.start.elapsed().as_secs_f64(),
                elapsed.as_secs_f64() * 1000.0,
            ]);
    }

    pub fn show(&self, ui: &mut egui::Ui) {
        for (name, perf) in &self.perf {
            ui.label(name);
            let line_points: PlotPoints<'_> = perf.iter().copied().collect();
            Plot::new(format!("frame_time_{name}"))
                .height(64.0)
                .show_y(true)
                .allow_zoom(false)
                .allow_drag(false)
                .allow_scroll(false)
                .allow_boxed_zoom(false)
                .include_y(0.)
                .include_y(20.)
                .auto_bounds(egui::Vec2b::new(true, true))
                .show(ui, |plot_ui| {
                    plot_ui.line(Line::new(name, line_points));
                });
        }
    }
}
