# DSH-desk FAQ
Engliish|[中文](FAQ.md)
> Logs are the primary source for troubleshooting — for most issues, **check the logs first** ([Where are the logs?](#where-are-the-logs-how-do-i-send-them-to-the-developer)).

## Table of Contents

- [Installation and startup](#installation-and-startup)
  - [What if SmartScreen blocks the installer?](#what-if-smartscreen-blocks-the-installer)
  - [First launch stuck on the waiting page or a network error](#first-launch-stuck-on-the-waiting-page-or-a-network-error)
  - [Missing WebView2](#missing-webview2)
  - [Port already in use](#port-already-in-use)
  - [Blocked by antivirus or firewall (false positive)](#blocked-by-antivirus-or-firewall-false-positive)
- [Usage](#usage)
  - [Where do I enter the API key and where is it stored?](#where-do-i-enter-the-api-key-and-where-is-it-stored)
  - [Can the desktop and CLI versions of dsh coexist?](#can-the-desktop-and-cli-versions-of-dsh-coexist)
  - [Newly installed plugins don't take effect](#newly-installed-plugins-dont-take-effect)
  - [How do I change a hotkey conflict?](#how-do-i-change-a-hotkey-conflict)
  - [Does the app keep running in the background after I close the window?](#does-the-app-keep-running-in-the-background-after-i-close-the-window)
  - [The page looks different from the web version or has missing features](#the-page-looks-different-from-the-web-version-or-has-missing-features)
- [Data and troubleshooting](#data-and-troubleshooting)
  - [Where are the logs? How do I send them to the developer?](#where-are-the-logs-how-do-i-send-them-to-the-developer)
  - [Why is the disk usage so large?](#why-is-the-disk-usage-so-large)
  - [Will upgrading lose data? How do I back up?](#will-upgrading-lose-data-how-do-i-back-up)
  - [How do I know if there is a new version?](#how-do-i-know-if-there-is-a-new-version)
  - [Can I open the same UI in a plain browser instead of the desktop app?](#can-i-open-the-same-ui-in-a-plain-browser-instead-of-the-desktop-app)

---

## Installation and startup

### What if SmartScreen blocks the installer?

v1 installers are **not code-signed**, so Windows SmartScreen showing "Windows protected your PC" is expected. Proceed as follows:

1. Click **"More info"** in the dialog;
2. Click **"Run anyway"**;
3. Confirm with **"Yes"** if prompted.

The same applies to the portable build: if the extracted `dsh-desk.exe` is blocked, follow the steps above, or right-click the file → **Properties** → **Unblock** → OK.

> Security note: this app is open source (MIT); installers are built automatically from source by CI and uploaded to GitHub Releases — the only download channel. You can also build from source yourself.

### First launch stuck on the waiting page or a network error

The first launch does two things: **extract the engine locally** (no network needed) and **initialize plugin configuration** (needs access to the npm registry to download plugin dependencies).

- **The "Preparing local environment…" progress bar stays forever**: the first extraction of the sidecar takes 0.5–2 minutes (depends on disk speed and antivirus scanning). Be patient; if nothing progresses for a long time, open the log directory (below) to see progress and errors.
- **Plugin initialization fails (network issue)**: **this does not block startup** — initialization is idempotent. When the network is unavailable, only the bridge plugin is skipped (the host and Web UI work as usual, just without the "Desktop" section in settings); the failure reason is recorded in the logs. Once the network is back, **restarting the app** installs the missing pieces automatically.
- If it repeatedly fails (extraction/startup level): open the log directory and send the latest `shell-*.log` and `sidecar-*.log` contents to the developer (see [Where are the logs?](#where-are-the-logs-how-do-i-send-them-to-the-developer)).

### Missing WebView2

DSH-desk's UI depends on the Microsoft WebView2 Runtime (built into Windows 11; needs a separate install on Windows 10).

- The installer detects a missing runtime and guides the installation automatically (needs network, one-time);
- If that fails, download and install the **Evergreen WebView2 Runtime** manually from Microsoft (`https://developer.microsoft.com/microsoft-edge/webview2/`), then reopen the app;
- After installation, the error page disappears and the main UI loads normally.

### Port already in use

The app **picks a free port automatically** for the local Harness service on every launch, so conflicts are unlikely. If you see a port-related error (or security software is blocking local loopback listening), try:

1. Check whether other programs are occupying many ports, or whether security software is blocking `127.0.0.1` loopback traffic;
2. Click **"Retry"** on the error page (it retries on a new port);
3. If it still fails, check the logs to locate the exact error (the logs state the rejection reason).

### Blocked by antivirus or firewall (false positive)

Unsigned apps are easily flagged by security software. DSH-desk is open source with zero telemetry; the outbound traffic list is in the [root README](../README.en.md#privacy):

- If your antivirus flags it, choose "Allow" and **report the false positive** to the vendor (provide the SHA-256 checksum);
- If a firewall blocks it, allow `dsh-desk.exe` — it only listens on local `127.0.0.1` and accesses `api.deepseek.com` (model calls) and `registry.npmjs.org` (plugin installation);
- For the portable build, download from official Releases and verify the hash to avoid third-party repackaging.

---

## Usage

### Where do I enter the API key and where is it stored?

On first launch, the app guides you through entering your DeepSeek API key **inside the page**: create one at the [DeepSeek Open Platform](https://platform.deepseek.com) (shaped like `sk-…`), paste it in and save.

- The key is written to `~/.dsh/.credentials.yaml` (managed credential file, `0600` permissions; respects the `DSH_HOME` environment variable) — the **same credential mechanism as CLI dsh**, so both sides interoperate. Existing old keys in `~/.dsh/.env` are still read (fallback layer), but new writes always go to the credential file;
- The desktop shell only detects "whether a key is configured" (skips onboarding if present) — it **never reads or uploads** the key content;
- Changing the key: re-enter and save it in the settings page inside the app (overwrites the credential file), or edit `~/.dsh/.credentials.yaml` and restart the app.

### Can the desktop and CLI versions of dsh coexist?

Yes. Both share the same `~/.dsh` user data (profiles, sessions, credentials, plugins). The desktop app **always uses its bundled engine** and never touches a dsh installed on your system; both can be installed at the same time and switched freely, with seamless data.

Two notes:

- **Keep both on the same rc version** (currently `0.1.1-rc.2`). Both sides share the same profile directory for the plugin dependency tree; if versions drift too far, profile dependency fallback links follow "the **last launched** installation" (last-writer-wins) — switching back and forth may trigger dependency re-installs, which is expected and auto-fixes on the next launch.
- Do not use the CLI version to install plugins into **the same profile** while the desktop app is running, to avoid interleaved writes.

### Newly installed plugins don't take effect

Plugins (including community ones) require a **host restart** after installation:

- Click **"Restart host"** in the "Desktop" section of settings (or "Restart host" in the tray menu);
- The hint "New plugins take effect after the host restarts" refers to exactly this step.

Plugin installation itself uses the standard Harness `dsh plugin` mechanism (npm registry), identical to the web version.

### How do I change a hotkey conflict?

The default global hotkey is `Ctrl+Alt+D` (show/hide window); the "Desktop" section of settings displays the current value (v1 is read-only; a key-binding UI comes in a later version). To change it manually:

1. Quit DSH-desk;
2. Edit `~/.dsh/desktop/config.json` (respects `DSH_HOME`, i.e. `$DSH_HOME/desktop/config.json`):

```json
{
  "hotkey": "Ctrl+Shift+D",
  "autostart": false
}
```

3. Save and restart the app.

Notes: the format follows Tauri shortcut syntax (`Ctrl` / `Alt` / `Shift` + a single key); **invalid values automatically fall back** to `Ctrl+Alt+D` and will not prevent startup.

### Does the app keep running in the background after I close the window?

No. Closing the window = quitting the app (and stopping the local Harness service); no background process is left behind. Tray residence mode is planned for a later version. If you want it to run in the background, enable **autostart** in settings (it only auto-starts when you log into Windows; unrelated to residence).

### The page looks different from the web version or has missing features

It doesn't. The desktop app loads **the same Harness Web UI** (served by the bundled engine on local `127.0.0.1`), fully identical to the web version. The desktop app only injects the "Desktop" settings section and the notification mirror on top; opening the same address in a plain browser (without the bridge) simply hides those desktop-specific items — everything else is unchanged.

---

## Data and troubleshooting

### Where are the logs? How do I send them to the developer?

Rolling logs (shell 1MB×2, sidecar 1MB×2) live at:

```
~/.dsh/desktop/logs/          # respects DSH_HOME: $DSH_HOME/desktop/logs/
├─ shell-YYYYMMDD.log         # shell events: startup, ready, restart, failure reasons
└─ sidecar-YYYYMMDD.log       # raw sidecar output (including error stacks)
```

Open them via **"Open log directory"** in the tray menu or on the error page.

When reporting a bug or asking for help, attach **that day's** two log files (redact sensitive info such as API key fragments first) and describe: steps to reproduce, when it happens, and the DSH-desk version (visible on the error page or about page).

### Why is the disk usage so large?

The app bundles a full Node runtime and a pinned Harness dependency tree (sidecar) so that "no Node install, double-click to run" holds:

- The installer is about 55MB (sidecar ships as a single compressed archive); **after installation** the sidecar extracts to a local cache of about 320MB;
- Cache location: `%LOCALAPPDATA%\com.dsh.desk\sidecar-dist` (overridable via the `DSH_DESK_SIDECAR_CACHE` environment variable);
- The cache is reused while the engine version is unchanged; upgrades refresh it automatically (the old version's cache is cleaned up);
- To fully free the space: uninstall the app, then delete `%LOCALAPPDATA%\com.dsh.desk\` and `~/.dsh` (this also deletes personal data — back it up first).

### Will upgrading lose data? How do I back up?

No. Upgrading (the in-app one-click update or installing a new installer over the existing one) only replaces program files; profiles, sessions, and credentials under `~/.dsh` are **kept intact**; the engine version refreshes automatically on the first start after upgrading.

Manual backup (also useful for migrating to a new PC): copy the whole `~/.dsh` directory (note that `.env` / `.credentials.yaml` contain plaintext credentials — keep them safe).

### How do I know if there is a new version?

**Installed edition**: the app **auto-checks for updates once at every startup** (installed edition only; the portable edition does not). When a new version exists, a **"Download update"** button appears in the left sidebar between the logo row and the "New session" button (the × at its top-right dismisses the prompt for the current session); clicking it runs the same download → install → auto-restart flow as the in-app check.

You can also check manually: Settings → "Desktop" section → **"Check for updates"** queries the latest GitHub release and compares it with the current version:

- **New version** (installed): shows the version comparison and release notes; you can directly "Download & update" → "Install" — the app exits automatically, silently installs over the existing installation, and restarts itself;
- **New version** (portable): prompts you to download the latest zip from GitHub Releases and extract it over the current directory;
- **Up to date**: shows "You are up to date".

Each check (startup auto-check or manual click) issues a single GitHub API request (anonymous rate limit 60/hour/IP); you can also visit <https://github.com/JiaosSir/DSH-desk/releases> directly.

### Can I open the same UI in a plain browser instead of the desktop app?

Yes. The Harness service started by the desktop app listens on a random free port on local `127.0.0.1`; opening the same address in a browser gives you **equivalent functionality** (minus desktop-only items: tray, hotkey, notifications, autostart — these are provided by the desktop shell and are hidden automatically in a browser). The ready address is recorded in `~/.dsh/desktop/logs/shell-*.log` (the "Host ready: http://127.0.0.1:<port>" line).

> Note: use the desktop app for normal usage; opening it in a plain browser is only for troubleshooting scenarios like "is it the desktop shell's fault?".
