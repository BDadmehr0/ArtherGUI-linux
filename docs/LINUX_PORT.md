# Aethon Linux Port (Arch Linux / Omarchy, x86_64)

This document is the Phase-A audit, the implementation plan, and the
as-built record for the Linux port. It is kept honest: anything in the
"verified" column below names *how* it was verified, and anything that
could not be run is listed as unverified rather than implied to work.

- Upstream GUI: `hamvex/AetherGUI` v2.2.0, commit
  `51d409a9417922c80f1e140e684ae1f96b1a75e6` (imported unmodified as the
  base of this repository; the Linux delta is every commit after it).
- Networking core: official `CluvexStudio/Aether` **v2.3.0**
  (source commit `6175b67df370ab856bcee07fe85b524031903956`, AGPL-3.0),
  the same version the Windows/Android clients pin.
- Routing engine: Xray-core **v26.3.27** (MPL-2.0), the same version
  Windows pins, using its official `Xray-linux-64.zip` build.

## 1. How the upstream application actually works

```
┌─────────────┐  Tauri IPC   ┌──────────────────┐  spawns   ┌───────────────┐
│ src/        │ ───────────▶ │ src-tauri (Rust) │ ────────▶ │ aether core   │
│ HTML/JS/CSS │ ◀── events ─ │ lib.rs/process.rs│  env+stdio│ SOCKS5 :1819  │
└─────────────┘              │ routing.rs, ...  │           │ HTTP   :1818  │
                              └───────┬──────────┘           └──────▲────────┘
                                      │ elevated helper             │ SOCKS
                                      │ (same binary,              │
                                      │  --routing-helper)   ┌──────┴────────┐
                                      └────────────────────▶ │ xray TUN      │
                                                             │ system routes │
                                                             └───────────────┘
```

- The **frontend** (`src/index.html`, `app.js`, `i18n.js`,
  `mercury-orb.js`, `styles.css`) is a static bundle with no build
  framework. It talks to Rust exclusively through Tauri commands and
  events. It contains no networking logic and is **platform-independent**
  (two cosmetic strings aside, it never mentions Windows).
- `src-tauri/src/lib.rs` owns the connect/disconnect state machine,
  data-plane probes (Cloudflare trace / HTTPS over SOCKS), settings I/O,
  tray, and every Tauri command. Settings are translated to `AETHER_*`
  environment variables (`settings.rs::environment_with_chain`) — the
  core's configuration contract is environment + TOML files, identical
  on every OS. Configuration import/export is JSON backup of the same
  `Settings` struct, so Windows backups restore on Linux unchanged.
- `process.rs` resolves the bundled `aether` sidecar, verifies its
  SHA-256 against a compile-time pin **before exec**, spawns it with a
  scrubbed environment (inherited `AETHER_*` stripped), and supervises
  it (log sanitisation, stats parsing, watchdog, kill-on-drop).
- `chain.rs` implements the Psiphon/Tor privacy-chain port relays
  (public 1818/1819 → chain egress 1821/1822…) in pure tokio TCP —
  platform-independent.
- **VPN (TUN) mode**: `routing.rs` writes a validated `request.json`
  into a per-session directory, launches *the same executable* elevated
  (`--routing-helper <request.json>`), and polls `status.json`. The
  helper verifies + spawns Xray, whose TUN inbound forwards everything
  to Aether's local SOCKS5. Teardown is via `control.json`, GUI-PID
  death watch, or `--repair-network` recovery. This request/status
  file protocol is sound and is reused verbatim on Linux.
- **Manual mode** skips routing entirely: the core's SOCKS5/HTTP
  listeners are the product. Nothing about manual mode needs privilege
  on any platform.

### The core on Linux (the question the whole port turns on)

Upstream Aether v2.3.0 publishes first-class Linux builds, including
`aether-linux-x86_64.tar.gz` (glibc ≥ 2.34 — Arch is far newer) with
layout `aether`, `pt/psiphon-tunnel-core`, `pt/lyrebird`, `LICENSES/`
(verified from the upstream release workflow, which cross-builds the
pt helpers per-GOOS). Xray v26.3.27 likewise publishes
`Xray-linux-64.zip` with a TUN inbound. **No core replacement is
needed.** Archive digests were taken from the GitHub release-asset API
and are pinned in `scripts/linux-pins.json`:

