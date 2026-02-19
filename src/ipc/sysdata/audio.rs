// ~/sentinel/sentinel-backend/src/ipc/sysdata/audio.rs

use serde_json::{json, Value};
use std::cell::RefCell;
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
		let mut output_level = 0.0f32;
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
					output_level = peak.clamp(0.0, 1.0);
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

		json!({
			"output_device": {
				"name": "default-output",
				"volume_percent": (output_volume * 100.0).round(),
				"muted": output_muted,
				"audio_level": output_level,
			},
			"input_device": {
				"name": "default-input",
				"volume_percent": (input_volume * 100.0).round(),
				"muted": input_muted,
			}
		})
	})
}
