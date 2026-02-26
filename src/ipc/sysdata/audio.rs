// ~/sentinel/sentinel-backend/src/ipc/sysdata/audio.rs

use serde_json::{json, Value};
use std::{
	cell::RefCell,
	collections::VecDeque,
	sync::{
		atomic::{AtomicBool, Ordering},
		OnceLock, RwLock,
	},
	time::{Duration, Instant},
};
use windows::Win32::{
	Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
	Media::Audio::{
		eCapture, eConsole, eMultimedia, eRender, IMMDevice, IMMDeviceEnumerator,
		MMDeviceEnumerator,
	},
	Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation},
	System::Com::{
		StructuredStorage::{PropVariantClear, PropVariantToStringAlloc},
		CoCreateInstance, CoInitializeEx, CoTaskMemFree, STGM_READ, CLSCTX_ALL,
		COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED,
	},
};
use windows::Media::Control::{
	GlobalSystemMediaTransportControlsSessionManager,
	GlobalSystemMediaTransportControlsSession,
	GlobalSystemMediaTransportControlsSessionMediaProperties,
	GlobalSystemMediaTransportControlsSessionPlaybackInfo,
	GlobalSystemMediaTransportControlsSessionTimelineProperties,
	GlobalSystemMediaTransportControlsSessionPlaybackStatus,
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

/// Media session polling cadence. This runs on a dedicated background thread
/// so media updates stay responsive even if audio pull cadence changes.
const MEDIA_POLL_INTERVAL_MS: u64 = 200;

static MEDIA_SESSION_CACHE: OnceLock<RwLock<Value>> = OnceLock::new();
static MEDIA_POLLER_STARTED: AtomicBool = AtomicBool::new(false);
static MEDIA_TIMELINE_TRACKER: OnceLock<RwLock<MediaTimelineTracker>> = OnceLock::new();

#[derive(Default)]
struct MediaTimelineTracker {
	session_key: String,
	position_ms: i64,
	sampled_at: Option<Instant>,
}

fn media_session_cache() -> &'static RwLock<Value> {
	MEDIA_SESSION_CACHE.get_or_init(|| RwLock::new(json!({ "playing": false })))
}

fn media_timeline_tracker() -> &'static RwLock<MediaTimelineTracker> {
	MEDIA_TIMELINE_TRACKER.get_or_init(|| RwLock::new(MediaTimelineTracker::default()))
}

fn start_media_poller_once() {
	if MEDIA_POLLER_STARTED
		.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
		.is_err()
	{
		return;
	}

	std::thread::spawn(|| {
		loop {
			let media = query_media_session();
			if media.is_null() {
				*media_session_cache().write().unwrap() = json!({ "playing": false });
			} else {
				*media_session_cache().write().unwrap() = media;
			}
			std::thread::sleep(Duration::from_millis(MEDIA_POLL_INTERVAL_MS));
		}
	});
}

fn refresh_media_session_cache() {
	let media = query_media_session();
	if media.is_null() {
		*media_session_cache().write().unwrap() = json!({ "playing": false });
	} else {
		*media_session_cache().write().unwrap() = media;
	}
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
	start_media_poller_once();
	refresh_media_session_cache();

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
			"media_session": media_session_cache().read().unwrap().clone(),
		})
	})
}

