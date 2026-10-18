# Copyright 2026 Kirpich Space
# Distributed under the terms of the MIT license

EAPI=8

DESCRIPTION="Offline-first engineering IDE for Cortex-M firmware"
HOMEPAGE="https://github.com/Kirpich-Space/EmbedIDE"
SRC_URI="https://github.com/Kirpich-Space/EmbedIDE/releases/download/v${PV}/embed-ide-${PV}-linux-x64.tar.gz -> ${P}-linux-x64.tar.gz"

LICENSE="MIT"
SLOT="0"
KEYWORDS="~amd64"
IUSE=""

RDEPEND="
	x11-libs/libX11
	x11-libs/libXrandr
	x11-libs/libXdamage
	x11-libs/libXcomposite
	x11-libs/libXcursor
	x11-libs/libXfixes
	x11-libs/libXi
	media-libs/mesa
	dev-libs/nss
	media-libs/alsa-lib
	x11-libs/gtk+:3
"
DEPEND=""
BDEPEND=""

QA_PREBUILT="*"

S="${WORKDIR}"

src_install() {
	dodir /opt/embed-ide
	# Unpack layout from electron-builder tar.gz
	cp -a "${S}"/. "${ED}/opt/embed-ide/" || die

	dobin "${FILESDIR}/embedide" 2>/dev/null || {
		newbin - embedide <<'EOF'
#!/bin/sh
exec /opt/embed-ide/embed-ide "$@"
EOF
	}

	domenu "${FILESDIR}/embedide.desktop" 2>/dev/null || {
		make_desktop_entry embedide EmbedIDE Development
	}
}

pkg_postinst() {
	elog "EmbedIDE ${PV} installed to /opt/embed-ide"
	elog "Run: embedide"
	elog "Alternatively use the AppImage from GitHub Releases on any glibc Linux."
}
