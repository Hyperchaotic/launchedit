// SPDX-License-Identifier: GPL-3.0-only

use log::info;
use std::collections::HashMap;
use std::env;
use std::fs;

use crate::app::DesktopEntryType;
use crate::fl;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static TITLE_DESKTOP_FILE: LazyLock<&'static str> =
    LazyLock::new(|| Box::leak(fl!("select-desktop").into_boxed_str()));

static TITLE_EXECUTABLE: LazyLock<&'static str> =
    LazyLock::new(|| Box::leak(fl!("select-executable").into_boxed_str()));

static TITLE_DIRECTORY: LazyLock<&'static str> =
    LazyLock::new(|| Box::leak(fl!("select-directory").into_boxed_str()));

static TITLE_ICON_FILE: LazyLock<&'static str> =
    LazyLock::new(|| Box::leak(fl!("select-icon").into_boxed_str()));

static DESKTOP_FILES: LazyLock<&'static str> =
    LazyLock::new(|| Box::leak(fl!("name-desktopfiles").into_boxed_str()));

static EXECUTABLES: LazyLock<&'static str> =
    LazyLock::new(|| Box::leak(fl!("name-executables").into_boxed_str()));

static IMAGES: LazyLock<&'static str> =
    LazyLock::new(|| Box::leak(fl!("name-images").into_boxed_str()));

static SAVE_DESKTOPFILE: LazyLock<&'static str> =
    LazyLock::new(|| Box::leak(fl!("save-desktopfile").into_boxed_str()));

static SAVE: LazyLock<&'static str> =
    LazyLock::new(|| Box::leak(fl!("menu-save").into_boxed_str()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    DesktopFile,
    Executable,
    TryExecutable,
    Directory,
    IconFile,
}

impl PickKind {
    pub fn title(self) -> &'static str {
        match self {
            PickKind::DesktopFile => *TITLE_DESKTOP_FILE,
            PickKind::Executable | PickKind::TryExecutable => *TITLE_EXECUTABLE,
            PickKind::Directory => *TITLE_DIRECTORY,
            PickKind::IconFile => *TITLE_ICON_FILE,
        }
    }
}

fn uri_to_path(u: &url::Url) -> Option<PathBuf> {
    if u.scheme() == "file" {
        u.to_file_path().ok()
    } else {
        None
    }
}

pub async fn save_desktop_file(suggested_name: String, kind: DesktopEntryType) -> Option<PathBuf> {
    use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};

    let base = || {
        let filter = if kind == DesktopEntryType::Directory {
            FileFilter::new(*DESKTOP_FILES)
                .glob("*.directory")
                .glob("*.desktop")
                .mimetype("application/x-desktop")
        } else {
            FileFilter::new(*DESKTOP_FILES)
                .glob("*.desktop")
                .mimetype("application/x-desktop")
        };

        SelectedFiles::save_file()
            .title(*SAVE_DESKTOPFILE)
            .accept_label(*SAVE)
            .current_name(suggested_name.as_str())
            .modal(true)
            .filter(filter)
    };

    let request =
        match dirs::home_dir().map(|h| h.join(".local").join("share").join("applications")) {
            None => base(),
            Some(folder) => {
                // Try building with current_folder first
                match base().current_folder(folder) {
                    Ok(req) => req,
                    Err(e) => {
                        log::error!("Failed to set start folder {e}");
                        base()
                    }
                }
            }
        };

    let response = match request.send().await {
        Ok(rq) => match rq.response() {
            Ok(r) => r,
            Err(e) => {
                log::error!("Portal response error: {e}");
                return None;
            }
        },
        Err(e) => {
            log::error!("Portal send error: {e}");
            return None;
        }
    };

    response.uris().first().and_then(uri_to_path)
}

