# Aethon VPN - Linux port of hamvex/AetherGUI (Tauri v2 / Rust) for Arch Linux / Omarchy.
#
# Build & install from this directory:
#   makepkg -si
#
# What the package contains:
#   /usr/bin/aethon                       GUI; runs unprivileged. For VPN mode it
#                                         re-executes itself through pkexec
#                                         (--routing-helper) - one polkit prompt
#                                         per connection, like the Windows UAC prompt.
#   /usr/lib/aethon/aether                official Aether core v2.3.0 (Linux build)
#   /usr/lib/aethon/xray                  official Xray v26.3.27 (Linux build)
#   /usr/lib/aethon/pt/                   Psiphon/Lyrebird helpers from the core archive
#   desktop launcher, hicolor icons, polkit action description, licenses.
#
# The two upstream archives are makepkg sources pinned by sha256sums below
# (mirrored in scripts/linux-pins.json, from the GitHub release API). The fetch
# script re-verifies them and generates the binary digests build.rs bakes in;
# the installed app re-verifies both binaries before every exec.
#
# NOTE: the -git source below tracks the Linux port branch. When the port is
# merged to main, change #branch= accordingly (or pin a tag).
pkgname=aethon
pkgver=2.2.0
pkgrel=1
pkgdesc="Aethon VPN - Linux client for the Aether networking core (Tauri/Rust GUI)"
arch=('x86_64')
url="https://github.com/BDadmehr0/ArtherGUI-linux"
license=('AGPL-3.0-only' 'MPL-2.0')
depends=('webkit2gtk-4.1' 'gtk3' 'libayatana-appindicator' 'librsvg'
         'polkit' 'iproute2' 'nftables')
makedepends=('git' 'rust' 'cargo' 'nodejs' 'npm')
optdepends=('hyprpolkitagent: polkit agent for Hyprland/Omarchy sessions'
            'polkit-gnome: polkit agent for other desktops')
source=("$pkgname::git+https://github.com/BDadmehr0/ArtherGUI-linux.git#branch=arena/e23570a6-arthergui-linux"
        "aether-linux-x86_64.tar.gz::https://github.com/CluvexStudio/Aether/releases/download/v2.3.0/aether-linux-x86_64.tar.gz"
        "Xray-linux-64.zip::https://github.com/XTLS/Xray-core/releases/download/v26.3.27/Xray-linux-64.zip")
noextract=('aether-linux-x86_64.tar.gz' 'Xray-linux-64.zip')
sha256sums=('SKIP'
            '77c6ce41695a616660d834b55be235c17e7588a474dfd737b71ca80b685bdc8a'
            '23cd9af937744d97776ee35ecad4972cf4b2109d1e0fe6be9930467608f7c8ae')

build() {
    cd "$srcdir/$pkgname"
    export AETHER_LINUX_ARCHIVE="$srcdir/aether-linux-x86_64.tar.gz"
    export XRAY_LINUX_ARCHIVE="$srcdir/Xray-linux-64.zip"
    npm ci --no-audit --no-fund
    npm run fetch:linux
    # Re-export the generated binary pins so build.rs pins exactly the bytes
    # staged from the sha256sums-verified archives above.
    export AETHER_LINUX_SHA256="$(node -p "require('./src-tauri/linux-binary-pins.json').aether")"
    export XRAY_LINUX_SHA256="$(node -p "require('./src-tauri/linux-binary-pins.json').xray")"
    npm run build -- --no-bundle
}

check() {
    cd "$srcdir/$pkgname"
    npm test
    cargo test --manifest-path src-tauri/Cargo.toml --locked
}

package() {
    cd "$srcdir/$pkgname"

    install -Dm755 src-tauri/target/release/aether-gui "$pkgdir/usr/bin/aethon"

    install -Dm755 src-tauri/binaries/aether-x86_64-unknown-linux-gnu "$pkgdir/usr/lib/aethon/aether"
    install -Dm755 src-tauri/binaries/xray-x86_64-unknown-linux-gnu    "$pkgdir/usr/lib/aethon/xray"
    install -Dm755 src-tauri/binaries/pt/psiphon-tunnel-core           "$pkgdir/usr/lib/aethon/pt/psiphon-tunnel-core"
    install -Dm755 src-tauri/binaries/pt/lyrebird                      "$pkgdir/usr/lib/aethon/pt/lyrebird"

    install -Dm644 packaging/aethon.desktop \
        "$pkgdir/usr/share/applications/aethon.desktop"
    install -Dm644 src-tauri/icons/32x32.png \
        "$pkgdir/usr/share/icons/hicolor/32x32/apps/aethon.png"
    install -Dm644 src-tauri/icons/128x128.png \
        "$pkgdir/usr/share/icons/hicolor/128x128/apps/aethon.png"
    install -Dm644 src-tauri/icons/128x128@2x.png \
        "$pkgdir/usr/share/icons/hicolor/256x256/apps/aethon.png"
    install -Dm644 src-tauri/icons/icon.svg \
        "$pkgdir/usr/share/icons/hicolor/scalable/apps/aethon.svg"

    install -Dm644 packaging/io.github.hamvex.aether-gui.policy \
        "$pkgdir/usr/share/polkit-1/actions/io.github.hamvex.aether-gui.policy"

    # Licenses: this GUI and the Aether core (AGPL-3.0), Xray (MPL-2.0),
    # plus the core archive's THIRD-PARTY notices for its pt helpers.
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
    install -Dm644 third-party/xray-LICENSE.txt "$pkgdir/usr/share/licenses/$pkgname/xray-LICENSE.txt"
    if [ -d src-tauri/binaries/aether-licenses ]; then
        mkdir -p "$pkgdir/usr/share/licenses/$pkgname/aether"
        cp -r src-tauri/binaries/aether-licenses/. "$pkgdir/usr/share/licenses/$pkgname/aether/"
    fi
}
