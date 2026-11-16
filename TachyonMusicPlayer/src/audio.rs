//! Output: the music player feeding the default output device through `cpal`. If there's no
//! device, `Output::new` returns `None` and the app says so.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::sync::{Arc, Mutex};
use tachyon_core::music::Player;

/// The player the output stream pulls from (`None`: silence).
pub type Slot = Arc<Mutex<Option<Player>>>;

pub struct Output {
    pub slot: Slot,
    _stream: cpal::Stream,
}

/// An output stream of sample type `T`: stereo goes to the first two channels (a mono device
/// gets the average), any others are silent.
fn build<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: &cpal::StreamConfig, slot: Slot) -> Option<cpal::Stream> {
    let (ch, rate) = (config.channels as usize, config.sample_rate.0);
    let mut acc = vec![];
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _| {
                acc.clear();
                acc.resize(out.len() / ch * 2, 0.0);
                if let Ok(mut p) = slot.lock() {
                    if let Some(p) = p.as_mut() {
                        p.fill(&mut acc, rate);
                    }
                }
                for (f, o) in out.chunks_mut(ch).enumerate() {
                    let (l, r) = (acc[2 * f].clamp(-1.0, 1.0), acc[2 * f + 1].clamp(-1.0, 1.0));
                    for (c, x) in o.iter_mut().enumerate() {
                        *x = T::from_sample(match (ch, c) {
                            (1, _) => (l + r) * 0.5,
                            (_, 0) => l,
                            (_, 1) => r,
                            _ => 0.0,
                        });
                    }
                }
            },
            |e| eprintln!("audio: {e}"),
            None,
        )
        .ok()
}

impl Output {
    pub fn new() -> Option<Output> {
        let device = cpal::default_host().default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let slot: Slot = Arc::default();
        let s = slot.clone();
        use cpal::SampleFormat as F;
        let stream = match format {
            F::F32 => build::<f32>(&device, &config, s),
            F::I16 => build::<i16>(&device, &config, s),
            F::U16 => build::<u16>(&device, &config, s),
            F::I32 => build::<i32>(&device, &config, s),
            F::F64 => build::<f64>(&device, &config, s),
            F::U8 => build::<u8>(&device, &config, s),
            _ => None,
        }?;
        stream.play().ok()?;
        Some(Output { slot, _stream: stream })
    }
}
