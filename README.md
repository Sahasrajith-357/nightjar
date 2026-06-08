### Check everything is ready, without transferring

```sh
nightjar-cli preflight
```

Reports whether rclone is installed, the remote is reachable, your sources exist, and whether the backup fits in the available cloud space.

### Run a backup

```sh
nightjar-cli backup
```

`backup` options:

| Flag | Meaning |
| --- | --- |
| `--power-off` | Power off the machine after a *successful, verified* backup. |
| `--partial-method <METHOD>` | If space is short, choose without prompting: `smallest-first` or `custom`. |
| `-y`, `--yes` | Assume "yes" to confirmation prompts (for unattended runs). |

### Unattended / scheduled backups

For a hands-off nightly backup that powers the machine off when done:

```sh
nightjar-cli backup --power-off --partial-method smallest-first -y
```

Because the CLI returns a non-zero exit code on failure and only powers off on a verified backup, it is safe to run from cron or a systemd timer.

---

## How it works (and why it's safe)

nightjar is deliberately conservative with your data:

- **Preflight gates.** Before transferring anything, it checks — in order — that rclone is installed, the remote is configured and reachable, the network is up, and your source folders exist. If any check fails, it stops with a clear message and transfers nothing.
- **Verification.** After copying a folder, nightjar runs an integrity check of the cloud copy against the local files. A folder counts as backed up only if it both copied and verified.
- **Success means success.** A backup is reported successful only if *every* selected folder copied and verified. It stops at the first failure.
- **Power-off is gated.** The machine can only be powered off after a fully verified backup. Internally, the power-off step requires a permit that simply cannot be produced for a failed, partial-failed, or aborted backup — so an unverified backup can never shut your machine down.
- **Honest about partial backups.** If everything doesn't fit, nightjar backs up what it can and tells you exactly which folders it did not.
- **Non-destructive.** nightjar only ever *copies* from your folders to the cloud. It never deletes or modifies your local files.

---

## Building and development

nightjar is a Cargo workspace with three crates:

- `crates/core` — the engine: config, preflight, transfer/verify orchestration, the safety logic. Pure logic is unit-tested; cloud-touching paths have integration tests gated behind `--ignored`.
- `crates/cli` — the command-line front-end.
- `crates/gui` — the desktop app (built with [iced](https://iced.rs/)).

```sh
# Run the test suite
cargo test

# Run the cloud integration tests too (requires a configured remote named "cloud")
cargo test -- --ignored

# Build optimized binaries
cargo build --release
```

---

## Scheduling backups

nightjar doesn't need a built-in scheduler — Linux already has excellent ones. Because the command-line tool runs unattended and reports success/failure via exit codes, it's safe to run on a schedule. (If the network or cloud is unreachable at that time, it fails fast and does nothing — no hang, no harm.)

### Option A — Automatic backups with a systemd timer (recommended)

Run a backup automatically, e.g. every night. Create two files:

`~/.config/systemd/user/nightjar.service`

```ini
[Unit]
Description=nightjar backup

[Service]
Type=oneshot
ExecStart=%h/.local/bin/nightjar-cli backup -y --partial-method smallest-first
```

`~/.config/systemd/user/nightjar.timer`

```ini
[Unit]
Description=Run nightjar backup on a schedule

[Timer]
OnCalendar=daily
Persistent=true

[Install]
WantedBy=timers.target
```

Then enable it:

```sh
systemctl --user daemon-reload
systemctl --user enable --now nightjar.timer
systemctl --user list-timers nightjar.timer   # confirm it's scheduled
```

Change `OnCalendar=daily` to `weekly`, `Mon *-*-* 02:00:00` (Mondays at 2am), or any [systemd calendar expression](https://www.freedesktop.org/software/systemd/man/systemd.time.html). (Adjust the `ExecStart` path to wherever your `nightjar-cli` binary lives.)

> **Tip:** to add `--power-off`, the user must be allowed to power off non-interactively; on most desktops this works out of the box.

### Option B — Automatic backups with cron

Run `crontab -e` and add a line — for example, every day at 2am:

```cron
0 2 * * * /home/you/.local/bin/nightjar-cli backup -y --partial-method smallest-first
```

### Option C — Just remind me (I'll run it myself)

Prefer to stay in control? Schedule a desktop *notification* instead of an automatic backup. Run `crontab -e` and add a reminder — for example, every Sunday at 6pm:

```cron
0 18 * * 0 notify-send "nightjar" "Time to back up — run nightjar-gui"
```

(`notify-send` is provided by `libnotify` — `sudo apt install libnotify-bin` if you don't have it. Note: triggering desktop notifications from cron sometimes needs extra environment setup — the ready-made script below handles that for you.)

### Ready-to-use templates

Paste-ready templates are in [`examples/`](examples/):

- `nightjar-reminder.sh` + `crontab-reminder.txt` — desktop reminders (the script handles cron's environment so the notification reliably appears)
- `crontab-autobackup.txt` — automatic scheduled backups

---

## License

MIT — see [LICENSE](LICENSE).

## Author

Sahasrajith M
