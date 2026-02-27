// ~/sentinel/sentinel-backend/src/ipc/sysdata/audio.rs

use serde_json::{json, Value};
use std::{
	cell::RefCell,
	collections::VecDeque,
	sync::{
		atomic::{AtomicBool, Ordering},
		OnceLock, RwLock,
	},
	time::Duration,
};
use rustfft::{FftPlanner, num_complex::Complex};
use super::media;
use windows::Win32::{
	Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
	Media::Audio::{
		eCapture, eConsole, eMultimedia, eRender, IMMDevice, IMMDeviceEnumerator,
		MMDeviceEnumerator,
		IAudioClient, IAudioCaptureClient,
		AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
		WAVEFORMATEX,
	},
	Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation},
	System::Com::{
		StructuredStorage::{PropVariantClear, PropVariantToStringAlloc},
		CoCreateInstance, CoInitializeEx, CoTaskMemFree, STGM_READ, CLSCTX_ALL,
		COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED,
	},
};

unsafe fn endpoint_display_name(device: &IMMDevice) -> Option<String> {
	if let Ok(store) = device.OpenPropertyStore(STGM_READ) {
		if let Ok(mut value) = store.GetValue(&PKEY_Device_FriendlyName) {
			if let Ok(wide_name) = PropVariantToStringAlloc(&value) {
				let name = wide_name.to_string().ok().map(|s| s.trim().to_string());
				CoTaskMemFree(Some(wide_name.0 as _));
				let _ = PropVariantClear(&mut value);
				if let Some(display) = name.filter(|s| !s.is_empty()) {
					return Some(display);
				}
			} else {
				let _ = PropVariantClear(&mut value);
			}
		}
	}

	let id = device.GetId().ok()?.to_string().ok()?;
	let trimmed = id.trim().to_string();
	if trimmed.is_empty() { None } else { Some(trimmed) }
}

thread_local! {
	static AUDIO_STATE: RefCell<Option<BackendAudioState>> = const { RefCell::new(None) };
}

/// How many `get_audio_json()` calls between full device re-queries.
/// At 100 ms poll rate this is roughly every 5 seconds.
const REFRESH_EVERY_N_CALLS: u32 = 50;

/// Number of frequency bins sent to the frontend.
const SPECTRUM_BINS: usize = 32;
/// FFT window size (samples). 2048 at 48 kHz ≈ 42.7 ms of audio.
const FFT_SIZE: usize = 2048;

static SPECTRUM_CACHE: OnceLock<RwLock<[f32; SPECTRUM_BINS]>> = OnceLock::new();
static SPECTRUM_STARTED: AtomicBool = AtomicBool::new(false);

fn spectrum_cache() -> &'static RwLock<[f32; SPECTRUM_BINS]> {
	SPECTRUM_CACHE.get_or_init(|| RwLock::new([0.0; SPECTRUM_BINS]))
}

struct BackendAudioState {
	enumerator: IMMDeviceEnumerator,
	output_meter: Option<IAudioMeterInformation>,
	output_volume: Option<IAudioEndpointVolume>,
	input_volume: Option<IAudioEndpointVolume>,
	output_name: String,
	input_name: String,
	peak_ema: f32,
	rms_ema: f32,
	peak_history: VecDeque<f32>,
	calls_since_refresh: u32,
}

impl BackendAudioState {
	fn new() -> Result<Self, String> {
		unsafe {
			let enumerator: IMMDeviceEnumerator =
				CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
					.map_err(|e| format!("CoCreateInstance(MMDeviceEnumerator) failed: {e:?}"))?;

			let mut state = Self {
				enumerator,
				output_meter: None,
				output_volume: None,
				input_volume: None,
				output_name: "default-output".to_string(),
				input_name: "default-input".to_string(),
				peak_ema: 0.0,
				rms_ema: 0.0,
				peak_history: VecDeque::with_capacity(64),
				calls_since_refresh: 0,
			};

			if let Ok(output) = state
				.enumerator
				.GetDefaultAudioEndpoint(eRender, eMultimedia)
				.or_else(|_| state.enumerator.GetDefaultAudioEndpoint(eRender, eConsole))
			{
				if let Some(name) = endpoint_display_name(&output) {
					state.output_name = name;
				}
			}

			if let Ok(input) = state
				.enumerator
				.GetDefaultAudioEndpoint(eCapture, eMultimedia)
				.or_else(|_| state.enumerator.GetDefaultAudioEndpoint(eCapture, eConsole))
			{
				if let Some(name) = endpoint_display_name(&input) {
					state.input_name = name;
				}
			}

			state.refresh();
			Ok(state)
		}
	}

