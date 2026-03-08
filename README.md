# 🍿 Rezka Downloader

A cross-platform desktop application for downloading films and series from HDRezka. Built with **Tauri 2** (Rust) and **React** (TypeScript), it supports authentication for premium access, multi-threaded downloads, and full season batch downloading.

## ⚙️ Features

- **Search & Discovery** — fast search with real-time suggestions, advanced paginated search with images, or paste a direct URL
- **Authentication** — optional login for premium/higher-quality streams with persistent session
- **Translator Selection** — choose from available dubbing/subtitle options per title
- **Quality Selection** — pick resolution from 240p up to 4K
- **Season & Episode Support** — browse seasons/episodes, download individually or batch-download an entire season
- **Multi-threaded Downloads** — parallel downloading with configurable thread count (1–64, default 16)
- **Download Manager** — real-time progress, speed estimates, queue management, cancel/remove tasks
- **Configurable** — custom HDRezka mirror URL, download directory, thread count, session file path

## 🧩 Requirements

- [Node.js](https://nodejs.org/) (for frontend build)
- [Rust](https://www.rust-lang.org/tools/install) toolchain (for Tauri backend)
- [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS

## 🛠️ Installation

### From source

1. Clone the repository:

   ```bash
   git clone https://github.com/your-username/rezka-downloader.git
   cd rezka-downloader/src
   ```

2. Install frontend dependencies:

   ```bash
   npm install
   ```

3. Run in development mode:

   ```bash
   npm run tauri dev
   ```

4. Build a release binary:

   ```bash
   npm run tauri build
   ```

   The built application will be placed in `src-tauri/target/release/bundle/`.

### From Releases

Download a prebuilt binary from the [Releases](../../releases) page for your platform.

**macOS note:** If you see *"App is damaged and can't be opened"*, this is because the app is not notarized by Apple. To fix this, run the following command in Terminal:

```bash
xattr -cr /Applications/rezka-downloader.app
```

Replace the path with wherever you placed the `.app` bundle. After that, the app will open normally.

**Linux:** make the binary executable if needed:

```bash
chmod +x /path/to/rezka-downloader
```

## 🚀 Usage

1. **Launch** the application.
2. **(Optional) Log in** — click the login button and enter your HDRezka credentials for access to premium-quality streams. Login is persistent; you only need to do it once.
3. **Search** — type a title into the search bar for quick suggestions, use advanced search, or paste a direct HDRezka URL.
4. **Select** — pick a translator/dubbing option and quality.
5. **Download** — download a single episode or an entire season. Track progress in the Downloads panel.

## 🔧 Configuration

Open the **Settings** panel in the app to configure:

| Setting | Description | Default |
|---|---|---|
| HDRezka Origin | Mirror/origin URL | `https://rezka.ag` |
| Download Directory | Where files are saved | System default |
| Thread Count | Parallel download threads (1–64) | `16` |
| Session File | Path to session/cookie file | App data directory |

## ⚠️ Troubleshooting

- **Login failures** — verify credentials and network connectivity. HDRezka may change its authentication flow; try logging in via a browser and inspect cookies if necessary.
- **Downloads hang or fail** — try a different quality or ensure network access to the configured HDRezka mirror. Using an authenticated session may help bypass request limits.
- **App won't start on macOS** — you may need to allow the app in *System Settings → Privacy & Security* since it is not signed from the App Store.

If you hit an error, please open an issue with the steps to reproduce and a short copy of the error output.

## 🏗️ Tech Stack

| Layer | Technology |
|---|---|
| Desktop Framework | Tauri 2 |
| Backend | Rust (reqwest, scraper, tokio, serde) |
| Frontend | React 19, TypeScript, Vite 7 |
| IPC | Tauri command system |

## 🤝 Contributing

Contributions are welcome. Please open an issue for discussion before submitting a non-trivial change. Small fixes (typos, docs) can be sent as PRs directly.
