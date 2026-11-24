//! Downloadable toolchains.
//!
//! EmbedIDE never picks or fetches a compiler on its own initiative. A toolchain is downloaded only
//! when **the user's own configuration** names it, and only from an entry that pins the exact
//! archive: an `https://` URL and its SHA-256. A download whose hash differs is discarded before
//! anything is unpacked. Archives are unpacked with path checks (no absolute paths, no `..`, no
//! symlinks leaving the install directory, no setuid bits) into a private per-user directory; no
//! installer, script or package manager is ever run, and no root rights are needed.
//!
//! ```toml
//! # config.toml
//! [toolchains]
//! require = ["zig", "flang"]          # make sure these exist on this machine
//!
//! [toolchains.install.flang]          # your own entry, or a pinned built-in one (see `builtin`)
//! url = "https://example.org/flang-20.1.0-linux-x86_64.tar.xz"
//! sha256 = "0123…"                    # 64 hex digits, mandatory
//! strip = 1                           # leading path components to drop (like tar --strip-components)
//! bin = ["bin"]                       # directories with executables (searched like PATH)
//! provides = ["flang"]                # toolchain ids that mean "already installed on the machine"
//! ```

use ring::digest;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Current stage of a toolchain archive installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Connecting to the download server.
    Connecting,
    /// Streaming archive bytes while computing SHA-256.
    Downloading,
    /// Verifying the archive SHA-256 digest.
    Verifying,
    /// Unpacking the verified archive into the private toolchain directory.
    Unpacking,
}

/// Live progress update emitted by [`install_with_progress`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// Toolchain entry name.
    pub name: String,
    /// Current installation stage.
    pub phase: Phase,
    /// Archive bytes received so far.
    pub downloaded: u64,
    /// Total archive size from `Content-Length`, if known.
    pub total: Option<u64>,
}

/// Hard limit on a download (bytes).
pub const MAX_DOWNLOAD: u64 = 4 << 30;
/// Hard limit on a decompressed xz tar stream (bytes).
const MAX_DECOMPRESSED: u64 = 8 << 30;

struct LimitedWriter<W> {
    inner: W,
    written: u64,
    limit: u64,
}
impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let next = self.written.saturating_add(buf.len() as u64);
        if next > self.limit {
            return Err(std::io::Error::other(format!(
                "decompressed archive exceeded the {} GiB limit",
                self.limit >> 30
            )));
        }
        let n = self.inner.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// File written into a finished install; its presence means the directory is complete.
const MARKER: &str = ".embedide-installed";

/// One downloadable toolchain: where it comes from and how to use it once unpacked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallSpec {
    /// `https://` address of the archive.
    pub url: String,
    /// Expected SHA-256 of the archive, 64 hex digits. Mandatory.
    pub sha256: String,
    /// `tar.gz`, `tar.xz` or `tar`; inferred from the URL when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
    /// Leading path components dropped while unpacking.
    #[serde(default)]
    pub strip: usize,
    /// Directories (relative to the install directory) with executables; `["bin", "."]` when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bin: Vec<String>,
    /// Toolchain ids that show the compiler is already available (e.g. `["gfortran"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provides: Vec<String>,
}

/// Supported archive formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Tar,
    TarGz,
    TarXz,
}

impl InstallSpec {
    fn format(&self) -> Result<Format, String> {
        let name = self
            .archive
            .clone()
            .unwrap_or_else(|| self.url.clone())
            .to_ascii_lowercase();
        let name = name.split(['?', '#']).next().unwrap_or("");
        if name.ends_with("tar.gz") || name.ends_with(".tgz") {
            Ok(Format::TarGz)
        } else if name.ends_with("tar.xz") || name.ends_with(".txz") {
            Ok(Format::TarXz)
        } else if name.ends_with(".tar") {
            Ok(Format::Tar)
        } else {
            Err("unsupported archive format; use tar.gz, tar.xz or tar (set `archive` if the URL has no extension)".into())
        }
    }
}

/// Check a toolchain name: it becomes part of a directory name.
pub fn validate_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "'{name}' is not a valid toolchain name; use letters, digits, '-', '_' and '.' (at most 64)"
        ))
    }
}

