use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

// Default cookies and headers
pub fn default_cookies() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("hdmbbs".to_string(), "1".to_string());
    m
}

pub fn default_headers() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        "User-Agent".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/81.0.4044.138 Safari/537.36".to_string(),
    );
    m
}

pub fn default_translators_priority() -> Vec<i64> {
    vec![
        56,  // Дубляж
        105, // StudioBand
        111, // HDrezka Studio
    ]
}

pub fn default_translators_non_priority() -> Vec<i64> {
    vec![
        238, // Оригинал + субтитры
    ]
}

// --- HdRezkaFormat ---
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HdRezkaFormat {
    TvSeries,
    Movie,
    Other(String),
}

impl HdRezkaFormat {
    pub fn name(&self) -> &str {
        match self {
            HdRezkaFormat::TvSeries => "tv_series",
            HdRezkaFormat::Movie => "movie",
            HdRezkaFormat::Other(s) => s,
        }
    }

    pub fn is_tv_series(&self) -> bool {
        matches!(self, HdRezkaFormat::TvSeries)
    }

    pub fn is_movie(&self) -> bool {
        matches!(self, HdRezkaFormat::Movie)
    }
}

impl fmt::Display for HdRezkaFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "format.{}", self.name())
    }
}

// --- HdRezkaCategory ---
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HdRezkaCategory {
    Film,
    Series,
    Cartoon,
    Anime,
    Other(String),
}

impl HdRezkaCategory {
    pub fn name(&self) -> &str {
        match self {
            HdRezkaCategory::Film => "film",
            HdRezkaCategory::Series => "series",
            HdRezkaCategory::Cartoon => "cartoon",
            HdRezkaCategory::Anime => "anime",
            HdRezkaCategory::Other(s) => s,
        }
    }
}

impl fmt::Display for HdRezkaCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "category.{}", self.name())
    }
}

// --- HdRezkaRating ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HdRezkaRating {
    pub value: Option<f64>,
    pub votes: Option<i64>,
}

impl HdRezkaRating {
    pub fn new(value: f64, votes: i64) -> Self {
        Self {
            value: Some(value),
            votes: Some(votes),
        }
    }

    pub fn empty() -> Self {
        Self {
            value: None,
            votes: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_none()
    }
}

impl fmt::Display for HdRezkaRating {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.value, self.votes) {
            (Some(v), Some(votes)) => write!(f, "{} ({})", v, votes),
            _ => write!(f, "HdRezkaRating(Empty)"),
        }
    }
}

// --- Translator info ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslatorInfo {
    pub name: String,
    pub premium: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslatorByName {
    pub id: i64,
    pub premium: bool,
}

// --- Series info ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesTranslatorInfo {
    pub translator_name: String,
    pub premium: bool,
    pub seasons: HashMap<i64, String>,
    pub episodes: HashMap<i64, HashMap<i64, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeTranslation {
    pub translator_id: i64,
    pub translator_name: String,
    pub premium: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeInfo {
    pub episode: i64,
    pub episode_text: String,
    pub translations: Vec<EpisodeTranslation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonEpisodesInfo {
    pub season: i64,
    pub season_text: String,
    pub episodes: Vec<EpisodeInfo>,
}

// --- Other parts ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtherPart {
    pub name: String,
    pub url: String,
}

// --- Search results ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastSearchResult {
    pub title: String,
    pub url: String,
    pub rating: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedSearchResult {
    pub title: String,
    pub url: String,
    pub image: String,
    pub category: Option<HdRezkaCategory>,
}