| Artifact | SHA-256 (archive) |
|---|---|
| `aether-linux-x86_64.tar.gz` (v2.3.0) | `77c6ce41695a616660d834b55be235c17e7588a474dfd737b71ca80b685bdc8a` |
| `Xray-linux-64.zip` (v26.3.27) | `23cd9af937744d97776ee35ecad4972cf4b2109d1e0fe6be9930467608f7c8ae` |

## 2. Windows-only components and their Linux treatment

| # | Component (file) | Windows mechanism | Linux treatment |
|---|---|---|---|
| 1 | Routing helper elevation (`routing::launch_elevated`) | `ShellExecuteW "runas"` (UAC) | `pkexec` (polkit) spawning the same `--routing-helper` mode; GUI never runs as root. Early pkexec failure (auth cancelled) is turned into a `status.json` error so the UI does not wait out the 45 s poll. |
| 2 | TUN device | Wintun adapter created by Xray + `netsh` addressing | Xray creates the TUN via `/dev/net/tun` (needs root/CAP_NET_ADMIN — the helper is root via pkexec); helper assigns `172.19.0.1/30` + `fdfe:dcba:9876::1/126` addressing with `ip` and sets MTU/link-up. Interface names respect the 15-char Linux limit (`aethon<N>`). |
| 3 | Takeover routing | `netsh` /1 routes on the adapter, metric games | **Policy routing**: `ip rule` priority 10000 `fwmark 97 → main`; priority 10001 `→ table 41840`; table 41840 holds only `default dev aethonN`. The user's main table/default route is **never modified**, so teardown = delete our rules + flush our table. |
| 4 | Loop prevention | Xray `process` rule (`aether.exe`, `xray.exe` → direct) | Xray process attribution is **not available for TUN on Linux**, so loop prevention is done in the kernel: the helper moves the Aether core PID (and Xray, its child) into cgroup v2 `/sys/fs/cgroup/aethon`, and an nftables table `inet aethon` (route/output hook) marks that cgroup's packets fwmark 97 → main table. Xray's freedom outbound also gets `sockopt.mark = 97` as a second belt. Children of the core (pt helpers) inherit the cgroup. |
| 5 | DNS | `netsh` per-adapter DNS 1.1.1.1 | `resolvectl dns/domain/default-route` on the TUN link when systemd-resolved answers (Arch/Omarchy default), reverted with `resolvectl revert`. Even without resolved, DNS packets enter the TUN by policy routing and the Xray port-53 → Aether rule (kept) prevents leaks. `/etc/resolv.conf` and NetworkManager profiles are never edited. |
| 6 | Privilege-boundary anchoring (`routing_base_dir`, `authorized_session_dir`) | `%LOCALAPPDATA%` canonicalisation | Same anchoring re-derived on Linux: data home of the *invoking* user (`PKEXEC_UID` → passwd entry when elevated; `XDG_DATA_HOME`/`HOME` otherwise), canonicalised, symlink-redirection refused. The previous non-Windows stub that returned the candidate unchecked is gone — it would have been a privilege-boundary hole. |
| 7 | Helper singleton lock | Named mutex `Local\AethonRoutingHelper` | `flock(LOCK_EX|LOCK_NB)` on `/run/aethon-routing.lock`, same retry budget. |
| 8 | Stale-adapter sweep | PowerShell `Get-NetAdapter`/`pnputil` | `/sys/class/net/aethon*` sweep: `resolvectl revert` + `ip link del`, plus our rules/table/nft-table/cgroup removal. Only Aethon-named objects are touched. |
| 9 | Owned-engine termination (repair path) | `OpenProcess` + image-path check + `TerminateProcess` | `/proc/<pid>/exe` must resolve to a shipped engine path before SIGTERM/SIGKILL — same anti-abuse property. |
| 10 | TUN counters (diagnostics) | `GetIfEntry2` MIB row | `/sys/class/net/<iface>/statistics/{rx,tx}_bytes`. |
| 11 | Core supervision (`process.rs`) | Job Object (kill-on-close) + `CREATE_NO_WINDOW` | `kill_on_drop(true)` (already set) + `prctl(PR_SET_PDEATHSIG, SIGTERM)` in `pre_exec`, so a SIGKILLed GUI cannot orphan the core. Binary name `aether`, pt helper names without `.exe`. |
| 12 | Integrity pins | Compile-time SHA-256 of the shipped `.exe`s | Same model: archive digests pinned in `scripts/linux-pins.json`; the fetch step verifies the archive, extracts, and **generates** the binary pins (`src-tauri/linux-binary-pins.json`, consumed by `build.rs`). Build refuses to run an unpinned core (fail-closed with instructions), exactly like Windows refuses a hash mismatch. |
| 13 | LAN sharing firewall | `netsh advfirewall` private-profile rule | No silent firewall edits on Linux. The listener bind itself is unchanged; if the user runs a firewall they open 1819 themselves (documented). LAN address discovery via primary-source-address lookup instead of PowerShell. |
| 14 | App picker (split tunneling) | Start-Menu `.lnk` COM enumeration (`app_picker.rs`), `.exe` filters | XDG `.desktop` enumeration (`app_picker_unix.rs`): name, resolved `Exec` path, PNG icon → base64 (same payload shape the frontend renders), system = under `/usr` or `/opt`. See limitation #1: enumeration works, per-app *enforcement* does not exist on Linux yet. |
| 15 | Quick-reconnect network fingerprint | `GetBestInterface` + MAC hash | Default interface from `/proc/net/route` + `/sys/class/net/<if>/address`, same hash construction; returns `None` (→ full scan) when it cannot name the link, same failure mode as Windows. |
| 16 | Daily connected-time clock | `GetLocalTime`/`GetTickCount64` | `localtime_r` via libc + `/proc/uptime` monotonic (also fixes the non-Windows stub that reported monotonic 0). |
| 17 | Self-updater (`update.rs`, `signature.rs`) | Downloads + Authenticode-verifies the Windows installer | Version check still queries the same release API and reports availability; download/install refuse with instructions to rebuild the Arch package. A Linux build must never fetch or execute a Windows installer. |
| 18 | Bundling (`tauri.conf.json`) | NSIS/MSI, `wintun.dll`, `pt/*.exe` resources | Base config untouched (Windows builds unchanged). `src-tauri/tauri.linux.conf.json` overrides for Linux: no wintun, Linux pt names, `.deb`/AppImage disabled in favour of `--no-bundle` + PKGBUILD layout. |