/// Validate an entry without touching the network.
pub fn validate_spec(name: &str, spec: &InstallSpec) -> Result<(), String> {
    validate_name(name)?;
    let ctx = |m: &str| format!("toolchains.install.{name}: {m}");
    if !spec.url.starts_with("https://")
        || spec.url.len() < 12
        || spec.url.contains(char::is_whitespace)
    {
        return Err(ctx("url must be an https:// address"));
    }
    if spec.sha256.len() != 64 || !spec.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ctx(
            "sha256 must be exactly 64 hex digits (the archive's SHA-256; it cannot be omitted)",
        ));
    }
    spec.format().map_err(|e| ctx(&e))?;
    if spec.strip > 8 {
        return Err(ctx("strip must be at most 8"));
    }
    for b in &spec.bin {
        let p = Path::new(b);
        if p.is_absolute()
            || p.components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(ctx(&format!(
                "bin entry '{b}' must be a relative path inside the install directory"
            )));
        }
    }
    Ok(())
}

/// Entries shipped with EmbedIDE. Only archives whose checksum the maintainers verified are listed;
/// everything else is described by the user's own `[toolchains.install.*]` entries.
pub fn builtin() -> BTreeMap<String, InstallSpec> {
    let mut m = BTreeMap::new();
    m.insert(
        "zig".to_string(),
        InstallSpec {
            // https://ziglang.org/download/index.json
            url: "https://ziglang.org/download/0.17.1/zig-x86_64-linux-0.17.1.tar.xz".into(),
            sha256: "3581863e3cbcfd0fce0efc682ce7e9e93da96668a977791d356a26d2a7e9b17a".into(),
            archive: None,
            strip: 1,
            bin: vec![".".into()],
            provides: vec!["zig-cc".into(), "zig-c++".into(), "zig".into()],
        },
    );
    m
}

/// Built-in entries overridden by the user's entries of the same name.
pub fn merged(user: &BTreeMap<String, InstallSpec>) -> BTreeMap<String, InstallSpec> {
    let mut all = builtin();
    all.extend(user.iter().map(|(k, v)| (k.clone(), v.clone())));
    all
}

/// The per-user directory where toolchains are unpacked (`$EMBEDIDE_DATA_DIR/toolchains`,
/// `$XDG_DATA_HOME/embedide/toolchains` or `~/.local/share/embedide/toolchains`).
pub fn install_root() -> Option<PathBuf> {
    let base = if let Some(d) = std::env::var_os("EMBEDIDE_DATA_DIR").filter(|d| !d.is_empty()) {
        PathBuf::from(d)
    } else if let Some(d) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        PathBuf::from(d).join("embedide")
    } else {
        PathBuf::from(std::env::var_os("HOME").filter(|d| !d.is_empty())?)
            .join(".local/share/embedide")
    };
    base.is_absolute().then(|| base.join("toolchains"))
}

/// Directory a given entry is (or will be) unpacked into. It includes part of the hash, so a changed
/// entry installs next to the old one instead of mixing files.
pub fn install_dir(name: &str, spec: &InstallSpec) -> Option<PathBuf> {
    Some(install_root()?.join(format!(
        "{name}-{}",
        &spec.sha256.to_ascii_lowercase()[..12]
    )))
}

/// Whether the entry is completely installed.
pub fn is_installed(name: &str, spec: &InstallSpec) -> bool {
    install_dir(name, spec).is_some_and(|d| d.join(MARKER).is_file())
}

/// Executable directories of an installed entry that exist on disk.
pub fn bin_dirs(name: &str, spec: &InstallSpec) -> Vec<PathBuf> {
    let Some(root) = install_dir(name, spec).filter(|d| d.join(MARKER).is_file()) else {
        return Vec::new();
    };
    let rels: Vec<&str> = if spec.bin.is_empty() {
        vec!["bin", "."]
    } else {
        spec.bin.iter().map(String::as_str).collect()
    };
    rels.into_iter()
        .map(|r| root.join(r))
        .filter(|p| p.is_dir())
        .collect()
}

/// Search directories contributed by every installed entry.
pub fn managed_search_dirs(specs: &BTreeMap<String, InstallSpec>) -> Vec<PathBuf> {
    specs.iter().flat_map(|(n, s)| bin_dirs(n, s)).collect()
}

