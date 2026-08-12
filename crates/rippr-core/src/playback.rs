use crate::PreparedSample;

#[derive(Default)]
pub struct PlaybackEngine {
    active_sample: Option<PreparedSample>,
    playhead: Option<usize>,
    pending_trigger: Option<usize>,
}

impl PlaybackEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn activate(&mut self, sample: PreparedSample) {
        let _ = self.replace(sample);
    }

    pub fn replace(&mut self, sample: PreparedSample) -> Option<PreparedSample> {
        let previous = self.active_sample.replace(sample);
        self.playhead = None;
        previous
    }

    pub fn clear(&mut self) {
        self.active_sample = None;
        self.playhead = None;
        self.pending_trigger = None;
    }

    pub fn trigger_at(&mut self, sample_offset: usize) {
        self.pending_trigger = Some(sample_offset);
    }

    pub fn trigger_now(&mut self) {
        self.pending_trigger = None;
        self.playhead = Some(0);
    }

    pub fn render_frame(&mut self, gain: f32) -> [f32; 2] {
        let (Some(sample), Some(playhead)) = (&self.active_sample, self.playhead) else {
            return [0.0, 0.0];
        };
        if let Some(frame) = sample.frame(playhead) {
            self.playhead = Some(playhead + 1);
            [frame[0] * gain, frame[1] * gain]
        } else {
            self.playhead = None;
            [0.0, 0.0]
        }
    }

    pub fn render(&mut self, output: &mut [[f32; 2]], gain: f32) {
        for (offset, output_frame) in output.iter_mut().enumerate() {
            *output_frame = [0.0, 0.0];
            if self.pending_trigger == Some(offset) {
                self.playhead = Some(0);
                self.pending_trigger = None;
            }
            *output_frame = self.render_frame(gain);
        }

        if let Some(pending) = self.pending_trigger {
            self.pending_trigger = Some(pending.saturating_sub(output.len()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PlaybackEngine;

    #[test]
    fn silence_is_written_without_an_active_sample() {
        let mut engine = PlaybackEngine::new();
        let mut output = [[1.0, 1.0]; 4];
        engine.render(&mut output, 1.0);
        assert_eq!(output, [[0.0, 0.0]; 4]);
    }
}
