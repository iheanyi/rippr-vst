use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};

use crate::RipError;

/// In-memory preview is deliberately bounded so an unusually large source can
/// still be acquired and dragged without putting the DAW process under memory pressure.
const MAX_PREVIEW_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct PreparedSample {
    frames: Arc<[[f32; 2]]>,
    sample_rate: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WavAnalysis {
    pub sample_rate: u32,
    pub frame_count: usize,
    pub waveform_peaks: Vec<[f32; 2]>,
}

impl PreparedSample {
    pub fn is_previewable(path: &Path) -> bool {
        std::fs::metadata(path).is_ok_and(|metadata| metadata.len() <= MAX_PREVIEW_BYTES)
    }

    pub fn from_wav(path: &Path, target_sample_rate: u32) -> Result<Self, RipError> {
        if std::fs::metadata(path)?.len() > MAX_PREVIEW_BYTES {
            return Err(RipError::PreviewTooLarge);
        }
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

    /// Returns an actual min/max peak envelope of the decoded stereo audio.
    /// Each bucket covers a proportional, non-overlapping range of frames.
    pub fn waveform_peaks(&self, bucket_count: usize) -> Vec<[f32; 2]> {
        let bucket_count = bucket_count.min(self.frames.len());
        if bucket_count == 0 {
            return Vec::new();
        }

        (0..bucket_count)
            .map(|bucket| {
                let start = bucket * self.frames.len() / bucket_count;
                let end = ((bucket + 1) * self.frames.len() / bucket_count).max(start + 1);
                self.frames[start..end]
                    .iter()
                    .flat_map(|frame| frame.iter().copied())
                    .fold([f32::INFINITY, f32::NEG_INFINITY], |[min, max], sample| {
                        [min.min(sample), max.max(sample)]
                    })
            })
            .collect()
    }

    pub(crate) fn frame(&self, index: usize) -> Option<[f32; 2]> {
        self.frames.get(index).copied()
    }
}

pub fn waveform_peaks_from_wav(
    path: &Path,
    bucket_count: usize,
) -> Result<Vec<[f32; 2]>, RipError> {
    Ok(analyze_wav(path, bucket_count)?.waveform_peaks)
}

/// Streams PCM samples directly from RIFF or RF64 WAV data. This keeps waveform
/// analysis bounded even when the handoff file is far too large to preview in RAM.
pub fn analyze_wav(path: &Path, bucket_count: usize) -> Result<WavAnalysis, RipError> {
    #[derive(Clone, Copy)]
    struct Format {
        code: u16,
        channels: u16,
        sample_rate: u32,
        block_align: u16,
        bits_per_sample: u16,
    }

    fn invalid_wav(message: &'static str) -> RipError {
        std::io::Error::new(std::io::ErrorKind::InvalidData, message).into()
    }

    let mut reader = BufReader::new(File::open(path)?);
    let mut header = [0_u8; 12];
    reader.read_exact(&mut header)?;
    if (&header[0..4] != b"RIFF" && &header[0..4] != b"RF64") || &header[8..12] != b"WAVE" {
        return Err(invalid_wav("the prepared file is not a RIFF/RF64 WAV"));
    }

    let mut format = None;
    let mut rf64_data_size = None;
    let (data_offset, data_size) = loop {
        let mut chunk_header = [0_u8; 8];
        reader.read_exact(&mut chunk_header)?;
        let chunk_id = &chunk_header[0..4];
        let chunk_size = u32::from_le_bytes(chunk_header[4..8].try_into().unwrap()) as u64;
        let chunk_start = reader.stream_position()?;

        match chunk_id {
            b"ds64" => {
                if chunk_size < 24 {
                    return Err(invalid_wav("the RF64 size chunk is truncated"));
                }
                let mut sizes = [0_u8; 24];
                reader.read_exact(&mut sizes)?;
                rf64_data_size = Some(u64::from_le_bytes(sizes[8..16].try_into().unwrap()));
            }
            b"fmt " => {
                if chunk_size < 16 {
                    return Err(invalid_wav("the WAV format chunk is truncated"));
                }
                let read_len = usize::try_from(chunk_size.min(64)).unwrap();
                let mut bytes = vec![0_u8; read_len];
                reader.read_exact(&mut bytes)?;
                let mut code = u16::from_le_bytes(bytes[0..2].try_into().unwrap());
                if code == 0xfffe && bytes.len() >= 26 {
                    code = u16::from_le_bytes(bytes[24..26].try_into().unwrap());
                }
                format = Some(Format {
                    code,
                    channels: u16::from_le_bytes(bytes[2..4].try_into().unwrap()),
                    sample_rate: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
                    block_align: u16::from_le_bytes(bytes[12..14].try_into().unwrap()),
                    bits_per_sample: u16::from_le_bytes(bytes[14..16].try_into().unwrap()),
                });
            }
            b"data" => {
                let data_size = if chunk_size == u64::from(u32::MAX) {
                    rf64_data_size.ok_or_else(|| invalid_wav("RF64 data size is missing"))?
                } else {
                    chunk_size
                };
                break (chunk_start, data_size);
            }
            _ => {}
        }

        let next = chunk_start
            .checked_add(chunk_size)
            .and_then(|position| position.checked_add(chunk_size % 2))
            .ok_or_else(|| invalid_wav("the WAV chunk table overflows"))?;
        reader.seek(SeekFrom::Start(next))?;
    };

    let format = format.ok_or_else(|| invalid_wav("the WAV format chunk is missing"))?;
    if !(1..=2).contains(&format.channels) {
        return Err(RipError::UnsupportedChannelCount);
    }
    if format.sample_rate == 0 || format.block_align == 0 {
        return Err(invalid_wav("the WAV format values are invalid"));
    }
    let bytes_per_sample = usize::from(format.bits_per_sample.div_ceil(8));
    if bytes_per_sample == 0
        || usize::from(format.block_align) < bytes_per_sample * usize::from(format.channels)
        || !matches!(
            (format.code, format.bits_per_sample),
            (1, 8 | 16 | 24 | 32) | (3, 32)
        )
    {
        return Err(RipError::UnsupportedSampleFormat);
    }

    let frame_count_u64 = data_size / u64::from(format.block_align);
    let frame_count = usize::try_from(frame_count_u64)
        .map_err(|_| invalid_wav("the WAV contains too many frames for this platform"))?;
    let bucket_count = bucket_count.min(frame_count);
    let mut peaks = vec![[f32::INFINITY, f32::NEG_INFINITY]; bucket_count];
    if frame_count > 0 && bucket_count > 0 {
        reader.seek(SeekFrom::Start(data_offset))?;
        let block_align = usize::from(format.block_align);
        let buffer_len = ((1024 * 1024) / block_align).max(1) * block_align;
        let mut buffer = vec![0_u8; buffer_len];
        let mut remaining = data_size;
        let mut frame_index = 0_u64;

        while remaining > 0 {
            let bytes_to_read = usize::try_from(remaining.min(buffer.len() as u64)).unwrap();
            reader.read_exact(&mut buffer[..bytes_to_read])?;
            for frame in buffer[..bytes_to_read].chunks_exact(block_align) {
                let bucket = ((u128::from(frame_index) * bucket_count as u128)
                    / u128::from(frame_count_u64)) as usize;
                for channel in 0..usize::from(format.channels) {
                    let offset = channel * bytes_per_sample;
                    let bytes = &frame[offset..offset + bytes_per_sample];
                    let sample = match (format.code, format.bits_per_sample) {
                        (1, 8) => (f32::from(bytes[0]) - 128.0) / 128.0,
                        (1, 16) => {
                            f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32_768.0
                        }
                        (1, 24) => {
                            let raw = i32::from_le_bytes([
                                bytes[0],
                                bytes[1],
                                bytes[2],
                                if bytes[2] & 0x80 == 0 { 0 } else { 0xff },
                            ]);
                            raw as f32 / 8_388_608.0
                        }
                        (1, 32) => {
                            i32::from_le_bytes(bytes.try_into().unwrap()) as f32 / 2_147_483_648.0
                        }
                        (3, 32) => f32::from_le_bytes(bytes.try_into().unwrap()),
                        _ => unreachable!("the sample representation was validated"),
                    };
                    peaks[bucket][0] = peaks[bucket][0].min(sample);
                    peaks[bucket][1] = peaks[bucket][1].max(sample);
                }
                frame_index += 1;
            }
            remaining -= bytes_to_read as u64;
        }
    }

    if bucket_count == 0 {
        peaks.clear();
    }
    for peak in &mut peaks {
        if !peak[0].is_finite() || !peak[1].is_finite() {
            *peak = [0.0, 0.0];
        }
    }
    Ok(WavAnalysis {
        sample_rate: format.sample_rate,
        frame_count,
        waveform_peaks: peaks,
    })
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
    use std::{fs, sync::Arc};

    use super::{PreparedSample, analyze_wav, resample_linear, waveform_peaks_from_wav};

    #[test]
    fn linearly_resamples_stereo_frames() {
        assert_eq!(
            resample_linear(&[[0.0, 0.0], [1.0, -1.0]], 2, 4),
            [[0.0, 0.0], [0.5, -0.5], [1.0, -1.0], [1.0, -1.0]]
        );
    }

    #[test]
    fn waveform_is_derived_from_real_sample_extrema() {
        let sample = PreparedSample {
            frames: Arc::from([[0.25, -0.5], [0.75, -0.25], [-1.0, 0.5], [0.125, 0.25]]),
            sample_rate: 48_000,
        };

        assert_eq!(sample.waveform_peaks(2), [[-0.5, 0.75], [-1.0, 0.5]]);
    }

    #[test]
    fn waveform_can_be_analyzed_from_disk_without_preloading_the_sample() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("waveform.wav");
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 48_000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for sample in [0.25_f32, -0.5, 0.75, -0.25, -1.0, 0.5, 0.125, 0.25] {
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();

        assert_eq!(
            waveform_peaks_from_wav(&path, 2).unwrap(),
            [[-0.5, 0.75], [-1.0, 0.5]]
        );
        let analysis = analyze_wav(&path, 2).unwrap();
        assert_eq!(analysis.sample_rate, 48_000);
        assert_eq!(analysis.frame_count, 4);
    }

    #[test]
    fn rf64_waveforms_are_streamed_without_a_classic_wav_size_ceiling() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("large-compatible.rf64.wav");
        let samples = [
            8_192_i16, -16_384, 24_576, -8_192, -32_768, 16_384, 4_096, 8_192,
        ];
        let data_size = (samples.len() * 2) as u64;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RF64");
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"ds64");
        bytes.extend_from_slice(&28_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u64.to_le_bytes());
        bytes.extend_from_slice(&data_size.to_le_bytes());
        bytes.extend_from_slice(&4_u64.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&192_000_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        fs::write(&path, bytes).unwrap();

        let analysis = analyze_wav(&path, 2).unwrap();
        assert_eq!(analysis.frame_count, 4);
        assert_eq!(analysis.waveform_peaks, [[-0.5, 0.75], [-1.0, 0.5]]);
    }
}