/// Which of `names` still need installing, given the toolchain ids already detected on the machine.
/// Unknown names are an error: the message says where to define them.
pub fn missing<'a>(
    names: &'a [String],
    specs: &BTreeMap<String, InstallSpec>,
    detected_ids: &[&str],
) -> Result<Vec<(&'a str, InstallSpec)>, String> {
    let mut out = Vec::new();
    for n in names {
        let Some(spec) = specs.get(n) else {
            let known: Vec<&str> = specs.keys().map(String::as_str).collect();
            return Err(format!(
                "Toolchain '{n}' is required but has no [toolchains.install.{n}] entry in config.toml \
                 (known entries: {}). Add one with the archive's https URL and SHA-256.",
                if known.is_empty() { "none".into() } else { known.join(", ") }
            ));
        };
        validate_spec(n, spec)?;
        if is_installed(n, spec)
            || spec
                .provides
                .iter()
                .any(|p| detected_ids.contains(&p.as_str()))
        {
            continue;
        }
        out.push((n.as_str(), spec.clone()));
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Removes the temporary directory however the install ends.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn io_err(what: &str, e: impl std::fmt::Display) -> String {
    format!("{what}: {e}")
}

/// Download, verify and unpack `spec` into [`install_dir`]. `progress` receives short status lines.
/// Returns the install directory. Already-installed entries return immediately.
pub fn install(
    name: &str,
    spec: &InstallSpec,
    progress: &mut dyn FnMut(&str),
) -> Result<PathBuf, String> {
    let mut next_mib_report = 0u64;
    let dest = install_with_progress(name, spec, &mut |p| match p.phase {
        Phase::Connecting => progress(&format!("Downloading {name} from {}", spec.url)),
        Phase::Downloading => {
            if p.downloaded >= next_mib_report {
                match p.total {
                    Some(t) if t > 0 => progress(&format!(
                        "  {name}: {} of {} MiB ({}%)",
                        p.downloaded >> 20,
                        t >> 20,
                        p.downloaded * 100 / t
                    )),
                    _ => progress(&format!("  {name}: {} MiB", p.downloaded >> 20)),
                }
                next_mib_report = p.downloaded + (8 << 20);
            }
        }
        Phase::Verifying => progress(&format!("Verifying SHA-256 of {name}")),
        Phase::Unpacking => progress(&format!("Verified SHA-256 of {name}; unpacking")),
    })?;
    progress(&format!("Installed {name} into {}", dest.display()));
    Ok(dest)
}

/// Download, verify and unpack `spec` into [`install_dir`], emitting structured byte-level
/// [`Progress`] updates for live progress bars and animations.
pub fn install_with_progress(
    name: &str,
    spec: &InstallSpec,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, String> {
    validate_spec(name, spec)?;
    let dest = install_dir(name, spec).ok_or(
        "Cannot determine the install directory; set EMBEDIDE_DATA_DIR to an absolute path",
    )?;
    if dest.join(MARKER).is_file() {
        return Ok(dest);
    }
    if std::env::var_os("EMBEDIDE_OFFLINE").is_some_and(|v| !v.is_empty() && v != "0") {
        return Err(format!(
            "'{name}' needs downloading, but EMBEDIDE_OFFLINE is set; unset it or install the toolchain yourself"
        ));
    }
    let root = dest.parent().ok_or("invalid install directory")?;
    fs::create_dir_all(root)
        .map_err(|e| io_err(&format!("Cannot create {}", root.display()), e))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let scratch = Scratch(root.join(format!(".tmp-{name}-{}-{stamp}", std::process::id())));
    fs::create_dir_all(&scratch.0).map_err(|e| io_err("Cannot create a scratch directory", e))?;
    let archive_path = scratch.0.join("archive");

    // 1. Download while hashing.
    on_progress(Progress {
        name: name.to_string(),
        phase: Phase::Connecting,
        downloaded: 0,
        total: None,
    });
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .redirects(5)
        .user_agent("embedide-toolchain-installer")
        .build();
    let resp = agent
        .get(&spec.url)
        .call()
        .map_err(|e| format!("Download of {} failed: {e}", spec.url))?;
    let total: Option<u64> = resp.header("Content-Length").and_then(|v| v.parse().ok());
    if total.is_some_and(|t| t > MAX_DOWNLOAD) {
        return Err(format!(
            "{} is larger than the {} GiB limit",
            spec.url,
            MAX_DOWNLOAD >> 30
        ));
    }
    let mut reader = resp.into_reader();
    let mut out = BufWriter::new(
        File::create(&archive_path).map_err(|e| io_err("Cannot create the download file", e))?,
    );
    let mut ctx = digest::Context::new(&digest::SHA256);
    let mut buf = vec![0u8; 64 * 1024];
    let mut got = 0u64;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        got += n as u64;
        if got > MAX_DOWNLOAD {
            return Err(format!(
                "Download exceeded the {} GiB limit",
                MAX_DOWNLOAD >> 30
            ));
        }
        ctx.update(&buf[..n]);
        out.write_all(&buf[..n])
            .map_err(|e| io_err("Cannot write the download", e))?;
        if last_emit.elapsed() >= Duration::from_millis(80) || total.is_some_and(|t| got >= t) {
            on_progress(Progress {
                name: name.to_string(),
                phase: Phase::Downloading,
                downloaded: got,
                total,
            });
            last_emit = Instant::now();
        }
    }
    out.flush()
        .map_err(|e| io_err("Cannot write the download", e))?;
    drop(out);

    // 2. Verify before anything is unpacked.
    on_progress(Progress {
        name: name.to_string(),
        phase: Phase::Verifying,
        downloaded: got,
        total: total.or(Some(got)),
    });
    let actual = hex(ctx.finish().as_ref());
    if !actual.eq_ignore_ascii_case(&spec.sha256) {
        return Err(format!(
            "SHA-256 mismatch for {}: expected {}, got {actual}. Nothing was installed.",
            spec.url, spec.sha256
        ));
    }
    on_progress(Progress {
        name: name.to_string(),
        phase: Phase::Unpacking,
        downloaded: got,
        total: total.or(Some(got)),
    });

    // 3. Unpack into the scratch directory, then move it into place.
    let unpacked = scratch.0.join("unpacked");
    fs::create_dir_all(&unpacked).map_err(|e| io_err("Cannot create the unpack directory", e))?;
    match spec.format()? {
        Format::Tar => unpack(
            BufReader::new(
                File::open(&archive_path).map_err(|e| io_err("Cannot reopen the download", e))?,
            ),
            &unpacked,
            spec.strip,
        )?,
        Format::TarGz => unpack(
            flate2::read::GzDecoder::new(BufReader::new(
                File::open(&archive_path).map_err(|e| io_err("Cannot reopen the download", e))?,
            )),
            &unpacked,
            spec.strip,
        )?,
        Format::TarXz => {
            // lzma-rs decompresses into a writer, so go through a temporary tar file.
            let tar_path = scratch.0.join("archive.tar");
            let mut input = BufReader::new(
                File::open(&archive_path).map_err(|e| io_err("Cannot reopen the download", e))?,
            );
            let mut tar_out = LimitedWriter {
                inner: BufWriter::new(
                    File::create(&tar_path).map_err(|e| io_err("Cannot create the tar file", e))?,
                ),
                written: 0,
                limit: MAX_DECOMPRESSED,
            };
            lzma_rs::xz_decompress(&mut input, &mut tar_out)
                .map_err(|e| format!("Cannot decompress the xz archive: {e}"))?;
            tar_out
                .flush()
                .map_err(|e| io_err("Cannot write the tar file", e))?;
            drop(tar_out);
            let _ = fs::remove_file(&archive_path);
            unpack(
                BufReader::new(
                    File::open(&tar_path).map_err(|e| io_err("Cannot reopen the tar file", e))?,
                ),
                &unpacked,
                spec.strip,
            )?;
        }
    }
    fs::write(
        unpacked.join(MARKER),
        format!(
            "name = \"{name}\"\nurl = \"{}\"\nsha256 = \"{}\"\n",
            spec.url,
            spec.sha256.to_ascii_lowercase()
        ),
    )
    .map_err(|e| io_err("Cannot write the install marker", e))?;
    if dest.exists() {
        fs::remove_dir_all(&dest)
            .map_err(|e| io_err(&format!("Cannot replace {}", dest.display()), e))?;
    }
    fs::rename(&unpacked, &dest).map_err(|e| {
        io_err(
            &format!("Cannot move the toolchain to {}", dest.display()),
            e,
        )
    })?;
    Ok(dest)
}

/// Remove an installed entry. Returns whether anything was removed.
pub fn uninstall(name: &str, spec: &InstallSpec) -> Result<bool, String> {
    let Some(dir) = install_dir(name, spec) else {
        return Ok(false);
    };
    if !dir.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(&dir).map_err(|e| io_err(&format!("Cannot remove {}", dir.display()), e))?;
    Ok(true)
}

/// Drop `strip` leading components; `Ok(None)` when nothing is left. Any component that is not a
/// plain name (absolute roots, `..`, drive prefixes) rejects the archive.
fn stripped(path: &Path, strip: usize) -> Result<Option<PathBuf>, String> {
    let mut names = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(n) => names.push(n.to_owned()),
            Component::CurDir => {}
            _ => {
                return Err(format!(
                    "Archive contains an unsafe path: {}",
                    path.display()
                ))
            }
        }
    }
    if names.len() <= strip {
        return Ok(None);
    }
    Ok(Some(names[strip..].iter().collect()))
}

