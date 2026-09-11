use openaction::*;

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

const USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct UsageSettings {
	mode: String,
	api_key: String,
	show_percent: bool,
}

impl Default for UsageSettings {
	fn default() -> Self {
		Self {
			mode: "rolling".to_string(),
			api_key: String::new(),
			show_percent: true,
		}
	}
}

/// Per-instance state tracked by the plugin so the background ticker can
/// refresh every visible key without waiting on the next inbound event.
#[derive(Clone, Default)]
struct InstanceState {
	settings: UsageSettings,
	resets_at: Option<i64>,
	percent: Option<f64>,
	last_fetch: Option<Instant>,
}

static INSTANCES: LazyLock<Mutex<HashMap<String, InstanceState>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiResponse {
	usage: UsageBuckets,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageBuckets {
	rolling: UsageBucket,
	weekly: UsageBucket,
	monthly: UsageBucket,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageBucket {
	percent: f64,
	resets_at: String,
}

impl UsageBuckets {
	fn get(&self, mode: &str) -> &UsageBucket {
		match mode {
			"weekly" => &self.weekly,
			"monthly" => &self.monthly,
			_ => &self.rolling,
		}
	}
}

fn format_remaining(secs: i64) -> String {
	if secs < 60 {
		return "now".to_string();
	}
	let mins = secs / 60;
	if mins < 60 {
		return format!("{} min.", mins);
	}
	let hours = mins / 60;
	if hours < 24 {
		return format!("{} hr", hours);
	}
	let days = hours / 24;
	format!("{} day{}", days, if days == 1 { "" } else { "s" })
}

fn color_for(percent: f64) -> &'static str {
	if percent < 50.0 {
		"#2ecc71"
	} else if percent < 85.0 {
		"#f1c40f"
	} else {
		"#e74c3c"
	}
}

fn render_svg(mode: &str, percent: f64, resets_at: i64, now: i64, show_percent: bool) -> String {
	let color = color_for(percent);
	let countdown = format_remaining((resets_at - now).max(0));

	let radius = 56.0;
	let circumference = 2.0 * std::f64::consts::PI * radius;
	let offset = circumference * (1.0 - (percent / 100.0).clamp(0.0, 1.0));

	let percent_text = format!("{:.0}%", percent);
	let mode_label = mode.to_uppercase();
	let percent_line = if show_percent {
		format!(r#"  <text x="72" y="102" text-anchor="middle" fill="{color}" font-family="sans-serif" font-size="14" font-weight="600">{percent_text}</text>"#)
	} else {
		String::new()
	};

	format!(
		r##"<svg xmlns="http://www.w3.org/2000/svg" width="144" height="144" viewBox="0 0 144 144">
  <circle cx="72" cy="72" r="{r}" fill="none" stroke="#3a3a3a" stroke-width="12"/>
  <circle cx="72" cy="72" r="{r}" fill="none" stroke="{color}" stroke-width="12" stroke-linecap="round"
     stroke-dasharray="{circ}" stroke-dashoffset="{offset}"
     transform="rotate(-90 72 72)"/>
  <text x="72" y="56" text-anchor="middle" fill="#9a9a9a" font-family="sans-serif" font-size="11" font-weight="600">{mode_label}</text>
  <text x="72" y="80" text-anchor="middle" fill="#ffffff" font-family="sans-serif" font-size="24" font-weight="bold">{countdown}</text>
{percent_line}
</svg>"##,
		r = radius,
		circ = circumference,
		offset = offset,
		color = color,
		mode_label = mode_label,
		countdown = countdown,
		percent_line = percent_line,
	)
}

fn render_svg_error(message: &str) -> String {
	let short: String = message.chars().take(16).collect();
	format!(
		r##"<svg xmlns="http://www.w3.org/2000/svg" width="144" height="144" viewBox="0 0 144 144">
  <text x="72" y="76" text-anchor="middle" fill="#e74c3c" font-family="sans-serif" font-size="18" font-weight="bold">err</text>
  <text x="72" y="98" text-anchor="middle" fill="#9a9a9a" font-family="sans-serif" font-size="12">{short}</text>
</svg>"##,
		short = short,
	)
}

fn render_svg_no_key() -> String {
	r##"<svg xmlns="http://www.w3.org/2000/svg" width="144" height="144" viewBox="0 0 144 144">
  <text x="72" y="76" text-anchor="middle" fill="#f1c40f" font-family="sans-serif" font-size="16" font-weight="bold">SET KEY</text>
</svg>"##
		.to_string()
}

fn to_data_uri(svg: &str) -> String {
	let b64 = base64::engine::general_purpose::STANDARD.encode(svg.as_bytes());
	format!("data:image/svg+xml;base64,{}", b64)
}

async fn send_image(instance: &Instance, svg: &str, label: &str) {
	let uri = to_data_uri(svg);
	log::debug!(
		"[{}] sending {label} image (uri len {})\nsvg: {}\nuri prefix: {}",
		instance.instance_id,
		uri.len(),
		svg,
		&uri[..uri.len().min(40)],
	);
	let result = instance.set_image(Some(uri), None).await;
	if let Err(e) = result {
		log::error!("[{}] set_image failed: {}", instance.instance_id, e);
	}
}

async fn fetch_usage(api_key: &str, mode: &str) -> Result<(f64, i64), String> {
	log::debug!("fetch_usage: requesting mode={} from {}", mode, USAGE_URL);
	let resp = CLIENT
		.get(USAGE_URL)
		.header("Authorization", format!("Bearer {}", api_key))
		.send()
		.await
		.map_err(|e| {
			log::warn!("fetch_usage: request failed: {}", e);
			e.to_string()
		})?;

	if !resp.status().is_success() {
		log::warn!("fetch_usage: non-success status {}", resp.status());
		return Err(format!("HTTP {}", resp.status()));
	}

	let body = resp.text().await.map_err(|e| {
		log::warn!("fetch_usage: could not decode response body: {:#}", e);
		format!("{:#}", e)
	})?;

	let parsed: ApiResponse = serde_json::from_str(&body).map_err(|e| {
		let preview: String = body.chars().take(200).collect();
		log::warn!("fetch_usage: json parse failed: {}\nbody preview: {}", e, preview);
		format!("{}", e)
	})?;
	let bucket = parsed.usage.get(mode);
	let resets = chrono::DateTime::parse_from_rfc3339(&bucket.resets_at)
		.map(|d| d.timestamp())
		.unwrap_or_else(|_| {
			log::warn!("fetch_usage: could not parse resets_at '{}'", bucket.resets_at);
			0
		});
	log::debug!("fetch_usage: mode={} percent={} resets_at={}", mode, bucket.percent, resets);
	Ok((bucket.percent, resets))
}

async fn set_usage_image(instance: &Instance, state: &InstanceState) {
	let now = chrono::Utc::now().timestamp();
	log::debug!(
		"[{}] set_usage_image: mode={} key_set={} cached={:?}",
		instance.instance_id,
		state.settings.mode,
		!state.settings.api_key.trim().is_empty(),
		(state.percent, state.resets_at),
	);

	if state.settings.api_key.trim().is_empty() {
		log::debug!("[{}] set_usage_image: no key, showing SET KEY", instance.instance_id);
		send_image(instance, &render_svg_no_key(), "no-key").await;
		return;
	}

	let (percent, resets_at) = match (state.percent, state.resets_at) {
		(Some(p), Some(r)) => (p, r),
		_ => {
			// No cached data yet; fetch synchronously and fall back to an error
			// message if that fails.
			let usage = match fetch_usage(&state.settings.api_key, &state.settings.mode).await {
				Ok(u) => u,
				Err(e) => {
					log::warn!("[{}] set_usage_image: fetch failed: {}", instance.instance_id, e);
					send_image(instance, &render_svg_error(&e), "error").await;
					return;
				}
			};
			// Store the fresh result so the background ticker doesn't refetch.
			if let Some(s) = INSTANCES.lock().unwrap().get_mut(&instance.instance_id) {
				s.percent = Some(usage.0);
				s.resets_at = Some(usage.1);
				s.last_fetch = Some(Instant::now());
			}
			usage
		}
	};

	let svg = render_svg(&state.settings.mode, percent, resets_at, now, state.settings.show_percent);
	send_image(instance, &svg, "usage").await;
}

struct UsageAction;

#[async_trait]
impl Action for UsageAction {
	const UUID: ActionUuid = "com.imdevinc.opencodeusage.usage";
	type Settings = UsageSettings;

	async fn will_appear(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		log::debug!(
			"[{}] will_appear: mode={} key_set={}",
			instance.instance_id,
			settings.mode,
			!settings.api_key.trim().is_empty(),
		);
		let id = instance.instance_id.clone();
		let state = {
			let mut map = INSTANCES.lock().unwrap();
			let entry = map.entry(id.clone()).or_default();
			entry.settings = settings.clone();
			entry.clone()
		};

		// Render immediately using whatever we have; the background ticker will
		// fetch fresh data if needed.
		set_usage_image(instance, &state).await;
		Ok(())
	}

	async fn will_disappear(&self, instance: &Instance, _settings: &Self::Settings) -> OpenActionResult<()> {
		log::debug!("[{}] will_disappear", instance.instance_id);
		INSTANCES.lock().unwrap().remove(&instance.instance_id);
		Ok(())
	}

	async fn did_receive_settings(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		log::debug!(
			"[{}] did_receive_settings: mode={} key_set={}",
			instance.instance_id,
			settings.mode,
			!settings.api_key.trim().is_empty(),
		);
		let id = instance.instance_id.clone();
		let state = {
			let mut map = INSTANCES.lock().unwrap();
			let entry = map.entry(id.clone()).or_default();
			// Settings changed; drop the cached fetch so the next render refetches.
			entry.settings = settings.clone();
			entry.resets_at = None;
			entry.percent = None;
			entry.last_fetch = None;
			entry.clone()
		};

		set_usage_image(instance, &state).await;
		Ok(())
	}

	async fn key_up(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		// Manual refresh on press.
		log::debug!("[{}] key_up: manual refresh", instance.instance_id);
		let state = InstanceState {
			settings: settings.clone(),
			resets_at: None,
			percent: None,
			last_fetch: None,
		};
		set_usage_image(instance, &state).await;
		Ok(())
	}
}

async fn tick_instances() {
	let now = Instant::now();

	// Fetch the visible instances without holding our lock across an await.
	let visible = visible_instances(UsageAction::UUID).await;

	// Collect a snapshot of instances + their tracked state so we don't hold
	// the lock across async work.
	let snapshot: Vec<(std::sync::Arc<Instance>, InstanceState)> = {
		let map = INSTANCES.lock().unwrap();
		visible
			.into_iter()
			.filter_map(|instance| {
				let id = instance.instance_id.clone();
				map.get(&id).map(|state| (instance, state.clone()))
			})
			.collect()
	};
	log::debug!("tick_instances: {} visible instances", snapshot.len());

	for (instance, mut state) in snapshot {
		// Refetch at most once a minute.
		let stale = state.last_fetch.map(|t| now.duration_since(t) >= Duration::from_secs(60)).unwrap_or(true);
		log::debug!(
			"[{}] tick: stale={} last_fetch={:?}",
			instance.instance_id,
			stale,
			state.last_fetch.map(|t| now.duration_since(t).as_secs()),
		);
		if stale {
			if !state.settings.api_key.trim().is_empty() {
				match fetch_usage(&state.settings.api_key, &state.settings.mode).await {
					Ok((percent, resets_at)) => {
						state.percent = Some(percent);
						state.resets_at = Some(resets_at);
					}
					Err(e) => {
						log::warn!("[{}] tick: fetch failed: {}", instance.instance_id, e);
						send_image(&instance, &render_svg_error(&e), "error").await;
						continue;
					}
				}
			}
			state.last_fetch = Some(now);
			if let Some(s) = INSTANCES.lock().unwrap().get_mut(&instance.instance_id) {
				*s = state.clone();
			}
		}
		set_usage_image(&instance, &state).await;
	}
}

#[tokio::main]
async fn main() -> OpenActionResult<()> {
	{
		use simplelog::*;
		if let Err(error) = TermLogger::init(
			LevelFilter::Debug,
			Config::default(),
			TerminalMode::Stdout,
			ColorChoice::Never,
		) {
			eprintln!("Logger initialization failed: {}", error);
		}
	}

	register_action(UsageAction).await;

	// Background refresher: update countdown text every 30s and refetch usage
	// at most once a minute.
	let ticker = tokio::spawn(async {
		log::debug!("background ticker started (30s interval)");
		let mut interval = tokio::time::interval(Duration::from_secs(30));
		loop {
			interval.tick().await;
			tick_instances().await;
		}
	});

	let result = run(std::env::args().collect()).await;
	ticker.abort();
	result
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn countdown_minutes() {
		assert_eq!(format_remaining(45), "now");
		assert_eq!(format_remaining(6 * 60), "6 min.");
	}

	#[test]
	fn countdown_hours() {
		assert_eq!(format_remaining(2 * 3600), "2 hr");
		assert_eq!(format_remaining(3600), "1 hr");
	}

	#[test]
	fn countdown_days() {
		assert_eq!(format_remaining(24 * 3600), "1 day");
		assert_eq!(format_remaining(5 * 24 * 3600), "5 days");
	}

	#[test]
	fn parses_sample_api_payload() {
		// Mirrors the real /zen/go/v1/usage response (camelCase resetsAt).
		let body = r#"{
			"usage": {
				"rolling": { "status": "ok", "percent": 8, "resetsAt": "2026-09-08T20:06:26.048Z" },
				"weekly": { "status": "ok", "percent": 3, "resetsAt": "2026-09-14T00:00:00.048Z" },
				"monthly": { "status": "ok", "percent": 1, "resetsAt": "2026-10-08T15:03:09.048Z" }
			}
		}"#;
		let parsed: ApiResponse = serde_json::from_str(body).expect("sample payload must deserialize");
		assert_eq!(parsed.usage.get("rolling").percent, 8.0);
		assert_eq!(parsed.usage.get("weekly").resets_at, "2026-09-14T00:00:00.048Z");
		assert_eq!(parsed.usage.get("monthly").percent, 1.0);
	}

	#[test]
	fn colors() {
		assert_eq!(color_for(49.9), "#2ecc71");
		assert_eq!(color_for(50.0), "#f1c40f");
		assert_eq!(color_for(84.9), "#f1c40f");
		assert_eq!(color_for(85.0), "#e74c3c");
		assert_eq!(color_for(100.0), "#e74c3c");
	}

	#[test]
	fn ring_geometry() {
		let svg = render_svg("weekly", 25.0, 1_000_000, 0, true);
		assert!(svg.contains("stroke-dasharray=\"351.858"));
		assert!(svg.contains("#2ecc71"));
		assert!(svg.contains("WEEKLY"));
		assert!(svg.contains("11 day"));
	}

	#[test]
	fn full_ring_has_zero_offset_arc() {
		// At 100%, the arc covers the whole ring (offset 0).
		let svg = render_svg("rolling", 100.0, 1_000_000, 0, true);
		assert!(svg.contains("stroke-dashoffset=\"0\""));
	}

	#[test]
	fn percent_toggle_shows_and_hides_percent_text() {
		let with_percent = render_svg("rolling", 42.0, 1_000_000, 0, true);
		assert!(with_percent.contains(">42%</text>"));
		assert!(with_percent.contains("font-size=\"24\""));

		let without_percent = render_svg("rolling", 42.0, 1_000_000, 0, false);
		assert!(!without_percent.contains("42%"));
		// Layout and countdown size are unchanged when the toggle is off.
		assert!(without_percent.contains("font-size=\"24\""));
	}

	#[test]
	fn data_uri_is_svg() {
		let uri = to_data_uri(&render_svg_no_key());
		assert!(uri.starts_with("data:image/svg+xml;base64,"));
	}

	#[test]
	fn error_truncation_is_char_safe() {
		let svg = render_svg_error("this is a very long message that should be truncated!");
		assert!(svg.contains("this is a very l"));
	}
}