## 3. Linux networking design (as built)

Connect (VPN mode), in order:

1. GUI starts the Aether core **unprivileged** (as on Windows) and waits
   for its SOCKS5 handshake + HTTPS data-plane probe. No system state
   has changed yet.
2. GUI writes `request.json` (now including `corePid`) into
   `~/.local/share/io.github.hamvex.aether-gui/routing/<session>/`
   and runs `pkexec <aethon> --routing-helper <request.json>`.
   One polkit prompt per connection — UAC parity.
3. Root helper re-validates the request, re-anchors every path under
   the routing base it derives itself, takes the flock, sweeps stale
   Aethon state, creates `/sys/fs/cgroup/aethon`, installs
   `inet aethon` nftables marking for that cgroup, moves the core PID
   in, verifies Xray's hash, spawns Xray, moves Xray's PID in.
4. When `/sys/class/net/aethonN` appears: address/MTU/link-up,
   best-effort `nmcli device set aethonN managed no` (so
   NetworkManager leaves the foreign TUN alone), `resolvectl` DNS
   (best-effort, logged), then the two `ip rule`s + table-41840
   default. Readiness = `ip route get 1.1.1.1` resolves via the TUN
   for an unmarked process (the helper itself).
5. Supervision loop is the upstream one: `control.json` stop, GUI-PID
   death, Xray death, SOCKS health with kill-switch hold vs fail-open
   restore (unchanged semantics).
6. Teardown kills **only** its Xray child, then removes rules
   (exact-selector loop), flushes table 41840, `resolvectl revert`,
   deletes the TUN, deletes nft table `inet aethon`, moves cgroup
   members back to root and removes the cgroup. Nothing else on the
   system was ever written, so nothing else needs restoring.

