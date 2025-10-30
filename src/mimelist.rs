// SPDX-License-Identifier: GPL-3.0-only

use cosmic::iced;
use cosmic::widget::table;
use log::info;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::{env, fs};

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy, Hash)]
pub enum MimeCategory {
    #[default]
    Name,
    Description,
}

impl std::fmt::Display for MimeCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Name => "Name",
            Self::Description => "Description",
        })
    }
}

impl table::ItemCategory for MimeCategory {
    fn width(&self) -> iced::Length {
        match self {
            Self::Name => iced::Length::Fixed(200.0),
            Self::Description => iced::Length::Fill,
        }
    }
}
#[derive(Default, Debug, Clone)]
pub struct MimeItem {
    pub name: String,
    pub description: String,
}

impl table::ItemInterface<MimeCategory> for MimeItem {
    fn get_icon(&self, _category: MimeCategory) -> Option<cosmic::widget::Icon> {
        None
    }

    fn get_text(&self, category: MimeCategory) -> std::borrow::Cow<'static, str> {
        match category {
            MimeCategory::Name => self.name.clone().into(),
            MimeCategory::Description => self.description.clone().into(),
        }
    }

    fn compare(&self, other: &Self, category: MimeCategory) -> std::cmp::Ordering {
        match category {
            MimeCategory::Name => self.name.to_lowercase().cmp(&other.name.to_lowercase()),
            MimeCategory::Description => self
                .description
                .to_lowercase()
                .cmp(&other.description.to_lowercase()),
        }
    }
}

pub struct MimeCache {
    mime_descriptions: HashMap<String, String>,
}

impl Default for MimeCache {
    fn default() -> Self {
        let mut cache = Self {
            mime_descriptions: Default::default(),
        };
        cache.scan();
        cache
    }
}

impl MimeCache {
    pub fn lookup(&self, name: &str) -> Option<&String> {
        self.mime_descriptions.get(name)
    }

    fn candidate_mime_dirs() -> Vec<PathBuf> {
        let in_flatpak = std::env::var_os("FLATPAK_ID").is_some();

        if in_flatpak {
            vec![
                PathBuf::from("/run/host/usr/share/mime/packages"),
                PathBuf::from("/run/host/share/mime/packages"),
                PathBuf::from("/usr/share/mime/packages"), // fallback to runtime's view
            ]
        } else {
            vec![
                PathBuf::from("/usr/share/mime/packages"),
                PathBuf::from("/usr/local/share/mime/packages"),
            ]
        }
    }

    pub fn get_mime_aliases() -> HashMap<String, String> {
        let mut paths: Vec<PathBuf> = Vec::new();
        let mut aliases = HashMap::new();

        paths.push(PathBuf::from("/usr/share/mime/aliases"));
        paths.push(PathBuf::from("/usr/local/share/mime/aliases"));

        if let Ok(fp) = env::var("FLATPAK_ID") {
            if let Ok(runtime) = env::var("FLATPAK_RUNTIME_DIR") {
                paths.push(PathBuf::from(runtime).join("mime/aliases"));
            }
            paths.push(PathBuf::from("/app/share/mime/aliases"));
            paths.push(PathBuf::from("/usr/share/mime/aliases"));
        }

        for path in paths {
            if let Ok(file) = fs::File::open(&path) {
                info!("Reading mime aliases from {}", path.display());
                let reader = BufReader::new(file);
                for line in reader.lines().flatten() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    if let Some((alias, canon)) = trimmed.split_once(char::is_whitespace) {
                        aliases.insert(canon.to_owned(), alias.to_owned());
                    }
                }
            }
        }
        info!("Loaded {} mime aliases.", aliases.len());
        aliases
    }

    pub fn scan(&mut self) {
        self.mime_descriptions.clear();
        let langs = freedesktop_desktop_entry::get_languages_from_env();

        let aliases = Self::get_mime_aliases();

        for dir in Self::candidate_mime_dirs() {
            if let Ok(read_dir) = fs::read_dir(&dir) {
                for entry in read_dir.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("xml") {
                        continue;
                    }

                    if let Ok(xml) = fs::read_to_string(&path) {
                        info!("Loading mime descriptions from {}", path.to_string_lossy());
                        if let Ok(doc) = roxmltree::Document::parse(&xml) {
                            for mime_node in
                                doc.descendants().filter(|n| n.has_tag_name("mime-type"))
                            {
                                let mime_type = match mime_node.attribute("type") {
                                    Some(t) => t.to_string(),
                                    None => continue,
                                };

                                // We'll pick the best comment based on language pref.
                                // We track best match index in langs[] (lower is better),
                                // or None for unlocalized fallback.
                                let mut best_score: Option<usize> = None;
                                let mut best_text: Option<String> = None;
                                let mut fallback_unlocalized: Option<String> = None;

                                for child in
                                    mime_node.children().filter(|c| c.has_tag_name("comment"))
                                {
                                    let txt = child.text().unwrap_or("").trim();
                                    if txt.is_empty() {
                                        continue;
                                    }

                                    if let Some(lang_attr) = child
                                        .attribute(("http://www.w3.org/XML/1998/namespace", "lang"))
                                    {
                                        // see if this lang matches our pref list
                                        if let Some(pos) = langs.iter().position(|l| l == lang_attr)
                                        {
                                            // lower pos is higher priority
                                            match best_score {
                                                Some(existing_pos) if existing_pos <= pos => {
                                                    // keep old best
                                                }
                                                _ => {
                                                    best_score = Some(pos);
                                                    best_text = Some(txt.to_string());
                                                }
                                            }
                                        }
                                    } else {
                                        fallback_unlocalized = Some(txt.to_string());
                                    }
                                }

                                let chosen = best_text.or(fallback_unlocalized);

                                // So we insert the new mimetype/description but if there's an alias
                                // we also insert that
                                if let Some(desc) = chosen {
                                    self.mime_descriptions
                                        .entry(mime_type.clone())
                                        .or_insert(desc.clone());
                                    if let Some(alias) = aliases.get(&mime_type) {
                                        self.mime_descriptions.entry(alias.clone()).or_insert(desc);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        info!(
            "Mime cache: Loaded {} mime type descriptions",
            self.mime_descriptions.len()
        );
    }
}