/// A symlink may point anywhere inside the install directory, never outside it.
fn check_link(entry_rel: &Path, target: &Path) -> Result<(), String> {
    if target.is_absolute() {
        return Err(format!(
            "Archive contains an absolute symlink: {} -> {}",
            entry_rel.display(),
            target.display()
        ));
    }
    let mut depth = entry_rel.parent().map_or(0, |p| p.components().count()) as i64;
    for c in target.components() {
        match c {
            Component::ParentDir => depth -= 1,
            Component::Normal(_) => depth += 1,
            _ => {}
        }
        if depth < 0 {
            return Err(format!(
                "Archive contains a symlink that leaves the install directory: {} -> {}",
                entry_rel.display(),
                target.display()
            ));
        }
    }
    Ok(())
}

/// Verify that no existing ancestor component of `rel` inside `dest` is a symlink,
/// preventing multi-entry tar-slip escapes (`a/b -> ..`, then `a/b/esc -> ..`).
fn ensure_real_ancestors(dest: &Path, rel: &Path) -> Result<(), String> {
    let mut cur = dest.to_path_buf();
    let comps: Vec<_> = rel.components().collect();
    for c in &comps[..comps.len().saturating_sub(1)] {
        cur.push(c.as_os_str());
        match fs::symlink_metadata(&cur) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!(
                    "Archive entry {} traverses a symlink at {}",
                    rel.display(),
                    cur.display()
                ));
            }
            Ok(meta) if !meta.is_dir() => {
                return Err(format!(
                    "Archive entry {} has non-directory ancestor {}",
                    rel.display(),
                    cur.display()
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(io_err(&format!("Cannot inspect {}", cur.display()), e)),
        }
    }
    Ok(())
}

