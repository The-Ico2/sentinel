// ~/sentinel/sentinel-backend/src/ipc/sysdata/audio.rs

use serde_json::{json, Value};
use std::{cell::RefCell, collections::VecDeque};
use windows::Win32::{
	Media::Audio::{
		eCapture, eConsole, eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
	},
	Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation},
	System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED},
};

thread_local! {
	static AUDIO_STATE: RefCell<Option<BackendAudioState>> = const { RefCell::new(None) };
}

struct BackendAudioState {
	enumerator: IMMDeviceEnumerator,
	output_meter: Option<IAudioMeterInformation>,
	output_volume: Option<IAudioEndpointVolume>,
	input_volume: Option<IAudioEndpointVolume>,
	peak_ema: f32,
	rms_ema: f32,
	peak_history: VecDeque<f32>,
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
				peak_ema: 0.0,
				rms_ema: 0.0,
				peak_history: VecDeque::with_capacity(64),
			};
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
				self.output_meter = output.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None).ok();
				self.output_volume = output.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok();
			}

			if let Ok(input) = self
				.enumerator
				.GetDefaultAudioEndpoint(eCapture, eMultimedia)
				.or_else(|_| self.enumerator.GetDefaultAudioEndpoint(eCapture, eConsole))
			{
				self.input_volume = input.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok();
			}
		}
	}
}

pub fn get_audio_json() -> Value {
	unsafe {
		let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
	}

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
				"name": "default-output",
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
				"name": "default-input",
				"volume_percent": (input_volume * 100.0).round(),
				"muted": input_muted,
			}
		})
	})
}