	fn refresh(&mut self) {
		unsafe {
			self.output_meter = None;
			self.output_volume = None;
			self.input_volume = None;

			if let Ok(output) = self
				.enumerator
				.GetDefaultAudioEndpoint(eRender, eMultimedia)
				.or_else(|_| self.enumerator.GetDefaultAudioEndpoint(eRender, eConsole))
			{
				if let Some(name) = endpoint_display_name(&output) {
					self.output_name = name;
				}
				self.output_meter = output.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None).ok();
				self.output_volume = output.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok();
			}

			if let Ok(input) = self
				.enumerator
				.GetDefaultAudioEndpoint(eCapture, eMultimedia)
				.or_else(|_| self.enumerator.GetDefaultAudioEndpoint(eCapture, eConsole))
			{
				if let Some(name) = endpoint_display_name(&input) {
					self.input_name = name;
				}
				self.input_volume = input.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok();
			}
		}
	}
}

pub fn get_audio_json() -> Value {
	unsafe {
		let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
	}
	media::refresh_media_session_cache_if_due();
	start_spectrum_capture_once();

	AUDIO_STATE.with(|cell| {
		const HISTORY_LIMIT: usize = 64;
		const EXPORT_LIMIT: usize = 32;
		const EMA_ALPHA: f32 = 0.35;
		const MIN_DB: f32 = -96.0;

		fn to_db(value: f32) -> f32 {
			if value <= 0.00001 {
				MIN_DB
			} else {
				(20.0 * value.log10()).max(MIN_DB)
			}
		}

		let mut state_opt = cell.borrow_mut();
		if state_opt.is_none() {
			*state_opt = BackendAudioState::new().ok();
		}

		let Some(state) = state_opt.as_mut() else {
			return json!({
				"output_device": {
					"name": "default-output",
					"volume_percent": 0,
					"muted": false,
					"audio_level": 0.0
				},
				"input_device": {
					"name": "default-input",
					"volume_percent": 0,
					"muted": false
				}
			});
		};

		let mut output_volume = 0.0f32;
		let mut output_muted = false;
		let mut output_peak = 0.0f32;
		let mut input_volume = 0.0f32;
		let mut input_muted = false;

		unsafe {
			if let Some(vol) = state.output_volume.as_ref() {
				if let Ok(level) = vol.GetMasterVolumeLevelScalar() {
					output_volume = level.clamp(0.0, 1.0);
				}
				if let Ok(mute) = vol.GetMute() {
					output_muted = mute.as_bool();
				}
			}

			if let Some(meter) = state.output_meter.as_ref() {
				if let Ok(peak) = meter.GetPeakValue() {
					output_peak = peak.clamp(0.0, 1.0);
				}
			}

			if let Some(vol) = state.input_volume.as_ref() {
				if let Ok(level) = vol.GetMasterVolumeLevelScalar() {
					input_volume = level.clamp(0.0, 1.0);
				}
				if let Ok(mute) = vol.GetMute() {
					input_muted = mute.as_bool();
				}
			}
		}

		if state.output_volume.is_none() && state.input_volume.is_none() {
			state.refresh();
		}

		// Periodic full device refresh to detect hot-swapped audio devices
		state.calls_since_refresh += 1;
		if state.calls_since_refresh >= REFRESH_EVERY_N_CALLS {
			state.calls_since_refresh = 0;
			state.refresh();
		}

		state.peak_history.push_back(output_peak);
		while state.peak_history.len() > HISTORY_LIMIT {
			let _ = state.peak_history.pop_front();
		}

		let rms = if state.peak_history.is_empty() {
			0.0
		} else {
			let sum_sq: f32 = state.peak_history.iter().map(|v| v * v).sum();
			(sum_sq / state.peak_history.len() as f32).sqrt().clamp(0.0, 1.0)
		};

		state.peak_ema = (EMA_ALPHA * output_peak + (1.0 - EMA_ALPHA) * state.peak_ema).clamp(0.0, 1.0);
		state.rms_ema = (EMA_ALPHA * rms + (1.0 - EMA_ALPHA) * state.rms_ema).clamp(0.0, 1.0);

		let peak_history: Vec<f32> = state
			.peak_history
			.iter()
			.rev()
			.take(EXPORT_LIMIT)
			.cloned()
			.collect::<Vec<_>>()
			.into_iter()
			.rev()
			.collect();

		json!({
			"output_device": {
				"name": state.output_name.clone(),
				"volume_percent": (output_volume * 100.0).round(),
				"muted": output_muted,
				"audio_level": output_peak,
				"levels": {
					"peak": output_peak,
					"peak_db": to_db(output_peak),
					"rms": rms,
					"rms_db": to_db(rms),
					"smoothed_peak": state.peak_ema,
					"smoothed_peak_db": to_db(state.peak_ema),
					"smoothed_rms": state.rms_ema,
					"smoothed_rms_db": to_db(state.rms_ema)
				},
				"history": {
					"peak_32": peak_history,
					"sample_count": state.peak_history.len()
				}
			},
			"input_device": {
				"name": state.input_name.clone(),
				"volume_percent": (input_volume * 100.0).round(),
				"muted": input_muted,
			},
			"media_session": media::get_media_session_json(),
			"spectrum_32": spectrum_cache().read().map(|s| s.to_vec()).unwrap_or_default(),
		})
	})
}

