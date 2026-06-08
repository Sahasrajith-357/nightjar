# nightjar — Quickstart

> For people who don't read READMEs. Run these in order. You'll be backing up in ~5 minutes.

**Linux only. You need [rclone](https://rclone.org/) and [Rust](https://rustup.rs/).**

```sh
# 1. Install rclone (pick your distro)
sudo apt install rclone      # Debian/Ubuntu/Mint
# sudo dnf install rclone     # Fedora
# sudo pacman -S rclone       # Arch

# 2. Build nightjar
git clone https://github.com/Sahasrajith-357/nightjar.git
cd nightjar
cargo build --release

# 3. Launch the app
./target/release/nightjar-gui
```

**Then, in the app:**
1. Click **Connect a cloud account** → follow the prompts (your browser opens; sign in). *(One time only.)*
2. Click **Common folders** to grab your usual stuff, or **Add folders** to pick your own.
3. Click **Back up now**. Done.

That's it. Everything else (the command-line tool, scheduled backups, config, all the knobs) is in the [full README](README.md).

---

**Prefer the terminal?** After step 2 above (connect a remote), skip the app entirely:

```sh
./target/release/nightjar-cli backup        # interactive
./target/release/nightjar-cli preflight      # just check, don't transfer
```
**Want it automatic?** Set up a nightly backup with a systemd timer or cron — see [Scheduling backups](README.md#scheduling-backups) in the full README.
