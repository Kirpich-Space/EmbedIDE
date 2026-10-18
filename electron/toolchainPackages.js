/**
 * Pinned toolchain package URLs for EmbedIDE (shared by CLI fetch + runtime installer).
 */
function platformKey(platform = process.platform, arch = process.arch) {
  const a = arch === 'arm64' ? 'arm64' : 'x64'
  if (platform === 'win32') return `win-${a}`
  if (platform === 'darwin') return `darwin-${a}`
  return `linux-${a}`
}

const PACKAGES = {
  'linux-x64': {
    gcc: {
      url: 'https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-linux-x64.tar.gz',
      strip: 1,
    },
    openocd: {
      url: 'https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-linux-x64.tar.gz',
      strip: 1,
    },
    zig: {
      url: 'https://ziglang.org/download/0.13.0/zig-linux-x86_64-0.13.0.tar.xz',
      strip: 1,
      renameBin: true,
    },
  },
  'linux-arm64': {
    gcc: {
      url: 'https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-linux-arm64.tar.gz',
      strip: 1,
    },
    openocd: {
      url: 'https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-linux-arm64.tar.gz',
      strip: 1,
    },
    zig: {
      url: 'https://ziglang.org/download/0.13.0/zig-linux-aarch64-0.13.0.tar.xz',
      strip: 1,
      renameBin: true,
    },
  },
  'win-x64': {
    gcc: {
      url: 'https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-win32-x64.zip',
      strip: 1,
    },
    openocd: {
      url: 'https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-win32-x64.zip',
      strip: 1,
    },
    zig: {
      url: 'https://ziglang.org/download/0.13.0/zig-windows-x86_64-0.13.0.zip',
      strip: 1,
      renameBin: true,
    },
  },
  'darwin-x64': {
    gcc: {
      url: 'https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-darwin-x64.tar.gz',
      strip: 1,
    },
    openocd: {
      url: 'https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-darwin-x64.tar.gz',
      strip: 1,
    },
    zig: {
      url: 'https://ziglang.org/download/0.13.0/zig-macos-x86_64-0.13.0.tar.xz',
      strip: 1,
      renameBin: true,
    },
  },
  'darwin-arm64': {
    gcc: {
      url: 'https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-darwin-arm64.tar.gz',
      strip: 1,
    },
    openocd: {
      url: 'https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-darwin-arm64.tar.gz',
      strip: 1,
    },
    zig: {
      url: 'https://ziglang.org/download/0.13.0/zig-macos-aarch64-0.13.0.tar.xz',
      strip: 1,
      renameBin: true,
    },
  },
}

module.exports = { PACKAGES, platformKey }
