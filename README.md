# nightjar

**A robust backup tool that runs while you sleep.**

> 😴 **Allergic to documentation?** → **[QUICKSTART.md](QUICKSTART.md)** — three commands and you're backing up.

nightjar backs up your folders to the cloud, **verifies** every file arrived intact, and — only if you ask — powers the machine off afterward. It is built around a simple promise: it will never tell you a backup succeeded unless it actually did, and it will never power off your machine on a backup that wasn't fully verified.

It wraps [rclone](https://rclone.org/) (which does the actual transfers and supports 70+ cloud providers) and adds a careful safety layer, a scriptable command-line tool, and a polished desktop interface.

---

## Screenshots

| Main window | Backup in progress | Directory tree |
| --- | --- | --- |
| ![main](docs/screenshot-main.png) | ![progress](docs/screenshot-progress.png) | ![tree](docs/screenshot-tree.png) |

---

## Features

- **Verified backups.** Every source is copied and then checked against the cloud copy. A backup is only "successful" if every file verified.
- **Optional power-off.** Choose to shut the machine down after a backup — but only ever after a *verified* one. A failed or aborted backup never powers off.
- **Handles "not enough space."** If the cloud doesn't have room for everything, nightjar offers to back up a subset — either automatically (smallest folders first) or a selection you choose — and tells you exactly what it skipped.
- **Guided cloud setup.** Connect a cloud account from the app; it walks you through rclone's own setup so you never have to read a manual.
- **Live progress.** A smooth progress bar driven by real transfer stats.
- **Backup report.** After a backup: what was backed up, how much space it used, and how much cloud space remains.
- **Directory tree viewer.** See the folder structure that will be backed up.
- **Abort anytime.** Stop a running backup cleanly; nightjar reports it as incomplete (never as success).
- **Themed interface.** Seven color themes, including neon-on-black.
- **Two ways to use it.** A graphical app, and a command-line tool suitable for scripts and scheduled (cron) runs.

---

## Compatibility

nightjar is a **Linux** application, developed and tested on Ubuntu. It should work on most mainstream desktop distributions (Fedora, Debian, Arch, Mint, Pop!_OS, openSUSE, and similar), with these per-feature requirements:

- **Backups and verification** — work anywhere rclone runs. No special requirements beyond rclone itself.
- **Power-off after backup** — requires **systemd** (`systemctl`). On distros using other init systems (e.g. Devuan, Void, Gentoo without systemd, Alpine), the backup still works; only the optional power-off step is unavailable.
- **The "Add folders" picker** — requires a desktop portal (`xdg-desktop-portal`), standard on GNOME and KDE. On minimal or window-manager-only setups it may be unavailable; you can still set folders via the config file.
- **The guided cloud-connect wizard** — opens your default terminal emulator; if none is found, it tells you to run `rclone config` yourself.
- **The desktop app** — needs working graphics drivers (it uses the GPU). On a headless server, use the command-line tool instead, which has no GUI requirement.

Binaries are built against glibc. For musl-based distributions (e.g. Alpine), build from source on that system.

macOS and Windows are **not** supported or tested.

---

## Prerequisites

### 1. rclone

nightjar uses rclone to talk to your cloud storage. Install it first:

- **Debian / Ubuntu / Mint:** `sudo apt install rclone`
- **Fedora:** `sudo dnf install rclone`
- **Arch:** `sudo pacman -S rclone`
- **Any distro:** see [rclone.org/install](https://rclone.org/install/)

Verify it is installed:

```sh
rclone version
```

### 2. A cloud remote

A "remote" is rclone's name for a configured cloud account (Google Drive, OneDrive, Dropbox, etc.).

**Easiest:** open the nightjar app and click **Connect a cloud account** — it launches rclone's guided setup for you.

**Or set one up manually:**

```sh
rclone config
```

Follow the prompts: choose `n` for a new remote, give it a name (e.g. `cloud`), pick your provider, and when asked *"Use web browser to automatically authenticate?"* choose **Yes** — your browser opens, you sign in, and rclone saves the connection. See [rclone's remote setup docs](https://rclone.org/docs/) for provider-specific notes.

Confirm it worked:

```sh
rclone listremotes
```

You should see your remote name (e.g. `cloud:`).

---

## Installing nightjar

nightjar is built from source with [Rust](https://www.rust-lang.org/tools/install).

```sh
# 1. Install Rust (if you don't have it)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Clone and build
git clone https://github.com/Sahasrajith-357/nightjar.git
cd nightjar
cargo build --release
```

The two binaries land in `target/release/`:

- `target/release/nightjar-cli` — the command-line tool
- `target/release/nightjar-gui` — the desktop app

You can run them from there, or copy them somewhere on your `PATH`:

```sh
cp target/release/nightjar-cli ~/.local/bin/nightjar-cli
cp target/release/nightjar-gui ~/.local/bin/nightjar-gui
```

If `nightjar-cli` isn't found afterward, ensure `~/.local/bin` is on your `PATH` (it is by default on most distros).

---

## Configuration

nightjar reads a config file at:
