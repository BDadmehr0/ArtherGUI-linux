//! Linux application enumeration for the split-tunneling app picker.
//!
//! The Windows picker (`app_picker.rs`) resolves Start-Menu `.lnk`
//! targets in-process; this is the freedesktop equivalent and obeys the
//! same dev.030 rules: no child process, no script files, read-only
//! metadata from well-known locations only, icons read from files (never
//! by executing the target), and the exact same `EnumeratedApplication`
//! payload (name / absolute path / base64 PNG icon) for the frontend.
//!
//! Sources, in XDG precedence order:
//!   * `$XDG_DATA_HOME/applications` (default `~/.local/share/applications`)
//!   * each `$XDG_DATA_DIRS/applications` (default `/usr/local/share`,
//!     `/usr/share`)
//!
//! Only `Type=Application` entries without `NoDisplay=true`/`Hidden=true`
//! are returned, and only when their `Exec=` resolves to an absolute
//! executable path (settings validation requires absolute paths). A
//! per-user entry shadows a system entry with the same desktop-file ID,
//! matching desktop-environment behaviour.

use base64::Engine;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One enumerated application (same shape as the Windows picker).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumeratedApplication {
    pub name: String,
    pub path: String,
    pub icon: String,
}

/// Cap the harvest: the picker is a settings UI, not an app launcher, and
/// icon reads make unbounded enumeration pointlessly slow.
const MAX_APPLICATIONS: usize = 512;
const MAX_ICON_BYTES: usize = 256 * 1024;

fn application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(data_home);
        if path.is_absolute() {
            dirs.push(path.join("applications"));
        }
    }
    if dirs.is_empty() {
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".local/share/applications"));
        }
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for entry in data_dirs.split(':') {
        if !entry.is_empty() {
            dirs.push(PathBuf::from(entry).join("applications"));
        }
    }
    dirs
}

fn data_dirs_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for entry in data_dirs.split(':') {
        if !entry.is_empty() {
            roots.push(PathBuf::from(entry));
        }
    }
    roots
}

/// Split an `Exec=` value into its program token, honouring the simple
/// quoting the desktop-entry spec allows. Arguments and field codes
/// (`%f`, `%U`, ...) belong to *launching*, which the picker never does;
/// only the program identity matters here.
fn exec_program(exec: &str) -> Option<String> {
    let exec = exec.trim();
    if exec.is_empty() {
        return None;
    }
    // `env FOO=bar prog ...` is common in shipped desktop files.
    let mut tokens = exec.split_whitespace();
    let mut program = tokens.next()?.trim_matches('"').to_string();
    if program == "env" {
        for token in tokens.by_ref() {
            if !token.contains('=') {
                program = token.trim_matches('"').to_string();
                break;
            }
        }
    }
    if program.is_empty() || program.starts_with('%') {
        return None;
    }
    Some(program)
}

fn resolve_program(program: &str) -> Option<PathBuf> {
    let candidate = Path::new(program);
    if candidate.is_absolute() {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let resolved = dir.join(program);
        if resolved.is_file() {
            return Some(resolved);
        }
    }
    None
}

/// Resolve an `Icon=` value to base64 PNG bytes: absolute paths directly,
/// themed names through the hicolor fallback locations and /usr/share/pixmaps.
/// Anything else (SVG-only themes, missing files) yields an empty icon, which
/// the frontend renders as its placeholder tile - same as a Windows target
/// without an extractable icon.
fn icon_base64(icon: &str) -> String {
    if icon.is_empty() {
        return String::new();
    }
    let read_png = |path: &Path| -> Option<String> {
        let bytes = std::fs::read(path).ok()?;
        if bytes.len() > MAX_ICON_BYTES || !bytes.starts_with(b"\x89PNG") {
            return None;
        }
        Some(base64::engine::general_purpose::STANDARD.encode(bytes))
    };
    let as_path = Path::new(icon);
    if as_path.is_absolute() {
        return read_png(as_path).unwrap_or_default();
    }
    for root in data_dirs_roots() {
        for size in ["48x48", "64x64", "128x128", "256x256", "32x32"] {
            let candidate = root
                .join("icons/hicolor")
                .join(size)
                .join("apps")
                .join(format!("{icon}.png"));
            if let Some(encoded) = read_png(&candidate) {
                return encoded;
            }
        }
        let pixmap = root.join("pixmaps").join(format!("{icon}.png"));
        if let Some(encoded) = read_png(&pixmap) {
            return encoded;
        }
    }
    String::new()
}

/// Parse one .desktop file's [Desktop Entry] group into an application,
/// or None when it is not a visible, resolvable application entry.
fn parse_desktop_entry(path: &Path) -> Option<EnumeratedApplication> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let (mut name, mut exec, mut icon) = (None, None, None);
    let (mut is_application, mut hidden) = (false, false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Type" => is_application = value.trim() == "Application",
            "Name" if name.is_none() => name = Some(value.trim().to_string()),
            "Exec" if exec.is_none() => exec = Some(value.trim().to_string()),
            "Icon" if icon.is_none() => icon = Some(value.trim().to_string()),
            "NoDisplay" | "Hidden" if value.trim().eq_ignore_ascii_case("true") => {
                hidden = true;
            }
            _ => {}
        }
    }
    if !is_application || hidden {
        return None;
    }
    let program = exec_program(&exec?)?;
    let resolved = resolve_program(&program)?;
    let name = name.filter(|value| !value.is_empty())?;
    Some(EnumeratedApplication {
        name,
        path: resolved.to_string_lossy().into_owned(),
        icon: icon_base64(&icon.unwrap_or_default()),
    })
}

/// Enumerate visible applications from the XDG desktop-entry dirs.
pub fn enumerate_start_menu_apps() -> Result<Vec<EnumeratedApplication>, String> {
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut apps = Vec::new();
    for dir in application_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("desktop"))
                    .unwrap_or(false)
            })
            .collect();
        files.sort();
        for file in files {
            if apps.len() >= MAX_APPLICATIONS {
                return Ok(apps);
            }
            // Desktop-file IDs are unique per application; a same-named
            // file in an earlier (higher-precedence) dir shadows this one.
            let id = file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !seen_ids.insert(id) {
                continue;
            }
            if let Some(app) = parse_desktop_entry(&file) {
                if seen_paths.insert(app.path.clone()) {
                    apps.push(app);
                }
            }
        }
    }
    apps.sort_by_key(|app| app.name.to_lowercase());
    Ok(apps)
}
