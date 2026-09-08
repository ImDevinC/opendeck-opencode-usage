# AGENTS.md

## Project purpose

An [OpenAction](https://openaction.amankhanna.me) plugin for [OpenDeck](https://github.com/nekename/OpenDeck) that shows [opencode](https://opencode.ai) API usage on a stream-deck-style key.

The action renders a circular progress ring showing usage percentage for one of three windows (rolling, weekly, monthly). Ring color is green below 50%, yellow below 85%, red 85-100%. Center text shows the time until the next usage reset.

## Architecture

- **Rust backend** (`src/main.rs`) using the official `openaction` crate v2.1. This is the recommended OpenAction language. Do not rewrite in another language unless asked.
- **Single action** with UUID `com.opendeck.opencodeusage.usage`, defined in `assets/manifest.json`.
- **Property inspector** (`assets/pi.html`): a webview that stores the selected `mode`, the `api_key`, and the `show_percent` toggle in the action's settings via the OpenAction WebSocket (`setSettings` event). The backend reads them in `will_appear` / `did_receive_settings`.
- **Rendering**: the backend builds an SVG string and sends it to the key with `instance.set_image(...)` as a base64 data URI (`data:image/svg+xml;base64,...`). OpenDeck rasterises SVG at 144x144 in its frontend canvas, so SVG works on hardware. The manifest sets `ShowTitle: false` because the SVG carries all text. `show_percent` (default true) only hides the percentage `<text>`; font sizes and layout are fixed.
- **Manifest naming**: plugin and action are named `Usage` under the `OpenCode` category. `CategoryIcon` (`assets/opencode.png`) is the opencode favicon so the category listing shows the opencode logo. Keep the stage/install copies in sync if assets change.
- **Data source**: `GET https://opencode.ai/zen/go/v1/usage` with header `Authorization: Bearer <api_key>`. Response has `usage.{rolling|weekly|monthly}.{status,percent,resetsAt}`.

## How it stays live

- A `tokio::spawn` background task ticks every 30s (`tick_instances`), re-rendering countdown text from cached `resetsAt` using the local clock.
- It refetches from the API at most once per 60s (per instance, tracked by `last_fetch: Instant`).
- Pressing the key forces an immediate refetch + render.
- `set_usage_image` stores fresh fetch results back into the shared `INSTANCES` map so the ticker does not double-fetch.

## Key code conventions and gotchas

- **State tracking**: `INSTANCES` is a `static LazyLock<Mutex<HashMap<InstanceId, InstanceState>>>` keyed by `instance.instance_id`, updated in `will_appear` / `will_disappear` / `did_receive_settings`. The crate's own `Instance.settings_json` is `pub(crate)` and not readable from the plugin, so this map is how the ticker knows each instance's settings.
- **Do not hold the `INSTANCES` lock across `.await`**: `std::sync::MutexGuard` is not `Send`, so a guard held across an await fails to compile in spawned tasks and `async_trait` handlers. Snapshot (clone) state inside a scope, then await outside it.
- **SVG format strings**: use `r##"..."##` raw strings, never `r#"..."#`. The hex colors (`fill="#ffffff"`) contain the byte sequence `"#` which prematurely terminates `r#"..."#` and causes confusing `prefix ... is unknown` compile errors.
- **Truncate error text char-safely**: use `message.chars().take(16).collect()`, not `&message[..16]` (panics on multi-byte boundaries).
- **API JSON is camelCase**: the usage API sends `resetsAt` (camelCase). The response structs (`ApiResponse`, `UsageBuckets`, `UsageBucket`) carry `#[serde(rename_all = "camelCase")]`. Keep that annotation if the structs change; a missing field here surfaces as a generic `error decoding response body` in the plugin log, which can be mistaken for an image/SVG decode problem.
- **Pure helpers are testable**: `format_remaining`, `color_for`, `render_svg*`, `to_data_uri` are side-effect free. Add unit tests for any changes to them.
- Tabs for indentation (matches the `openaction` crate examples and this repo).

## Commands

```sh
cargo build --release     # build
cargo test --release      # unit tests (render/countdown/color logic)
cargo clippy --release    # lint; must be clean before finishing
make stage                # assemble com.opendeck.opencodeusage.sdPlugin/
make install              # stage + copy into OpenDeck plugins dir
make clean                # remove build artifacts and staged plugin dir
```

OpenDeck plugins directory: Linux `~/.config/opendeck/plugins/` (or `$XDG_CONFIG_HOME/opendeck/plugins/`), macOS `~/Library/Application Support/OpenDeck/plugins/`.

## Distribution notes

- Manifest `CodePaths`/`CodePathWin`/`CodePathMac`/`CodePathLin` list the five standard platform triples (`x86_64`/`aarch64` × windows/mac/linux). The binary is named `oaopencode-usage-<target-triple>`. Only the host platform is built locally; there is no CI matrix. Add one if asked.
- The plugin is not published to the OpenAction Marketplace.

## Docs maintenance

Keep `AGENTS.md` and `README.md` accurate and up to date automatically. When you change behavior, commands, dependencies, settings, or architecture, update the relevant section of both files in the same change. Do not wait to be asked.