fn start_spectrum_capture_once() {
	if SPECTRUM_STARTED
		.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
		.is_err()
	{
		return;
	}

	std::thread::Builder::new()
		.name("spectrum-capture".into())
		.spawn(move || {
			if let Err(_e) = spectrum_capture_loop() {
				// On failure, zero out the cache so the frontend sees silence.
				if let Ok(mut bins) = spectrum_cache().write() {
					*bins = [0.0; SPECTRUM_BINS];
				}
			}
		})
		.ok();
}

fn build_band_edges(fft_bins: usize, sample_rate: u32) -> Vec<(usize, usize)> {
	let nyquist = sample_rate as f64 / 2.0;
	let hz_per_bin = nyquist / fft_bins as f64;

	// Map frequencies 20 Hz → nyquist on a log scale into SPECTRUM_BINS bands.
	let lo_hz: f64 = 20.0;
	let hi_hz: f64 = nyquist.min(20_000.0);
	let log_lo = lo_hz.ln();
	let log_hi = hi_hz.ln();
	let mut edges = Vec::with_capacity(SPECTRUM_BINS);
	for i in 0..SPECTRUM_BINS {
		let t0 = i as f64 / SPECTRUM_BINS as f64;
		let t1 = (i + 1) as f64 / SPECTRUM_BINS as f64;
		let f0 = (log_lo + (log_hi - log_lo) * t0).exp();
		let f1 = (log_lo + (log_hi - log_lo) * t1).exp();
		let bin_lo = (f0 / hz_per_bin).floor() as usize;
		let bin_hi = ((f1 / hz_per_bin).ceil() as usize).max(bin_lo + 1).min(fft_bins);
		edges.push((bin_lo, bin_hi));
	}
	edges
}

#[inline]
fn hann(i: usize, n: usize) -> f32 {
	let x = std::f32::consts::PI * 2.0 * i as f32 / (n as f32 - 1.0);
	0.5 * (1.0 - x.cos())
}

