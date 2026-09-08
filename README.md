# OpenCode Usage OpenDeck plugin

An [OpenAction](https://openaction.amankhanna.me) plugin for [OpenDeck](https://github.com/nekename/OpenDeck) that tracks your [opencode](https://opencode.ai) API usage. The plugin is listed as **OpenCode Usage** in OpenDeck; its single action is **Usage**, under the `OpenCode` category.

The action shows a circular progress ring on the key:

- Ring fills to your current usage percentage.
- Ring color: green below 50%, yellow below 85%, red 85-100%.
- Center text: time until the next usage reset (e.g. `6 min.`, `2 hr`, `5 days`).
- Mode label at the top: `ROLLING`, `WEEKLY`, or `MONTHLY`.

## Requirements

- Rust (stable) to build.
- An opencode API key. Run `opencode auth` (or check your opencode config) to get it.

## Build & install

```sh
make install
```

This builds the plugin, stages it into `com.opendeck.opencodeusage.sdPlugin/`, and copies it into your OpenDeck plugins directory:

- Linux: `~/.config/opendeck/plugins/` (or `$XDG_CONFIG_HOME/opendeck/plugins/`)
- macOS: `~/Library/Application Support/OpenDeck/plugins/`

Restart OpenDeck (or reload plugins) and add the **Usage** action from the `OpenCode` category.

### Manual install

Run `make stage` to produce `com.opendeck.opencodeusage.sdPlugin/`, then copy that folder into your OpenDeck plugins directory (found via **Open config directory** in OpenDeck settings → `plugins/`).

## Packaging & releases

Run `make package` to assemble the plugin bundle and zip it into `com.opendeck.opencodeusage.zip`. The archive contains `com.opendeck.opencodeusage.sdPlugin/` and can be installed in OpenDeck via **Install from file**.

Releases are driven by PR labels. Every pull request to `main` must carry exactly one of the `major`, `minor`, or `patch` labels (validated by `.github/workflows/pr.yaml`). On merge, `.github/workflows/build.yml` bumps the version from that label, builds all five platform binaries (Windows, macOS, Linux; x86_64 + arm64), assembles the bundle, and publishes a GitHub Release with `com.opendeck.opencodeusage.zip` attached. CI writes the new version back into `Cargo.toml` and `assets/manifest.json` so they stay in sync with the release tag.

## Configuration

Select the action on your deck, then in the property inspector:

1. **Mode**: `Rolling`, `Weekly`, or `Monthly`.
2. **API Key**: paste your opencode API key. It is stored in the action's settings only.
3. **Show percentage**: toggle the usage percentage text on the key (default on). The countdown and layout are unchanged when hidden.

The key refreshes usage from `https://opencode.ai/zen/go/v1/usage` at most once a minute; the countdown text updates every 30 seconds. Press the key to force an immediate refresh.

## Development

```sh
cargo test       # run the render/countdown unit tests
make stage       # build and assemble the .sdPlugin directory
cargo clippy     # lint
```