IPv6: a v6 default is installed into table 41840 whenever the host
has IPv6, even when the core's upstream is v4-only — Xray's existing
`::/0 → block` rule then drops it inside the TUN instead of leaking
it around the tunnel (stricter than the Windows build, which only
installs v6 takeover routes when the upstream is v6-capable).

All helper subprocesses are fixed-argv `ip`/`nft`/`resolvectl`/`nmcli`
invocations. No string from a configuration file is ever interpolated
into a command: every variable argument (interface name, MTU, PIDs,
SOCKS address) is re-validated behind the privilege boundary
(interface name charset/length, MTU range, loopback SOCKS check are
all in `RoutingRequest::validate`).

## 4. Known limitations (deliberate, explicit)

1. **Per-application split tunneling (`split-include` /
   `split-exclude`) is refused on Linux** with a clear error at
   connect time. The Windows implementation depends on Xray's TUN
   process attribution, which does not exist on Linux. The honest
   Linux implementation is cgroup-based: a second fwmark cgroup whose
   membership is maintained by scanning `/proc/*/exe` in the helper's
   supervision loop. That is designed but **not implemented**, because
   it cannot be verified in this port's build environment and a
   half-working split tunnel silently misroutes traffic. `full` and
   `bypass-local` modes and CIDR route exclusions are fully supported.
2. **The experimental `tun2proxy` backend is Windows-only** (it needs
   Wintun) and is refused on Linux; the default Xray backend is used.
3. **Psiphon/Tor chains** use the upstream Linux `pt/` helpers from
   the official v2.3.0 archive. They are staged and the core env
   contract is identical, but chain bootstrap was **not live-verified**
   on Linux (see §6) — treat `*+psiphon` / `*+tor` as implemented but
   unverified until a run on a real machine is recorded.
4. **systemd-resolved is used only if it answers;** on systems
   without it, DNS still cannot leak in VPN mode (point 5 in §2), but
   per-link DNS is not programmed.
5. **x86_64 only**, matching the pinned sidecars. aarch64 sidecars
   exist upstream; adding them is a pins + PKGBUILD `arch=` change,
   not code.

## 5. Building

### Arch / Omarchy package (the supported path)

```bash
git clone https://github.com/BDadmehr0/ArtherGUI-linux.git
cd ArtherGUI-linux
makepkg -si            # builds in-tree, installs with pacman
```

`makepkg` downloads the two **pinned** upstream archives as package
sources (checksummed by `sha256sums` in the PKGBUILD), generates the
binary pins from the verified bytes, builds the Tauri app, and
installs:

- `/usr/bin/aethon` — GUI (never needs root)
- `/usr/lib/aethon/aether`, `/usr/lib/aethon/xray`, `/usr/lib/aethon/pt/`
- `/usr/share/applications/aethon.desktop`, hicolor icons
- `/usr/share/polkit-1/actions/io.github.hamvex.aether-gui.policy`
  (describes the routing-helper elevation prompt)

Runtime deps: `webkit2gtk-4.1 gtk3 libayatana-appindicator librsvg
polkit iproute2 nftables`. A polkit agent must be running in the
session (Omarchy starts one; plain Arch: e.g. `hyprpolkitagent`,
`polkit-gnome`). Kernel requirements: TUN support (`/dev/net/tun`,
standard), cgroup v2 unified hierarchy (Arch default), nftables.

### Source build without makepkg (development)

```bash
npm ci
npm run fetch:linux        # pinned download + pin generation
npm run dev                # tauri dev (needs the deps above + rust + nodejs)
# or: npm run build -- --no-bundle
```

`npm run fetch:linux` (`scripts/fetch-core-linux.sh`) refuses to
continue if either archive digest differs from
`scripts/linux-pins.json` — the same "digest comes from this
repository, not the server" rule as the Windows fetchers.

## 6. Verification record

This port was developed in a sandbox **without a Rust toolchain and
without package-registry access to install one**, and with the
GitHub release-asset CDN blocked (only `github.com`,
`codeload.github.com`, `api.github.com`, npm and PyPI were reachable).
Verification was therefore split:

Actually executed in the sandbox:

- `node tests/frontend.test.mjs` — upstream frontend suite, 38/38 on
  the imported baseline, re-run after every port commit (it also
  greps the Rust sources for the Windows invariants the port must not
  break).
