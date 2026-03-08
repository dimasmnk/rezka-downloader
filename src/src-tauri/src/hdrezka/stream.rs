use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use crate::hdrezka::errors::HdRezkaError;

/// Subtitles associated with a stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HdRezkaStreamSubtitles {
    pub subtitles: HashMap<String, SubtitleEntry>,
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleEntry {
    pub title: String,
    pub link: String,
}

impl HdRezkaStreamSubtitles {
    pub fn new(data: Option<&str>, codes: Option<&HashMap<String, String>>) -> Self {
        let mut subtitles = HashMap::new();
        let mut keys = Vec::new();

        if let (Some(data), Some(codes)) = (data, codes) {
            if !data.is_empty() {
                for item in data.split(',') {
                    if let Some(bracket_start) = item.find('[') {
                        if let Some(bracket_end) = item[bracket_start..].find(']') {
                            let lang = &item[bracket_start + 1..bracket_start + bracket_end];
                            let link = &item[bracket_start + bracket_end + 1..];
                            if let Some(code) = codes.get(lang) {
                                subtitles.insert(
                                    code.clone(),
                                    SubtitleEntry {
                                        title: lang.to_string(),
                                        link: link.to_string(),
                                    },
                                );
                                keys.push(code.clone());
                            }
                        }
                    }
                }
            }
        }

        Self { subtitles, keys }
    }

    /// Get subtitle URL by language code, title, or index.
    pub fn get(&self, id: &str) -> Result<String, HdRezkaError> {
        if self.subtitles.is_empty() {
            return Err(HdRezkaError::ValueError("No subtitles available".to_string()));
        }

        // Try direct key lookup
        if let Some(entry) = self.subtitles.get(id) {
            return Ok(entry.link.clone());
        }

        // Try matching by title
        for (_key, value) in &self.subtitles {
            if value.title == id {
                return Ok(value.link.clone());
            }
        }

        // Try as numeric index
        if let Ok(index) = id.parse::<usize>() {
            if index < self.keys.len() {
                let code = &self.keys[index];
                if let Some(entry) = self.subtitles.get(code) {
                    return Ok(entry.link.clone());
                }
            }
        }

        Err(HdRezkaError::ValueError(format!(
            "Subtitles \"{}\" is not defined",
            id
        )))
    }
}

impl fmt::Display for HdRezkaStreamSubtitles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.keys)
    }
}

/// A stream result containing video URLs at different resolutions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HdRezkaStream {
    videos: HashMap<String, Vec<String>>,
    pub name: String,
    pub translator_id: i64,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub subtitles: HdRezkaStreamSubtitles,
}

impl HdRezkaStream {
    pub fn new(
        season: Option<i64>,
        episode: Option<i64>,
        name: String,
        translator_id: i64,
        subtitle_data: Option<&str>,
        subtitle_codes: Option<&HashMap<String, String>>,
    ) -> Self {
        Self {
            videos: HashMap::new(),
            name,
            translator_id,
            season,
            episode,
            subtitles: HdRezkaStreamSubtitles::new(subtitle_data, subtitle_codes),
        }
    }

    pub fn videos(&self) -> &HashMap<String, Vec<String>> {
        &self.videos
    }

    pub fn append(&mut self, resolution: String, link: String) {
        self.videos
            .entry(resolution)
            .or_default()
            .push(link);
    }

    /// Get video URLs for a given resolution (partial match).
    pub fn get(&self, resolution: &str) -> Result<&Vec<String>, HdRezkaError> {
        let resolution_str = resolution.to_string();
        let coincidences: Vec<&String> = self
            .videos
            .keys()
            .filter(|k| k.contains(&resolution_str))
            .collect();

        if let Some(key) = coincidences.first() {
            return Ok(self.videos.get(*key).unwrap());
        }

        Err(HdRezkaError::ValueError(format!(
            "Resolution \"{}\" is not defined",
            resolution
        )))
    }

    /// Get all available resolutions.
    pub fn resolutions(&self) -> Vec<String> {
        self.videos.keys().cloned().collect()
    }
}

impl fmt::Display for HdRezkaStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let resolutions: Vec<String> = self.videos.keys().cloned().collect();
        if !self.subtitles.subtitles.is_empty() {
            write!(
                f,
                "<HdRezkaStream> : {:?}, subtitles={}",
                resolutions, self.subtitles
            )
        } else {
            write!(f, "<HdRezkaStream> : {:?}", resolutions)
        }
    }
}
