// ~/sentinel/sentinel-backend/src/ipc/sysdata/media.rs

//
// ─────────────────────────────────────────────────────────────
// THIS SHIT DOES NOT WORK AND I DONT KNOW HOW TO MAKE IT WORK
// ─────────────────────────────────────────────────────────────
//

use serde_json::{json, Value};
use std::{
	sync::{
		atomic::{AtomicBool, Ordering},
		OnceLock, RwLock,
	},
	time::{Duration, Instant},
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Media::Control::{
	GlobalSystemMediaTransportControlsSession,
	GlobalSystemMediaTransportControlsSessionManager,
	GlobalSystemMediaTransportControlsSessionMediaProperties,
	GlobalSystemMediaTransportControlsSessionPlaybackInfo,
	GlobalSystemMediaTransportControlsSessionPlaybackStatus,
	GlobalSystemMediaTransportControlsSessionTimelineProperties,
};

// ── constants ────────────────────────────────────────────────

const MEDIA_POLL_INTERVAL_MS: u64 = 200;

// ── statics ──────────────────────────────────────────────────

static MEDIA_SESSION_CACHE: OnceLock<RwLock<Value>> = OnceLock::new();
static MEDIA_TIMELINE_TRACKER: OnceLock<RwLock<MediaTimelineTracker>> = OnceLock::new();
static MEDIA_WORKER_STARTED: AtomicBool = AtomicBool::new(false);

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

// ── public API (called from audio.rs) ────────────────────────

/// Starts the background media-session poller (once) and also does
/// one synchronous refresh so the cache is always fresh.
pub fn refresh_media_session_cache_if_due() {
	start_media_poller_once();
}

pub fn get_media_session_json() -> Value {
	media_session_cache()
		.read()
		.map(|v| v.clone())
		.unwrap_or_else(|_| json!({ "playing": false }))
}

// ── worker thread ────────────────────────────────────────────

fn start_media_poller_once() {
	if MEDIA_WORKER_STARTED
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

// ── core query (GetResults polling — the proven approach) ────

fn query_media_session() -> Value {
	unsafe {
		let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
	}

	// Block on RequestAsync via GetResults() polling
	let manager: GlobalSystemMediaTransportControlsSessionManager = match
		GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
	{
		Ok(op) => {
			let mut result = None;
			for _ in 0..100 {
				match op.GetResults() {
					Ok(m) => { result = Some(m); break; }
					Err(_) => std::thread::sleep(Duration::from_millis(10)),
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
		let current_status = current
			.GetPlaybackInfo()
			.ok()
			.and_then(|info| info.PlaybackStatus().ok());
		best_score = match current_status {
			Some(s) if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing => 320,
			Some(s) if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused => 220,
			Some(s) if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Opened => 180,
			Some(s) if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Changing => 140,
			Some(s) if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped => 80,
			_ => 40,
		};
	}

	if let Some(list) = sessions {
		if let Ok(size) = list.Size() {
			for i in 0..size {
				let Ok(candidate) = list.GetAt(i) else {
					continue;
				};

				let status = candidate
					.GetPlaybackInfo()
					.ok()
					.and_then(|info| info.PlaybackStatus().ok());

				let mut score = match status {
					Some(s) if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing => 300,
					Some(s) if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused => 200,
					Some(s) if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Opened => 160,
					Some(s) if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Changing => 120,
					Some(s) if s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped => 60,
					_ => 20,
				};

				if current_session
					.as_ref()
					.and_then(|c| c.SourceAppUserModelId().ok())
					.zip(candidate.SourceAppUserModelId().ok())
					.map(|(a, b)| a == b)
					.unwrap_or(false)
				{
					score += 25;
				}

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
		None => {
			return json!({ "playing": false });
		}
	};

	let playback_info: Option<GlobalSystemMediaTransportControlsSessionPlaybackInfo> =
		session.GetPlaybackInfo().ok();
	let timeline: Option<GlobalSystemMediaTransportControlsSessionTimelineProperties> =
		session.GetTimelineProperties().ok();

	// Block on TryGetMediaPropertiesAsync via GetResults() polling
	let properties: Option<GlobalSystemMediaTransportControlsSessionMediaProperties> =
		session.TryGetMediaPropertiesAsync().ok().and_then(|op| {
			for _ in 0..100 {
				match op.GetResults() {
					Ok(r) => return Some(r),
					Err(_) => std::thread::sleep(Duration::from_millis(10)),
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

	let source_app_id = session
		.SourceAppUserModelId()
		.ok()
		.map(|s| s.to_string());

	// Timeline (units are 100-nanosecond intervals → ms)
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

	// ── Timeline interpolation ───────────────────────────────
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
