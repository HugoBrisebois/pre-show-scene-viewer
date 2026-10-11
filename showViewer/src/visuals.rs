use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use std::f32::consts::TAU;

pub const SPECTRUM_BINS: usize = 513;
const LOOP_SECONDS: f32 = 16.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    Aurora,
    Orbit,
    Kaleidoscope,
}

impl Scene {
    pub const ALL: [Self; 3] = [Self::Aurora, Self::Orbit, Self::Kaleidoscope];

    pub fn label(self) -> &'static str {
        match self {
            Self::Aurora => "Aurora",
            Self::Orbit => "Orbit",
            Self::Kaleidoscope => "Kaleidoscope",
        }
    }
}

pub fn render(painter: &Painter, rect: Rect, spectrum: &[f32], elapsed_seconds: f32, scene: Scene) {
    let phase = elapsed_seconds.rem_euclid(LOOP_SECONDS) / LOOP_SECONDS;
    let mut frame = spectrum_frame(spectrum, phase);
    let peak = frame.iter().copied().fold(0.0_f32, f32::max);
    if peak > 0.0 {
        for value in &mut frame {
            *value = (*value / peak).sqrt().clamp(0.0, 1.0);
        }
    }

    let bass = band_energy(&frame, 1..32);
    let mids = band_energy(&frame, 32..160);
    let highs = band_energy(&frame, 160..SPECTRUM_BINS);
    let energy = (bass * 0.45 + mids * 0.35 + highs * 0.2).clamp(0.0, 1.0);

    painter.rect_filled(rect, 4.0, Color32::from_rgb(7, 10, 24));
    let glow = Color32::from_rgba_unmultiplied(80, 220, 255, (energy * 110.0) as u8);
    painter.circle_filled(rect.center(), rect.width().min(rect.height()) * 0.34, glow);

    match scene {
        Scene::Aurora => draw_aurora(painter, rect, &frame, phase, bass, mids, highs),
        Scene::Orbit => draw_orbit(painter, rect, &frame, phase, bass, mids, highs),
        Scene::Kaleidoscope => draw_kaleidoscope(painter, rect, &frame, phase, bass, mids, highs),
    }
}

fn spectrum_frame(spectrum: &[f32], phase: f32) -> Vec<f32> {
    let frame_count = spectrum.len() / SPECTRUM_BINS;
    if frame_count == 0 {
        return vec![0.0; SPECTRUM_BINS];
    }

    let position = phase * frame_count as f32;
    let first = position.floor() as usize % frame_count;
    let second = (first + 1) % frame_count;
    let blend = position.fract();
    (0..SPECTRUM_BINS)
        .map(|bin| {
            let a = spectrum[first * SPECTRUM_BINS + bin];
            let b = spectrum[second * SPECTRUM_BINS + bin];
            a + (b - a) * blend
        })
        .collect()
}

fn band_energy(frame: &[f32], bins: std::ops::Range<usize>) -> f32 {
    let values = &frame[bins.start.min(frame.len())..bins.end.min(frame.len())];
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f32>() / values.len() as f32
    }
}

fn draw_aurora(
    painter: &Painter,
    rect: Rect,
    frame: &[f32],
    phase: f32,
    bass: f32,
    mids: f32,
    highs: f32,
) {
    let width = rect.width();
    let height = rect.height();
    let colors = [
        Color32::from_rgb(34, 232, 197),
        Color32::from_rgb(99, 120, 255),
        Color32::from_rgb(235, 76, 196),
        Color32::from_rgb(255, 176, 90),
    ];

    for (ribbon, color) in colors.into_iter().enumerate() {
        let mut points = Vec::with_capacity(181);
        let ribbon_phase = phase * TAU + ribbon as f32 * 1.63;
        for step in 0..=180 {
            let x = step as f32 / 180.0;
            let bin = (x * (SPECTRUM_BINS - 1) as f32) as usize;
            let spectral = frame[bin];
            let wave = (x * TAU * (1.4 + mids * 1.4) + ribbon_phase).sin();
            let detail = (x * TAU * 4.0 - ribbon_phase * 1.4).cos();
            let displacement =
                wave * (0.06 + bass * 0.1) + detail * (0.025 + highs * 0.05) + spectral * 0.08;
            let y =
                rect.center().y + (ribbon as f32 - 1.5) * height * 0.105 + displacement * height;
            points.push(Pos2::new(rect.left() + x * width, y));
        }
        painter.add(Shape::line(
            points.clone(),
            Stroke::new(12.0 + bass * 22.0, color.gamma_multiply(0.08)),
        ));
        painter.add(Shape::line(
            points,
            Stroke::new(1.6 + highs * 2.5, color.gamma_multiply(0.85)),
        ));
    }
}