fn unpack<R: Read>(reader: R, dest: &Path, strip: usize) -> Result<(), String> {
    let mut archive = tar::Archive::new(reader);
    archive.set_preserve_permissions(true);
    archive.set_unpack_xattrs(false);
    let entries = archive
        .entries()
        .map_err(|e| format!("Cannot read the archive: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("Corrupt archive: {e}"))?;
        let raw = entry
            .path()
            .map_err(|e| format!("Corrupt archive path: {e}"))?
            .into_owned();
        let Some(rel) = stripped(&raw, strip)? else {
            continue;
        };
        ensure_real_ancestors(dest, &rel)?;
        let target = dest.join(&rel);
        let kind = entry.header().entry_type();
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| io_err(&format!("Cannot create {}", parent.display()), e))?;
        }
        if kind.is_dir() {
            if fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(format!(
                    "Archive directory {} conflicts with an existing symlink",
                    rel.display()
                ));
            }
            fs::create_dir_all(&target)
                .map_err(|e| io_err(&format!("Cannot create {}", target.display()), e))?;
        } else if kind.is_file() {
            if fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(format!(
                    "Archive file {} conflicts with an existing symlink",
                    rel.display()
                ));
            }
            let mode = entry.header().mode().unwrap_or(0o644);
            entry
                .unpack(&target)
                .map_err(|e| io_err(&format!("Cannot unpack {}", rel.display()), e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                // No setuid/setgid/sticky bits, and always readable by the owner.
                fs::set_permissions(&target, fs::Permissions::from_mode((mode & 0o777) | 0o600))
                    .map_err(|e| {
                        io_err(&format!("Cannot set permissions on {}", rel.display()), e)
                    })?;
            }
            #[cfg(not(unix))]
            let _ = mode;
        } else if kind.is_symlink() {
            let link = entry
                .link_name()
                .map_err(|e| io_err("Corrupt symlink", e))?
                .ok_or("Symlink without a target")?
                .into_owned();
            check_link(&rel, &link)?;
            #[cfg(unix)]
            {
                let _ = fs::remove_file(&target);
                std::os::unix::fs::symlink(&link, &target)
                    .map_err(|e| io_err(&format!("Cannot create symlink {}", rel.display()), e))?;
            }
        } else if kind.is_hard_link() {
            let link = entry
                .link_name()
                .map_err(|e| io_err("Corrupt hard link", e))?
                .ok_or("Hard link without a target")?
                .into_owned();
            let Some(src_rel) = stripped(&link, strip)? else {
                continue;
            };
            ensure_real_ancestors(dest, &src_rel)?;
            let src = dest.join(&src_rel);
            let src_meta = fs::symlink_metadata(&src).map_err(|e| {
                io_err(
                    &format!("Cannot inspect hard link target {}", src_rel.display()),
                    e,
                )
            })?;
            if src_meta.file_type().is_symlink() || !src_meta.is_file() {
                return Err(format!(
                    "Archive hard link {} does not point to a regular file",
                    rel.display()
                ));
            }
            let _ = fs::remove_file(&target);
            fs::hard_link(&src, &target)
                .or_else(|_| fs::copy(&src, &target).map(|_| ()))
                .map_err(|e| io_err(&format!("Cannot link {}", rel.display()), e))?;
        }
        // Everything else (devices, fifos, extended headers) is ignored.
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> InstallSpec {
        InstallSpec {
            url: "https://example.org/x/tool-1.tar.xz".into(),
            sha256: "ab".repeat(32),
            archive: None,
            strip: 1,
            bin: vec![],
            provides: vec!["tool".into()],
        }
    }

    #[test]
    fn specs_need_https_a_full_hash_and_a_known_archive() {
        assert!(validate_spec("tool", &spec()).is_ok());
        let mut s = spec();
        s.url = "http://example.org/a.tar.gz".into();
        assert!(validate_spec("tool", &s).unwrap_err().contains("https"));
        let mut s = spec();
        s.sha256 = "abc".into();
        assert!(validate_spec("tool", &s).unwrap_err().contains("64 hex"));
        let mut s = spec();
        s.url = "https://example.org/a.zip".into();
        assert!(validate_spec("tool", &s)
            .unwrap_err()
            .contains("unsupported archive"));
        s.archive = Some("tar.gz".into());
        assert!(validate_spec("tool", &s).is_ok());
        let mut s = spec();
        s.bin = vec!["../x".into()];
        assert!(validate_spec("tool", &s)
            .unwrap_err()
            .contains("relative path"));
        assert!(validate_name("../evil").is_err() && validate_name("ok-1.2_x").is_ok());
    }

    #[test]
    fn builtin_entries_are_valid() {
        for (n, s) in builtin() {
            validate_spec(&n, &s).unwrap();
        }
    }

    #[test]
    fn strip_and_unsafe_paths() {
        assert_eq!(
            stripped(Path::new("zig-1/bin/zig"), 1).unwrap(),
            Some(PathBuf::from("bin/zig"))
        );
        assert_eq!(stripped(Path::new("zig-1"), 1).unwrap(), None);
        assert!(stripped(Path::new("../x"), 0).is_err());
        assert!(stripped(Path::new("/etc/passwd"), 0).is_err());
    }

    #[test]
    fn symlinks_may_not_leave_the_install_directory() {
        assert!(check_link(Path::new("bin/zig"), Path::new("../lib/zig")).is_ok());
        assert!(check_link(Path::new("bin/zig"), Path::new("../../outside")).is_err());
        assert!(check_link(Path::new("a"), Path::new("/etc/passwd")).is_err());
        assert!(check_link(Path::new("a"), Path::new("../x")).is_err());
    }

    #[test]
    fn missing_reports_unknown_names_and_skips_satisfied_ones() {
        let mut specs = BTreeMap::new();
        specs.insert("tool".to_string(), spec());
        let names = vec!["tool".to_string()];
        assert_eq!(missing(&names, &specs, &[]).unwrap().len(), 1);
        assert!(missing(&names, &specs, &["tool"]).unwrap().is_empty());
        let unknown = vec!["nope".to_string()];
        assert!(missing(&unknown, &specs, &[])
            .unwrap_err()
            .contains("[toolchains.install.nope]"));
    }

    #[test]
    fn unpacks_a_tar_with_strip_and_rejects_escaping_entries() {
        let dir = std::env::temp_dir().join(format!("embedide-unpack-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut data = Vec::new();
        {
            let mut b = tar::Builder::new(&mut data);
            let mut h = tar::Header::new_gnu();
            h.set_path("top/bin/tool").unwrap();
            h.set_size(3);
            h.set_mode(0o4755); // setuid must not survive
            h.set_cksum();
            b.append(&h, &b"hi\n"[..]).unwrap();
            let mut l = tar::Header::new_gnu();
            l.set_entry_type(tar::EntryType::Symlink);
            l.set_path("top/bin/alias").unwrap();
            l.set_link_name("tool").unwrap();
            l.set_size(0);
            l.set_cksum();
            b.append(&l, std::io::empty()).unwrap();
            b.finish().unwrap();
        }
        unpack(&data[..], &dir, 1).unwrap();
        assert_eq!(fs::read_to_string(dir.join("bin/tool")).unwrap(), "hi\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join("bin/tool"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o7000, 0, "setuid bits are dropped");
            assert_eq!(
                fs::read_link(dir.join("bin/alias")).unwrap(),
                PathBuf::from("tool")
            );
        }
        // An archive that tries to escape is refused outright.
        let mut bad = Vec::new();
        {
            let mut b = tar::Builder::new(&mut bad);
            let mut l = tar::Header::new_gnu();
            l.set_entry_type(tar::EntryType::Symlink);
            l.set_path("top/evil").unwrap();
            l.set_link_name("../../../etc").unwrap();
            l.set_size(0);
            l.set_cksum();
            b.append(&l, std::io::empty()).unwrap();
            b.finish().unwrap();
        }
        assert!(unpack(&bad[..], &dir, 1)
            .unwrap_err()
            .contains("leaves the install directory"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_nested_symlink_traversal_escape() {
        let base = std::env::temp_dir().join(format!("embedide-slip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let dest = base.join("dest");
        fs::create_dir_all(&dest).unwrap();
        let mut slip = Vec::new();
        {
            let mut b = tar::Builder::new(&mut slip);
            let mut l1 = tar::Header::new_gnu();
            l1.set_entry_type(tar::EntryType::Symlink);
            l1.set_path("top/a/b").unwrap();
            l1.set_link_name("..").unwrap();
            l1.set_size(0);
            l1.set_cksum();
            b.append(&l1, std::io::empty()).unwrap();
            let mut l2 = tar::Header::new_gnu();
            l2.set_entry_type(tar::EntryType::Symlink);
            l2.set_path("top/a/b/esc").unwrap();
            l2.set_link_name("..").unwrap();
            l2.set_size(0);
            l2.set_cksum();
            b.append(&l2, std::io::empty()).unwrap();
            let mut f = tar::Header::new_gnu();
            f.set_path("top/a/b/esc/pwned").unwrap();
            f.set_size(4);
            f.set_cksum();
            b.append(&f, &b"evil"[..]).unwrap();
            b.finish().unwrap();
        }
        let err = unpack(&slip[..], &dest, 1).unwrap_err();
        assert!(err.contains("traverses a symlink"), "{err}");
        assert!(!base.join("pwned").exists());
        let _ = fs::remove_dir_all(&base);
    }
}