fn query_media_session() -> Value {
	unsafe {
		let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
	}

	// Block on the async request
	let manager: GlobalSystemMediaTransportControlsSessionManager = match
		GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
	{
		Ok(op) => {
			// Retry GetResults until the async op completes
			let mut result = None;
			for _ in 0..100 {
				match op.GetResults() {
					Ok(m) => { result = Some(m); break; }
					Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
				}
			}
			match result {
				Some(m) => m,
				None => return Value::Null,
			}
		}
		Err(_) => return Value::Null,
	};

	let current_session = manager.GetCurrentSession().ok();
	let sessions = manager.GetSessions().ok();
	let mut chosen_session = current_session.clone();
	let mut best_score: i32 = i32::MIN;

	if let Some(current) = current_session.as_ref() {
		let current_playing = current
			.GetPlaybackInfo()
			.ok()
			.and_then(|info| info.PlaybackStatus().ok())
			.map(|s| s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
			.unwrap_or(false);
		if current_playing {
			best_score = 100;
		}
	}

	if let Some(list) = sessions {
		if let Ok(size) = list.Size() {
			for i in 0..size {
				let Ok(candidate) = list.GetAt(i) else {
					continue;
				};

				let is_playing = candidate
					.GetPlaybackInfo()
					.ok()
					.and_then(|info| info.PlaybackStatus().ok())
					.map(|s| s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
					.unwrap_or(false);
				if !is_playing {
					continue;
				}

				let mut score = 100;
				let timeline = candidate.GetTimelineProperties().ok();
				let pos = timeline
					.as_ref()
					.and_then(|t| t.Position().ok())
					.map(|d| d.Duration / 10_000);
				let start = timeline
					.as_ref()
					.and_then(|t| t.StartTime().ok())
					.map(|d| d.Duration / 10_000)
					.unwrap_or(0);
				let end = timeline
					.as_ref()
					.and_then(|t| t.EndTime().ok())
					.map(|d| d.Duration / 10_000);
				let duration = end.map(|e| (e - start).max(0));
				let rel_pos = pos.map(|p| (p - start).max(0));

				if duration.unwrap_or(0) > 0 {
					score += 20;
				}

				if let Some(d) = duration {
					if d >= 900_000 {
						score += 220;
					} else if d >= 300_000 {
						score += 140;
					} else if d >= 120_000 {
						score += 90;
					} else if d >= 60_000 {
						score += 30;
					} else {
						score -= 50;
					}
				}

				if let (Some(p), Some(d)) = (rel_pos, duration) {
					if d > 0 {
						if p + 1000 < d {
							score += 40;
						} else {
							score -= 20;
						}
					}
				}

				if score > best_score {
					best_score = score;
					chosen_session = Some(candidate);
				}
			}
		}
	}

	let session: GlobalSystemMediaTransportControlsSession = match chosen_session {
		Some(s) => s,
		None => return json!({ "playing": false }),
	};

	let playback_info: Option<GlobalSystemMediaTransportControlsSessionPlaybackInfo> =
		session.GetPlaybackInfo().ok();
	let timeline: Option<GlobalSystemMediaTransportControlsSessionTimelineProperties> =
		session.GetTimelineProperties().ok();

	// Block on media properties async
	let properties: Option<GlobalSystemMediaTransportControlsSessionMediaProperties> =
		session.TryGetMediaPropertiesAsync().ok().and_then(|op| {
			for _ in 0..100 {
				match op.GetResults() {
					Ok(r) => return Some(r),
					Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
				}
			}
			None
		});

	let status = playback_info
		.as_ref()
		.and_then(|info| info.PlaybackStatus().ok());

	let status_str = status.map(|s| {
		if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing {
			"playing"
		} else if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused {
			"paused"
		} else if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped {
			"stopped"
		} else if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Opened {
			"opened"
		} else if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Closed {
			"closed"
		} else if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Changing {
			"changing"
		} else {
			"unknown"
		}
	});

	let is_playing = status
		.map(|s| s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
		.unwrap_or(false);

	let title = properties
		.as_ref()
		.and_then(|p| p.Title().ok())
		.map(|s| s.to_string());
	let artist = properties
		.as_ref()
		.and_then(|p| p.Artist().ok())
		.map(|s| s.to_string());
	let album = properties
		.as_ref()
		.and_then(|p| p.AlbumTitle().ok())
		.map(|s| s.to_string());
	let album_artist = properties
		.as_ref()
		.and_then(|p| p.AlbumArtist().ok())
		.map(|s| s.to_string());
	let track_number = properties
		.as_ref()
		.and_then(|p| p.TrackNumber().ok());
	let album_track_count = properties
		.as_ref()
		.and_then(|p| p.AlbumTrackCount().ok());
	let genres = properties.as_ref().and_then(|p| {
		p.Genres().ok().map(|g| {
			let mut v = Vec::new();
			if let Ok(size) = g.Size() {
				for i in 0..size {
					if let Ok(s) = g.GetAt(i) {
						v.push(Value::String(s.to_string()));
					}
				}
			}
			v
		})
	});
	let playback_type = properties
		.as_ref()
		.and_then(|p| p.PlaybackType().ok())
		.and_then(|pt| pt.Value().ok())
		.map(|v| match v.0 {
			0 => "unknown",
			1 => "music",
			2 => "video",
			3 => "image",
			_ => "other",
		});

	// Source app info
	let source_app_id = session
		.SourceAppUserModelId()
		.ok()
		.map(|s| s.to_string());

	// Timeline (units are 100-nanosecond intervals)
	let raw_position_ms = timeline
		.as_ref()
		.and_then(|t| t.Position().ok())
		.map(|d| d.Duration / 10_000);
	let start_ms = timeline
		.as_ref()
		.and_then(|t| t.StartTime().ok())
		.map(|d| d.Duration / 10_000);
	let end_ms = timeline
		.as_ref()
		.and_then(|t| t.EndTime().ok())
		.map(|d| d.Duration / 10_000);
	let position_ms = raw_position_ms.map(|pos| {
		start_ms
			.map(|start| (pos - start).max(0))
			.unwrap_or(pos.max(0))
	});
	let duration_ms = end_ms
		.zip(start_ms)
		.map(|(e, s)| (e - s).max(0));

	// Playback rate and shuffle/repeat
	let playback_rate: Option<f64> = playback_info
		.as_ref()
		.and_then(|info| info.PlaybackRate().ok())
		.and_then(|r| r.Value().ok());
	let is_shuffle: Option<bool> = playback_info
		.as_ref()
		.and_then(|info| info.IsShuffleActive().ok())
		.and_then(|v| v.Value().ok());
	let auto_repeat = playback_info
		.as_ref()
		.and_then(|info| info.AutoRepeatMode().ok())
		.and_then(|v| v.Value().ok())
		.map(|v| match v.0 {
			0 => "none",
			1 => "track",
			2 => "list",
			_ => "unknown",
		});

	let session_key = format!(
		"{}|{}|{}|{}|{}|{}",
		source_app_id.as_deref().unwrap_or(""),
		title.as_deref().unwrap_or(""),
		artist.as_deref().unwrap_or(""),
		album.as_deref().unwrap_or(""),
		track_number.unwrap_or(0),
		duration_ms.unwrap_or(0)
	);

	let mut effective_position_ms = position_ms.unwrap_or(0).max(0);
	let effective_rate = playback_rate.unwrap_or(1.0).max(0.0);
	let duration_hint = duration_ms.unwrap_or(0).max(0);
	let raw_looks_stale_end = is_playing
		&& duration_hint > 0
		&& position_ms
			.map(|p| p >= duration_hint.saturating_sub(500))
			.unwrap_or(false);
	let now = Instant::now();

	{
		let mut tracker = media_timeline_tracker().write().unwrap();

		if tracker.session_key == session_key {
			if is_playing && effective_rate > 0.0 {
				if let Some(sampled_at) = tracker.sampled_at {
					let elapsed_ms = now.duration_since(sampled_at).as_millis() as f64;
					let projected = tracker.position_ms as f64 + elapsed_ms * effective_rate;
					let projected_i64 = projected.floor() as i64;
					if raw_looks_stale_end && tracker.position_ms + 1500 < duration_hint {
						effective_position_ms = projected_i64.max(tracker.position_ms);
					} else {
						effective_position_ms = effective_position_ms.max(projected_i64);
					}
				}
			} else if position_ms.is_none() {
				effective_position_ms = tracker.position_ms;
			}
		} else if raw_looks_stale_end {
			effective_position_ms = 0;
		}

		if let Some(duration) = duration_ms {
			effective_position_ms = effective_position_ms.clamp(0, duration);
		} else {
			effective_position_ms = effective_position_ms.max(0);
		}

		tracker.session_key = session_key;
		tracker.position_ms = effective_position_ms;
		tracker.sampled_at = Some(now);
	}

	let position_ms = Some(effective_position_ms);

	json!({
		"playing": is_playing,
		"source_app_id": source_app_id,
		"title": title,
		"artist": artist,
		"album": album,
		"album_artist": album_artist,
		"track_number": track_number,
		"album_track_count": album_track_count,
		"genres": genres,
		"playback_type": playback_type,
		"playback_status": status_str,
		"playback_rate": playback_rate,
		"shuffle": is_shuffle,
		"repeat_mode": auto_repeat,
		"timeline": {
			"position_ms": position_ms,
			"start_ms": start_ms,
			"end_ms": end_ms,
			"duration_ms": duration_ms,
		}
	})
}