pub async fn open_path(kind: PickKind) -> (Option<PathBuf>, PickKind) {
    use ashpd::desktop::file_chooser::{FileFilter, OpenFileRequest};

    let base = || {
        OpenFileRequest::default()
            .title(kind.title())
            .accept_label("Select")
            .modal(true)
    };

    let request = match kind {
        PickKind::Directory => base().directory(true),
        PickKind::DesktopFile => {
            let filter = FileFilter::new(*DESKTOP_FILES)
                .glob("*.desktop")
                .mimetype("application/x-desktop");

            match dirs::home_dir().map(|h| h.join(".local").join("share").join("applications")) {
                None => base(),
                Some(folder) => {
                    // Try building with current_folder first
                    match base().current_folder(folder) {
                        Ok(req) => req.filter(filter),
                        Err(e) => {
                            log::error!("Failed to set start folder {e}");
                            base().filter(filter)
                        }
                    }
                }
            }
        }
        PickKind::Executable | PickKind::TryExecutable => {
            let filter = FileFilter::new(*EXECUTABLES)
                .glob("*.sh")
                .glob("*.bin")
                .mimetype("application/x-executable")
                .mimetype("text/x-shellscript");
            base().filter(filter)
        }
        PickKind::IconFile => {
            // Common icon/image types used by desktop entries & themes
            let filter = FileFilter::new(*IMAGES)
                .glob("*.png")
                .glob("*.svg")
                .glob("*.jpg")
                .glob("*.jpeg")
                .mimetype("image/png")
                .mimetype("image/svg+xml")
                .mimetype("image/jpeg");
            base().filter(filter)
        }
    };

    let response = match request.send().await {
        Ok(rq) => match rq.response() {
            Ok(r) => r,
            Err(e) => {
                log::error!("Portal response error: {e}");
                return (None, kind);
            }
        },
        Err(e) => {
            log::error!("Portal send error: {e}");
            return (None, kind);
        }
    };

    let picked = response.uris().first().and_then(uri_to_path);
    (picked, kind)
}

pub struct IconCache {
    by_name_no_ext: HashMap<String, PathBuf>,
    by_full_name: HashMap<String, PathBuf>,
}

impl Default for IconCache {
    fn default() -> Self {
        let mut cache = Self {
            by_name_no_ext: HashMap::default(),
            by_full_name: HashMap::default(),
        };
        cache.scan();
        cache
    }
}

impl IconCache {
    const THEMES: [&'static str; 3] = ["cosmic", "Adwaita", "hicolor"];
    const SIZES: [&'static str; 9] = [
        "scalable", "512x512", "256x256", "128x128", "64x64", "48x48", "32x32", "24x24", "16x16",
    ];
    const CONTEXTS: [&'static str; 4] = ["apps", "places", "mimetypes", "actions"];

    // Load all icons paths
    pub fn scan(&mut self) {
        let base_dirs = Self::icon_search_dirs();

        for base in base_dirs {
            for theme in Self::THEMES {
                for size in Self::SIZES {
                    for ctx in Self::CONTEXTS {
                        let dir = base.join(theme).join(size).join(ctx);
                        self.scan_dir(&dir);
                    }
                }
            }
            self.scan_dir(&base.join("pixmaps"));
        }
        info!(
            "Icon cache: Loaded {} base names, {} full names",
            self.by_name_no_ext.len(),
            self.by_full_name.len()
        );
    }

    pub fn lookup(&self, name: &str) -> Option<&PathBuf> {
        if let Some(path) = self.by_full_name.get(name) {
            return Some(path);
        }
        if let Some(path) = self.by_name_no_ext.get(name) {
            return Some(path);
        }

        None
    }

    fn icon_search_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        if let Ok(home) = env::var("XDG_DATA_HOME") {
            dirs.push(PathBuf::from(home).join("icons"));
        } else if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".local/share/icons"));
        }

        if let Ok(var) = env::var("XDG_DATA_DIRS") {
            for p in var.split(':') {
                dirs.push(PathBuf::from(p).join("icons"));
            }
        } else {
            dirs.push(PathBuf::from("/usr/local/share/icons"));
            dirs.push(PathBuf::from("/usr/share/icons"));
        }

        // Flatpak host dirs (if inside sandbox)
        if env::var_os("FLATPAK_ID").is_some() {
            dirs.push(PathBuf::from("/run/host/usr/share/icons"));
            dirs.push(PathBuf::from("/run/host/share/icons"));
        }

        dirs.push(PathBuf::from("/usr/share/pixmaps"));

        dirs
    }

    fn scan_dir(&mut self, root: &Path) {
        let exts = ["png", "svg", "xpm", "ico", "jpg", "jpeg"];
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                self.scan_dir(&path);
                continue;
            }

            if let Some(ext) = path.extension().and_then(|e| e.to_str())
                && exts.contains(&ext)
                && let Some(fname) = path.file_name().and_then(|s| s.to_str())
            {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                self.by_full_name
                    .entry(fname.to_string())
                    .or_insert(path.clone());
                self.by_name_no_ext
                    .entry(stem.to_string())
                    .or_insert(path.clone());
            }
        }
    }
}