fn spectrum_capture_loop() -> Result<(), String> {
	unsafe {
		let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

		let enumerator: IMMDeviceEnumerator =
			CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
				.map_err(|e| format!("enumerator: {e:?}"))?;

		let device = enumerator
			.GetDefaultAudioEndpoint(eRender, eMultimedia)
			.or_else(|_| enumerator.GetDefaultAudioEndpoint(eRender, eConsole))
			.map_err(|e| format!("endpoint: {e:?}"))?;

		let client: IAudioClient = device
			.Activate(CLSCTX_ALL, None)
			.map_err(|e| format!("IAudioClient: {e:?}"))?;

		// Query the device mix format
		let mix_fmt_ptr = client
			.GetMixFormat()
			.map_err(|e| format!("GetMixFormat: {e:?}"))?;
		let mix_fmt: WAVEFORMATEX = *mix_fmt_ptr;
		let sample_rate = mix_fmt.nSamplesPerSec;
		let channels = mix_fmt.nChannels as usize;
		let bits_per_sample = mix_fmt.wBitsPerSample;

		// Initialise in shared loopback mode
		// Buffer duration = 50 ms in 100-ns units
		let buffer_duration: i64 = 500_000; // 50 ms
		client
			.Initialize(
				AUDCLNT_SHAREMODE_SHARED,
				AUDCLNT_STREAMFLAGS_LOOPBACK,
				buffer_duration,
				0,
				mix_fmt_ptr,
				None,
			)
			.map_err(|e| format!("Initialize: {e:?}"))?;

		let capture: IAudioCaptureClient = client
			.GetService()
			.map_err(|e| format!("GetService(capture): {e:?}"))?;

		client.Start().map_err(|e| format!("Start: {e:?}"))?;

		// Pre-compute
		let band_edges = build_band_edges(FFT_SIZE / 2, sample_rate);
		let mut planner = FftPlanner::<f32>::new();
		let fft = planner.plan_fft_forward(FFT_SIZE);
		let mut ring = Vec::<f32>::with_capacity(FFT_SIZE);
		let mut fft_buf: Vec<Complex<f32>> = vec![Complex::new(0.0, 0.0); FFT_SIZE];

		// Smoothed output bins (EMA)
		let mut smooth_bins = [0.0f32; SPECTRUM_BINS];
		const SMOOTH_UP: f32 = 0.55;   // attack — fast enough to see transients
		const SMOOTH_DOWN: f32 = 0.18;  // release — graceful fall-off

		loop {
			// Sleep briefly to let the capture buffer fill
			std::thread::sleep(Duration::from_millis(16));

			// Drain all available packets
			loop {
				let packet_size = match capture.GetNextPacketSize() {
					Ok(s) => s,
					Err(_) => break,
				};
				if packet_size == 0 {
					break;
				}

				let mut data_ptr: *mut u8 = std::ptr::null_mut();
				let mut frames: u32 = 0;
				let mut flags: u32 = 0;
				if capture
					.GetBuffer(
						&mut data_ptr as *mut *mut u8,
						&mut frames as *mut u32,
						&mut flags as *mut u32,
						None,
						None,
					)
					.is_err()
				{
					break;
				}

				let frame_count = frames as usize;
				let silent = (flags & 0x02) != 0; // AUDCLNT_BUFFERFLAGS_SILENT

				if !silent && frame_count > 0 && !data_ptr.is_null() {
					// Interpret samples based on format
					if bits_per_sample == 32 {
						// IEEE float (most common for shared mode)
						let samples = std::slice::from_raw_parts(
							data_ptr as *const f32,
							frame_count * channels,
						);
						// Down-mix to mono and push into ring buffer
						for f in 0..frame_count {
							let mut sum = 0.0f32;
							for c in 0..channels {
								sum += samples[f * channels + c];
							}
							ring.push(sum / channels as f32);
							if ring.len() > FFT_SIZE {
								ring.drain(..ring.len() - FFT_SIZE);
							}
						}
					} else if bits_per_sample == 16 {
						let samples = std::slice::from_raw_parts(
							data_ptr as *const i16,
							frame_count * channels,
						);
						for f in 0..frame_count {
							let mut sum = 0.0f32;
							for c in 0..channels {
								sum += samples[f * channels + c] as f32 / 32768.0;
							}
							ring.push(sum / channels as f32);
							if ring.len() > FFT_SIZE {
								ring.drain(..ring.len() - FFT_SIZE);
							}
						}
					}
					// else: unsupported bit depth — skip silently
				}

				let _ = capture.ReleaseBuffer(frames);
			}

			// Only run FFT when we have a full window
			if ring.len() < FFT_SIZE {
				continue;
			}

			// Apply Hann window and fill FFT buffer
			let offset = ring.len() - FFT_SIZE;
			for i in 0..FFT_SIZE {
				fft_buf[i] = Complex::new(ring[offset + i] * hann(i, FFT_SIZE), 0.0);
			}

			fft.process(&mut fft_buf);

			// Compute magnitude spectrum (first half only — real input is symmetric)
			let half = FFT_SIZE / 2;
			let scale = 2.0 / FFT_SIZE as f32;

			let mut raw_bins = [0.0f32; SPECTRUM_BINS];
			for (band, &(lo, hi)) in band_edges.iter().enumerate() {
				let mut sum = 0.0f32;
				let count = (hi - lo).max(1) as f32;
				for k in lo..hi.min(half) {
					let mag = fft_buf[k].norm() * scale;
					sum += mag;
				}
				raw_bins[band] = sum / count;
			}

			// Convert to dB, normalise to 0..1 range
			const FLOOR_DB: f32 = -80.0;
			const CEIL_DB: f32 = 0.0;
			const RANGE_DB: f32 = CEIL_DB - FLOOR_DB;

			for i in 0..SPECTRUM_BINS {
				let db = if raw_bins[i] > 1e-10 {
					20.0 * raw_bins[i].log10()
				} else {
					FLOOR_DB
				};
				let norm = ((db - FLOOR_DB) / RANGE_DB).clamp(0.0, 1.0);

				// EMA smooth
				if norm > smooth_bins[i] {
					smooth_bins[i] += (norm - smooth_bins[i]) * SMOOTH_UP;
				} else {
					smooth_bins[i] += (norm - smooth_bins[i]) * SMOOTH_DOWN;
				}
			}

			// Write to shared cache
			if let Ok(mut cache) = spectrum_cache().write() {
				*cache = smooth_bins;
			}
		}
	}
}