- `bash -n` on all shell scripts and the PKGBUILD; `python3 -m
  json.tool` on all JSON configs.
- Upstream archive *existence and digests* confirmed through the
  GitHub API (`digest` field of the v2.3.0 / v26.3.27 assets).
- Full Rust *syntax* parse of every `.rs` file with the tree-sitter
  Rust grammar (zero errors in all ported files).

Defined for GitHub Actions but **not yet run** — the workflow lives
at `docs/ci/linux.yml.txt` because this environment's GitHub token
cannot create workflow files; activate it by copying it to
`.github/workflows/linux.yml` with a `workflows`-scoped token or the
GitHub web UI (upstream's `release.yml` is preserved the same way at
`docs/ci/upstream-release.yml.txt`). Until it runs green, treat the
Rust side as syntax-checked, not compiler-verified. It executes
(Arch Linux container, real compilation):

- `cargo test --manifest-path src-tauri/Cargo.toml --locked`
- `npm run fetch:linux` (real download → digest check → pins)
- `npm run build -- --no-bundle` (full Tauri release build)
- Sidecar smoke: `aether --version`-class invocation of the fetched
  binaries; GUI binary `--repair-network` with no state (must fail
  gracefully, touching nothing).

**Not executed anywhere** (needs the user's machine — credentials,
real uplink, root TUN): an end-to-end VPN connection, DNS-leak
behaviour under NetworkManager, kill-switch hold during a core
crash, Psiphon/Tor chain bootstrap, tray behaviour under
Hyprland/waybar. The "Manual first-run checklist" below is the
acceptance test for these; nothing in this document claims them done.

## 7. Manual first-run checklist (Arch / Omarchy)

```bash
aethon &                                    # GUI as your user
# Configurations → Device VPN → protocol of choice → Connect
# Expect ONE polkit prompt ("configure system-wide VPN routing").
ip rule show | grep -E '10000|10001'        # fwmark + table rules present
ip -4 route show table 41840               # default dev aethonN
ip route get 1.1.1.1                        # dev aethonN
sudo nft list table inet aethon             # cgroup marking chain
cat /sys/fs/cgroup/aethon/cgroup.procs      # core + xray PIDs
resolvectl status aethon0                   # DNS 1.1.1.1, ~. (name may differ)
curl -s https://www.cloudflare.com/cdn-cgi/trace | grep -E 'ip=|loc=|warp='
# Disconnect in the GUI, then ALL of these must be empty/absent:
ip rule show | grep -E '10000|10001'; ip route show table 41840
sudo nft list table inet aethon             # "No such file or directory"
ls /sys/class/net | grep aethon             # nothing
```

Failure recovery: if the GUI is killed mid-connection, the helper
notices the dead GUI PID and tears down by itself. If both die
(power loss), interfaces/rules do not survive reboot (nothing is
persistent), and within a session `pkexec aethon --repair-network`
(or Diagnostics → Repair Network) sweeps leftover Aethon state.

## 8. Diagnostics

- GUI: Settings/Diagnostics view shows live core log (sanitised),
  connection probes, and Repair Network.
- Files: `~/.local/share/io.github.hamvex.aether-gui/`
  - `core.log` — rotating Aether core log (credentials redacted
    before write, as on Windows)
  - `routing/<session>/routing.log` — Xray stdout/stderr for the
    session, `timeline.log` — helper stage trace, `status.json`
- Never paste `aether.toml` / `*-lastconn.toml` from that directory
  into a bug report: they hold WireGuard identity material.

## 9. Risks & acceptance criteria

Risks: (R1) Xray TUN behaviour drift between its Windows and Linux
builds (same pinned version mitigates); (R2) NetworkManager fighting
the foreign TUN (mitigated with `managed no`; worst case it logs and
the connection still routes by rules); (R3) polkit agent absent on
minimal Arch installs (connect fails with a clear error, manual
SOCKS mode keeps working); (R4) cgroup v1/hybrid systems (helper
fails closed with "requires cgroup v2" — Arch/Omarchy default is v2).

Acceptance = CI green (unit tests + full build + smokes), frontend
suite green, `makepkg` succeeds on Arch, and the §7 checklist passes
on a real machine including empty post-disconnect state.
