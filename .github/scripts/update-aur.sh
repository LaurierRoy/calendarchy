#!/bin/bash
set -euo pipefail

VERSION="$1"
SOURCE_SHA="$2"
LINUX_SHA="$3"

# --- AUR source package (calendarchy) ---
git clone ssh://aur@aur.archlinux.org/calendarchy.git /tmp/aur-source

cat > /tmp/aur-source/PKGBUILD << EOF
# Maintainer: Serge Ovanesyan
pkgname=calendarchy
pkgver=${VERSION}
pkgrel=1
pkgdesc='Terminal calendar app for Google Calendar and iCloud'
arch=('x86_64')
url='https://github.com/sovanesyan/calendarchy'
license=('MIT')
makedepends=('cargo')
options=(!lto)
source=("\$pkgname-\$pkgver.tar.gz::\$url/archive/v\$pkgver.tar.gz")
sha256sums=('${SOURCE_SHA}')

prepare() {
  cd "\$pkgname-\$pkgver"
  export RUSTUP_TOOLCHAIN=stable
  cargo fetch --locked --target "\$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "\$pkgname-\$pkgver"
  export RUSTUP_TOOLCHAIN=stable
  export CARGO_TARGET_DIR=target
  cargo build --frozen --release
}

package() {
  cd "\$pkgname-\$pkgver"
  install -Dm755 "target/release/\$pkgname" "\$pkgdir/usr/bin/\$pkgname"
  install -Dm644 LICENSE "\$pkgdir/usr/share/licenses/\$pkgname/LICENSE"
}
EOF

cat > /tmp/aur-source/.SRCINFO << EOF
pkgbase = calendarchy
	pkgdesc = Terminal calendar app for Google Calendar and iCloud
	pkgver = ${VERSION}
	pkgrel = 1
	url = https://github.com/sovanesyan/calendarchy
	arch = x86_64
	license = MIT
	makedepends = cargo
	options = !lto
	source = calendarchy-${VERSION}.tar.gz::https://github.com/sovanesyan/calendarchy/archive/v${VERSION}.tar.gz
	sha256sums = ${SOURCE_SHA}

pkgname = calendarchy
EOF

cd /tmp/aur-source
git add PKGBUILD .SRCINFO
git commit -m "Update to v${VERSION}" || true
git push

# --- AUR binary package (calendarchy-bin) ---
git clone ssh://aur@aur.archlinux.org/calendarchy-bin.git /tmp/aur-bin

cat > /tmp/aur-bin/PKGBUILD << EOF
# Maintainer: Serge Ovanesyan
pkgname=calendarchy-bin
pkgver=${VERSION}
pkgrel=1
pkgdesc='Terminal calendar app for Google Calendar and iCloud'
arch=('x86_64')
url='https://github.com/sovanesyan/calendarchy'
license=('MIT')
provides=('calendarchy')
conflicts=('calendarchy')
source=("\$pkgname-\$pkgver.tar.gz::\$url/releases/download/v\$pkgver/calendarchy-x86_64-unknown-linux-gnu.tar.gz")
sha256sums=('${LINUX_SHA}')

package() {
  install -Dm755 calendarchy "\$pkgdir/usr/bin/calendarchy"
  install -Dm644 LICENSE "\$pkgdir/usr/share/licenses/\$pkgname/LICENSE"
}
EOF

cat > /tmp/aur-bin/.SRCINFO << EOF
pkgbase = calendarchy-bin
	pkgdesc = Terminal calendar app for Google Calendar and iCloud
	pkgver = ${VERSION}
	pkgrel = 1
	url = https://github.com/sovanesyan/calendarchy
	arch = x86_64
	license = MIT
	provides = calendarchy
	conflicts = calendarchy
	source = calendarchy-bin-${VERSION}.tar.gz::https://github.com/sovanesyan/calendarchy/releases/download/v${VERSION}/calendarchy-x86_64-unknown-linux-gnu.tar.gz
	sha256sums = ${LINUX_SHA}

pkgname = calendarchy-bin
EOF

cd /tmp/aur-bin
git add PKGBUILD .SRCINFO
git commit -m "Update to v${VERSION}" || true
git push
