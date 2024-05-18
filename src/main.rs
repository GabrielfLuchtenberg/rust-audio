use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat};
use ffmpeg_next::format::sample::Type as SampleType;
use ffmpeg_next::format::{input, Sample as FFmpegSample};
use ffmpeg_next::frame;
use ffmpeg_next::media::Type;
use ffmpeg_next::software::resampling::context::Context as ResamplingContext;
use ringbuf::storage::Heap;
use ringbuf::traits::{Consumer, Producer};
use ringbuf::traits::{Observer, Split};
use ringbuf::wrap::Wrap;
use ringbuf::{CachingCons, HeapRb, SharedRb};

trait SampleConversion {
    fn as_ffmpeg_sample(&self) -> FFmpegSample;
}
impl SampleConversion for SampleFormat {
    fn as_ffmpeg_sample(&self) -> FFmpegSample {
        match self {
            SampleFormat::I8 => panic!("ffmpeg resampler doesn't support i8"),
            SampleFormat::I16 => FFmpegSample::I16(SampleType::Packed),
            SampleFormat::I32 => FFmpegSample::I32(SampleType::Packed),
            SampleFormat::I64 => FFmpegSample::I64(SampleType::Packed),
            SampleFormat::U8 => FFmpegSample::U8(SampleType::Packed),
            SampleFormat::U16 => panic!("ffmpeg resampler doesn't support u16"),
            SampleFormat::U32 => panic!("ffmpeg resampler doesn't support u32"),
            SampleFormat::U64 => panic!("ffmpeg resampler doesn't support 64"),
            SampleFormat::F32 => FFmpegSample::F32(SampleType::Packed),
            SampleFormat::F64 => FFmpegSample::F64(SampleType::Packed),
            _ => panic!("Unkown sample format"),
        }
    }
}

fn init_cpal() -> (cpal::Device, cpal::SupportedStreamConfig) {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no output device available");
    let mut supported_config_range = device
        .supported_output_configs()
        .expect("error querying config");
    let supported_config = supported_config_range
        .next()
        .expect("no supported config")
        .with_max_sample_rate();

    (device, supported_config)
}

fn write_audio<T: Sample>(
    data: &mut [T],
    samples: &mut CachingCons<Arc<SharedRb<Heap<T>>>>,
    _: &cpal::OutputCallbackInfo,
) {
    for d in data {
        match samples.pop_iter().next() {
            Some(sample) => *d = sample,
            None => (),
        }
    }
}

fn packed<T: frame::audio::Sample>(frame: &frame::Audio) -> &[T] {
    if !frame.is_packed() {
        panic!("data is not packed");
    }

    if !<T as frame::audio::Sample>::is_valid(frame.format(), frame.channels()) {
        panic!("unsupported type");
    }
    unsafe {
        std::slice::from_raw_parts(
            (*frame.as_ptr()).data[0] as *const T,
            frame.samples() * frame.channels() as usize,
        )
    }
}

fn main() {
    ffmpeg_next::init().expect("Failed to init ffmpeg_next");

    let (device, config) = init_cpal();
    let filepath = std::env::args()
        .nth(1)
        .expect("couldn't read filename variable");
    let mut file = input(&filepath).expect("couldn't open file");
    let stream = file
        .streams()
        .best(Type::Audio)
        .ok_or(ffmpeg_next::Error::StreamNotFound)
        .expect("Could not create stream");

    let context = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
        .expect("could not create context");
    let stream_index = stream.index();
    let mut decoder = context
        .decoder()
        .audio()
        .expect("could not create audio decoder");
    let mut resampler = ResamplingContext::get(
        decoder.format(),
        decoder.channel_layout(),
        decoder.rate(),
        config.sample_format().as_ffmpeg_sample(),
        decoder.channel_layout(),
        config.sample_rate().0,
    )
    .expect("failed to create resampler");

    let buffer = HeapRb::<f32>::new(8192);
    let (mut producer, mut consumer) = buffer.split();
    let audio_stream = match config.sample_format() {
        SampleFormat::I8 => panic!("Unimplemented"),
        SampleFormat::I16 => panic!("Unimplemented"),
        SampleFormat::I32 => panic!("Unimplemented"),
        SampleFormat::I64 => panic!("Unimplemented"),
        SampleFormat::U8 => panic!("Unimplemented"),
        SampleFormat::U16 => panic!("Unimplemented"),
        SampleFormat::U32 => panic!("Unimplemented"),
        SampleFormat::U64 => panic!("Unimplemented"),
        SampleFormat::F32 => device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], cbinfo| write_audio(data, &mut consumer, cbinfo),
            |err| eprintln!("error occurred on the audio output stream {}", err),
            None,
        ),
        SampleFormat::F64 => panic!("Unimplemented"),
        _ => panic!("Unimplemented"),
    }
    .expect("Failed to create audio stream");

    let mut receive_and_queue_audio_frames =
        |decoder: &mut ffmpeg_next::decoder::Audio| -> Result<(), ffmpeg_next::Error> {
            let mut decoded = frame::Audio::empty();

            while decoder.receive_frame(&mut decoded).is_ok() {
                let mut resampled = frame::Audio::empty();
                resampler.run(&decoded, &mut resampled)?;

                let both_channels = packed(&resampled);
                while !producer.rb().is_empty() {
                    // std::thread::sleep(std::time::Duration::from_nanos(1));
                }
                producer.push_slice(both_channels);
            }
            Ok(())
        };

    audio_stream.play().expect("Unnable to play");

    for (stream, packet) in file.packets() {
        if stream.index() == stream_index {
            decoder.send_packet(&packet).expect("error to send packet");
            receive_and_queue_audio_frames(&mut decoder)
                .expect("Error to receive and queue audio frame");
        }
    }
}
