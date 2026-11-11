#!/usr/bin/env bash
# Fetch, verify, and stage the Linux Aether core + Xray routing engine.
#
# Linux counterpart of scripts/fetch-aether.ps1 + scripts/fetch-xray.ps1 and
# bound by the same rule: the expected digests come from this repository
# (scripts/linux-pins.json), never from a checksum file served beside the
# archive. After the archive check, the extracted payload digests are
# recorded in src-tauri/linux-binary-pins.json, which build.rs turns into
# the compile-time pins the application refuses to run without.
#
# Offline/package builds: set AETHER_LINUX_ARCHIVE / XRAY_LINUX_ARCHIVE to
# already-downloaded files (the PKGBUILD does this with makepkg-verified
# sources). They are still re-verified against the pins here.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pins="$repo_root/scripts/linux-pins.json"
bin_dir="$repo_root/src-tauri/binaries"
pins_out="$repo_root/src-tauri/linux-binary-pins.json"
triple="x86_64-unknown-linux-gnu"

pin() { node -e 'const p=require(process.argv[1]); process.stdout.write(p[process.argv[2]][process.argv[3]])' "$pins" "$1" "$2"; }

aether_url="$(pin aether url)"; aether_sha="$(pin aether sha256)"; aether_name="$(pin aether archive)"
xray_url="$(pin xray url)";     xray_sha="$(pin xray sha256)";     xray_name="$(pin xray archive)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fetch() { # fetch <url> <dest> <env-archive-or-empty> <label>
    local url="$1" dest="$2" preset="${3:-}" label="$4"
    if [ -n "$preset" ]; then
        [ -f "$preset" ] || { echo "Preset $label archive not found: $preset" >&2; exit 1; }
        cp "$preset" "$dest"
        echo "Using preset $label archive: $preset"
    else
        echo "Downloading $label: $url"
        curl -fL --retry 3 -o "$dest" "$url"
    fi
}

verify() { # verify <file> <expected> <label>
    local actual
    actual="$(sha256sum "$1" | cut -d' ' -f1)"
    if [ "$actual" != "$2" ]; then
        echo "$3 archive SHA-256 mismatch: expected $2, got $actual (refusing to use it)" >&2
        exit 1
    fi
    echo "Verified $3 archive SHA-256: $actual"
}

fetch "$aether_url" "$work/$aether_name" "${AETHER_LINUX_ARCHIVE:-}" "Aether core"
verify "$work/$aether_name" "$aether_sha" "Aether core"
fetch "$xray_url" "$work/$xray_name" "${XRAY_LINUX_ARCHIVE:-}" "Xray"
verify "$work/$xray_name" "$xray_sha" "Xray"

mkdir -p "$work/aether" "$work/xray" "$bin_dir/pt"
tar -xzf "$work/$aether_name" -C "$work/aether"
if command -v unzip >/dev/null 2>&1; then
    unzip -q -o "$work/$xray_name" -d "$work/xray"
else
    python3 -c 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])' "$work/$xray_name" "$work/xray"
fi

aether_bin="$(find "$work/aether" -maxdepth 2 -type f -name aether | head -n1)"
xray_bin="$(find "$work/xray" -maxdepth 2 -type f -name xray | head -n1)"
[ -n "$aether_bin" ] || { echo "aether binary not found in the verified archive" >&2; exit 1; }
[ -n "$xray_bin" ] || { echo "xray binary not found in the verified archive" >&2; exit 1; }

install -m 0755 "$aether_bin" "$bin_dir/aether-$triple"
install -m 0755 "$xray_bin" "$bin_dir/xray-$triple"
for helper in psiphon-tunnel-core lyrebird; do
    found="$(find "$work/aether" -path "*/pt/$helper" -type f | head -n1)"
    [ -n "$found" ] || { echo "pt/$helper not found in the verified Aether archive" >&2; exit 1; }
    install -m 0755 "$found" "$bin_dir/pt/$helper"
done

# Keep the upstream license bundle beside the staged binaries; packagers
# install it (PKGBUILD -> /usr/share/licenses/aethon/).
licenses="$(find "$work/aether" -maxdepth 2 -type d -name LICENSES | head -n1)"
if [ -n "$licenses" ]; then
    rm -rf "$bin_dir/aether-licenses"
    cp -r "$licenses" "$bin_dir/aether-licenses"
fi
[ -f "$work/xray/LICENSE" ] && cp "$work/xray/LICENSE" "$repo_root/third-party/xray-LICENSE.txt"

digest() { sha256sum "$1" | cut -d' ' -f1; }
cat > "$pins_out" <<EOF
{
  "aether": "$(digest "$bin_dir/aether-$triple")",
  "xray": "$(digest "$bin_dir/xray-$triple")",
  "pt": {
    "psiphon-tunnel-core": "$(digest "$bin_dir/pt/psiphon-tunnel-core")",
    "lyrebird": "$(digest "$bin_dir/pt/lyrebird")"
  }
}
EOF

echo "Staged verified sidecars under $bin_dir"
echo "Wrote build-time pins to $pins_out:"
cat "$pins_out"
