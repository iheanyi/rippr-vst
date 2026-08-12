use std::{path::Path, sync::Arc};

use crate::RipError;

#[derive(Clone, Debug)]
pub struct PreparedSample {
    frames: Arc<[[f32; 2]]>,
    sample_rate: u32,
}

impl PreparedSample {
    pub fn from_wav(path: &Path, target_sample_rate: u32) -> Result<Self, RipError> {
        let mut reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        if !(1..=2).contains(&spec.channels) {
            return Err(RipError::UnsupportedChannelCount);
        }

        let interleaved = match (spec.sample_format, spec.bits_per_sample) {
            (hound::SampleFormat::Float, 32) => {
                reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?
            }
            (hound::SampleFormat::Int, bits @ 1..=32) => {
                let scale = (1_u64 << (bits - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|sample| sample.map(|sample| sample as f32 / scale))
                    .collect::<Result<Vec<_>, _>>()?
            }
            _ => return Err(RipError::UnsupportedSampleFormat),
        };

        let channels = usize::from(spec.channels);
        let source_frames = interleaved
            .chunks_exact(channels)
            .map(|frame| {
                if channels == 1 {
                    [frame[0], frame[0]]
                } else {
                    [frame[0], frame[1]]
                }
            })
            .collect::<Vec<_>>();
        let frames = resample_linear(&source_frames, spec.sample_rate, target_sample_rate);
        Ok(Self {
            frames: frames.into(),
            sample_rate: target_sample_rate,
        })
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn frame(&self, index: usize) -> Option<[f32; 2]> {
        self.frames.get(index).copied()
    }
}

fn resample_linear(source: &[[f32; 2]], source_rate: u32, target_rate: u32) -> Vec<[f32; 2]> {
    if source.is_empty() || source_rate == target_rate {
        return source.to_vec();
    }
    let target_len =
        ((source.len() as u64 * u64::from(target_rate)).div_ceil(u64::from(source_rate))) as usize;
    let ratio = source_rate as f64 / target_rate as f64;
    (0..target_len)
        .map(|target_index| {
            let position = target_index as f64 * ratio;
            let lower = position.floor() as usize;
            let upper = (lower + 1).min(source.len() - 1);
            let fraction = (position - lower as f64) as f32;
            [
                source[lower][0] + (source[upper][0] - source[lower][0]) * fraction,
                source[lower][1] + (source[upper][1] - source[lower][1]) * fraction,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::resample_linear;

    #[test]
    fn linearly_resamples_stereo_frames() {
        assert_eq!(
            resample_linear(&[[0.0, 0.0], [1.0, -1.0]], 2, 4),
            [[0.0, 0.0], [0.5, -0.5], [1.0, -1.0], [1.0, -1.0]]
        );
    }
}