fn draw_orbit(
    painter: &Painter,
    rect: Rect,
    frame: &[f32],
    phase: f32,
    bass: f32,
    mids: f32,
    highs: f32,
) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.2;
    let rotation = phase * TAU;
    let palette = [
        Color32::from_rgb(50, 232, 207),
        Color32::from_rgb(98, 130, 255),
        Color32::from_rgb(245, 91, 195),
    ];

    for ring in 0..4 {
        let ring_radius = radius * (0.72 + ring as f32 * 0.25) + bass * 13.0;
        let color = palette[ring % palette.len()].gamma_multiply(0.22 + ring as f32 * 0.12);
        painter.circle_stroke(center, ring_radius, Stroke::new(1.0, color));
    }

    let spokes = 144;
    for spoke in 0..spokes {
        let angle = spoke as f32 / spokes as f32 * TAU + rotation;
        let bin = spoke * (SPECTRUM_BINS - 1) / spokes;
        let magnitude = frame[bin];
        let low_radius = radius * (0.8 + (angle * 3.0 + rotation).sin() * 0.06);
        let length = 9.0 + magnitude * (30.0 + bass * 40.0) + mids * 7.0;
        let start = Pos2::new(
            center.x + angle.cos() * low_radius,
            center.y + angle.sin() * low_radius,
        );
        let end = Pos2::new(
            center.x + angle.cos() * (low_radius + length),
            center.y + angle.sin() * (low_radius + length),
        );
        let color = palette[spoke % palette.len()].gamma_multiply(0.35 + magnitude * 0.65);
        painter.line_segment([start, end], Stroke::new(1.0 + highs * 1.4, color));
    }

    for orbit in 0..3 {
        let orbit_phase = rotation * (0.5 + orbit as f32 * 0.23) + orbit as f32 * 2.1;
        let orbit_radius = radius * (0.48 + orbit as f32 * 0.2);
        let dot = Pos2::new(
            center.x + orbit_phase.cos() * orbit_radius,
            center.y + orbit_phase.sin() * orbit_radius,
        );
        painter.circle_filled(dot, 2.5 + bass * 4.0, palette[orbit].gamma_multiply(0.8));
    }
}

fn draw_kaleidoscope(
    painter: &Painter,
    rect: Rect,
    frame: &[f32],
    phase: f32,
    bass: f32,
    mids: f32,
    highs: f32,
) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.42;
    let palette = [
        Color32::from_rgb(255, 93, 170),
        Color32::from_rgb(114, 113, 255),
        Color32::from_rgb(50, 231, 209),
        Color32::from_rgb(255, 188, 83),
    ];

    for layer in 0..5 {
        let base_radius = radius * (0.22 + layer as f32 * 0.145);
        let mut points = Vec::with_capacity(241);
        for step in 0..=240 {
            let angle = step as f32 / 240.0 * TAU;
            let spectral_angle = (angle * 6.0 / TAU + phase * 6.0).fract();
            let bin = (spectral_angle * (SPECTRUM_BINS - 1) as f32) as usize;
            let spectral = frame[bin];
            let petals = (angle * 6.0 + phase * TAU).cos();
            let detail = (angle * 12.0 - phase * TAU * 2.0).sin();
            let radial = base_radius
                * (1.0 + petals * (0.07 + mids * 0.08) + detail * (0.025 + highs * 0.04))
                + spectral * (6.0 + bass * 22.0);
            points.push(Pos2::new(
                center.x + angle.cos() * radial,
                center.y + angle.sin() * radial,
            ));
        }
        let color = palette[layer % palette.len()];
        painter.add(Shape::line(
            points,
            Stroke::new(
                1.2 + bass * 1.7,
                color.gamma_multiply(0.4 + layer as f32 * 0.1),
            ),
        ));
    }

    for ray in 0..24 {
        let angle = ray as f32 / 24.0 * TAU + phase * TAU * 0.15;
        let bin = ray * (SPECTRUM_BINS - 1) / 24;
        let length = radius * (0.45 + frame[bin] * (0.3 + bass * 0.35));
        let end = Pos2::new(
            center.x + angle.cos() * length,
            center.y + angle.sin() * length,
        );
        painter.line_segment(
            [center, end],
            Stroke::new(1.0, palette[ray % palette.len()].gamma_multiply(0.22)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{SPECTRUM_BINS, spectrum_frame};

    #[test]
    fn spectrum_frame_interpolates_and_wraps_between_frames() {
        let mut spectrum = vec![1.0; SPECTRUM_BINS];
        spectrum.extend(vec![3.0; SPECTRUM_BINS]);

        assert!(
            spectrum_frame(&spectrum, 0.25)
                .iter()
                .all(|value| (*value - 2.0).abs() < f32::EPSILON)
        );
        assert!(
            spectrum_frame(&spectrum, 0.75)
                .iter()
                .all(|value| (*value - 2.0).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn spectrum_frame_is_silent_without_analyzed_audio() {
        assert!(spectrum_frame(&[], 0.5).iter().all(|value| *value == 0.0));
    }
}
