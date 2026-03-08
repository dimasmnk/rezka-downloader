use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use once_cell::sync::OnceCell;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use scraper::{Html, Selector};
use serde_json::Value;
use std::collections::HashMap;
use url::Url;

use crate::hdrezka::errors::HdRezkaError;
use crate::hdrezka::stream::HdRezkaStream;
use crate::hdrezka::types::*;

/// Main API client for HDRezka.
pub struct HdRezkaApi {
    pub url: String,
    pub origin: String,
    pub proxy: Option<String>,
    pub cookies: HashMap<String, String>,
    headers: HashMap<String, String>,
    pub translators_priority: Vec<i64>,
    pub translators_non_priority: Vec<i64>,

    // Cached / lazily-initialized fields
    page_content: OnceCell<String>,
    page_status: OnceCell<Result<(), HdRezkaError>>,
    client: reqwest::Client,
}

impl std::fmt::Debug for HdRezkaApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HdRezkaApi")
            .field("url", &self.url)
            .field("origin", &self.origin)
            .finish()
    }
}

impl HdRezkaApi {
    /// Create a new HdRezkaApi instance.
    pub fn new(
        url: &str,
        proxy: Option<String>,
        headers: HashMap<String, String>,
        cookies: HashMap<String, String>,
        translators_priority: Option<Vec<i64>>,
        translators_non_priority: Option<Vec<i64>>,
    ) -> Self {
        let clean_url = match url.split_once(".html") {
            Some((before, _)) => format!("{}.html", before),
            None => url.to_string(),
        };

        let parsed = Url::parse(&clean_url).unwrap_or_else(|_| Url::parse("http://localhost").unwrap());
        let origin = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or("localhost"));

        let mut merged_cookies = default_cookies();
        for (k, v) in cookies {
            merged_cookies.insert(k, v);
        }

        let mut merged_headers = default_headers();
        for (k, v) in headers {
            merged_headers.insert(k, v);
        }

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .gzip(true)
            .build()
            .unwrap_or_default();

        Self {
            url: clean_url,
            origin,
            proxy,
            cookies: merged_cookies,
            headers: merged_headers,
            translators_priority: translators_priority.unwrap_or_else(default_translators_priority),
            translators_non_priority: translators_non_priority
                .unwrap_or_else(default_translators_non_priority),
            page_content: OnceCell::new(),
            page_status: OnceCell::new(),
            client,
        }
    }

    fn build_header_map(&self) -> HeaderMap {
        let mut hm = HeaderMap::new();
        for (k, v) in &self.headers {
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                hm.insert(name, val);
            }
        }
        hm
    }

    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Fetch the page content (cached).  
    async fn fetch_page(&self) -> Result<&str, HdRezkaError> {
        if self.page_content.get().is_some() {
            // Already fetched — check stored status
            if let Some(Err(e)) = self.page_status.get() {
                return Err(HdRezkaError::Http {
                    code: 0,
                    message: format!("{}", e),
                });
            }
            return Ok(self.page_content.get().unwrap().as_str());
        }

        let mut req = self
            .client
            .get(&self.url)
            .headers(self.build_header_map())
            .header("Cookie", self.cookie_header());

        if let Some(ref proxy_url) = self.proxy {
            // For proxy support we'd need to rebuild the client; store info for now
            let _ = proxy_url;
        }
        let _ = &mut req;

        let response = self
            .client
            .get(&self.url)
            .headers(self.build_header_map())
            .header("Cookie", self.cookie_header())
            .send()
            .await?;

        if !response.status().is_success() {
            let code = response.status().as_u16();
            let reason = response
                .status()
                .canonical_reason()
                .unwrap_or("")
                .to_string();
            let err = HdRezkaError::Http {
                code,
                message: reason,
            };
            let _ = self.page_status.set(Err(HdRezkaError::Http {
                code,
                message: String::new(),
            }));
            return Err(err);
        }

        let body = response.text().await?;
        let _ = self.page_content.set(body);
        let _ = self.page_status.set(Ok(()));
        Ok(self.page_content.get().unwrap().as_str())
    }

    fn parse_html(content: &str) -> Result<Html, HdRezkaError> {
        let document = Html::parse_document(content);
        let title_sel = Selector::parse("title").unwrap();
        if let Some(title_el) = document.select(&title_sel).next() {
            let title_text = title_el.text().collect::<String>();
            if title_text == "Sign In" {
                return Err(HdRezkaError::LoginRequired);
            }
            if title_text == "Verify" {
                return Err(HdRezkaError::CaptchaError);
            }
        }
        Ok(document)
    }

    /// Check if the page was fetched successfully.
    pub async fn ok(&self) -> bool {
        self.fetch_page().await.is_ok()
    }

    /// Get the exception if the page fetch failed.
    pub async fn exception(&self) -> Option<HdRezkaError> {
        match self.fetch_page().await {
            Err(e) => Some(e),
            Ok(content) => match Self::parse_html(content) {
                Err(e) => Some(e),
                Ok(_) => None,
            },
        }
    }

    /// Login with email and password.
    pub async fn login(&mut self, email: &str, password: &str) -> Result<bool, HdRezkaError> {
        let mut form = HashMap::new();
        form.insert("login_name", email);
        form.insert("login_password", password);

        let response = self
            .client
            .post(format!("{}/ajax/login/", self.origin))
            .headers(self.build_header_map())
            .header("Cookie", self.cookie_header())
            .form(&form)
            .send()
            .await?;

        // Extract cookies from response
        for cookie in response.cookies() {
            self.cookies
                .insert(cookie.name().to_string(), cookie.value().to_string());
        }

        let body = response.text().await?;
        let data: Value = serde_json::from_str(&body).map_err(|_| {
            HdRezkaError::ValueError(format!("Invalid JSON in login response"))
        })?;
        if data["success"].as_bool() == Some(true) {
            return Ok(true);
        }

        let msg = data["message"]
            .as_str()
            .unwrap_or("Unknown error")
            .to_string();
        Err(HdRezkaError::LoginFailed(msg))
    }

    /// Build cookies dict from user_id and password_hash.
    pub fn make_cookies(user_id: &str, password_hash: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("dle_user_id".to_string(), user_id.to_string());
        m.insert("dle_password".to_string(), password_hash.to_string());
        m
    }

    // --- Parsed properties ---

    async fn get_document(&self) -> Result<Html, HdRezkaError> {
        let content = self.fetch_page().await?;
        Self::parse_html(content)
    }

    /// Get the film/series ID.
    pub async fn id(&self) -> Result<i64, HdRezkaError> {
        let doc = self.get_document().await?;

        // Try post_id input
        let sel = Selector::parse("#post_id").unwrap();
        if let Some(el) = doc.select(&sel).next() {
            if let Some(val) = el.value().attr("value") {
                if let Ok(id) = val.parse::<i64>() {
                    return Ok(id);
                }
            }
        }

        // Try send-video-issue
        let sel2 = Selector::parse("#send-video-issue").unwrap();
        if let Some(el) = doc.select(&sel2).next() {
            if let Some(val) = el.value().attr("data-id") {
                if let Ok(id) = val.parse::<i64>() {
                    return Ok(id);
                }
            }
        }

        // Try user-favorites-holder
        let sel3 = Selector::parse("#user-favorites-holder").unwrap();
        if let Some(el) = doc.select(&sel3).next() {
            if let Some(val) = el.value().attr("data-post_id") {
                if let Ok(id) = val.parse::<i64>() {
                    return Ok(id);
                }
            }
        }

        // Extract from URL
        let last_segment = self.url.split('/').last().unwrap_or("");
        let id_str = last_segment.split('-').next().unwrap_or("0");
        id_str
            .parse::<i64>()
            .map_err(|_| HdRezkaError::ValueError("Could not determine film ID".to_string()))
    }

    /// Get the film name.
    pub async fn name(&self) -> Result<String, HdRezkaError> {
        let names = self.names().await?;
        names
            .into_iter()
            .next()
            .ok_or_else(|| HdRezkaError::ValueError("No name found".to_string()))
    }

    /// Get all names (split by "/").
    pub async fn names(&self) -> Result<Vec<String>, HdRezkaError> {
        let doc = self.get_document().await?;
        let sel = Selector::parse(".b-post__title").unwrap();
        if let Some(el) = doc.select(&sel).next() {
            let text: String = el.text().collect();
            return Ok(text.split('/').map(|s| s.trim().to_string()).collect());
        }
        Err(HdRezkaError::ValueError("Name element not found".to_string()))
    }

    /// Get original name(s).
    pub async fn orig_names(&self) -> Result<Vec<String>, HdRezkaError> {
        let doc = self.get_document().await?;
        let sel = Selector::parse(".b-post__origtitle").unwrap();
        if let Some(el) = doc.select(&sel).next() {
            let text: String = el.text().collect();
            return Ok(text.split('/').map(|s| s.trim().to_string()).collect());
        }
        Ok(vec![])
    }

    /// Get the primary original name.
    pub async fn orig_name(&self) -> Result<Option<String>, HdRezkaError> {
        let names = self.orig_names().await?;
        Ok(names.last().cloned())
    }

    /// Get description.
    pub async fn description(&self) -> Result<String, HdRezkaError> {
        let doc = self.get_document().await?;
        let sel = Selector::parse(".b-post__description_text").unwrap();
        if let Some(el) = doc.select(&sel).next() {
            let text: String = el.text().collect();
            return Ok(text.trim().to_string());
        }
        Err(HdRezkaError::ValueError("Description not found".to_string()))
    }

    /// Get thumbnail URL.
    pub async fn thumbnail(&self) -> Result<String, HdRezkaError> {
        let doc = self.get_document().await?;
        let sel = Selector::parse(".b-sidecover img").unwrap();
        if let Some(el) = doc.select(&sel).next() {
            if let Some(src) = el.value().attr("src") {
                return Ok(src.to_string());
            }
        }
        Err(HdRezkaError::ValueError("Thumbnail not found".to_string()))
    }

    /// Get high quality thumbnail URL.
    pub async fn thumbnail_hq(&self) -> Result<String, HdRezkaError> {
        let doc = self.get_document().await?;
        let sel = Selector::parse(".b-sidecover a").unwrap();
        if let Some(el) = doc.select(&sel).next() {
            if let Some(href) = el.value().attr("href") {
                return Ok(href.to_string());
            }
        }
        Err(HdRezkaError::ValueError("HQ thumbnail not found".to_string()))
    }

    /// Get release year.
    pub async fn release_year(&self) -> Result<Option<i32>, HdRezkaError> {
        let doc = self.get_document().await?;
        let sel = Selector::parse(".b-content__main .b-post__info a[href*=\"/year/\"]").unwrap();
        if let Some(el) = doc.select(&sel).next() {
            if let Some(href) = el.value().attr("href") {
                let re = regex::Regex::new(r"\d{4}").unwrap();
                if let Some(m) = re.find(href) {
                    if let Ok(year) = m.as_str().parse::<i32>() {
                        return Ok(Some(year));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Get content type (TVSeries or Movie).
    pub async fn content_type(&self) -> Result<HdRezkaFormat, HdRezkaError> {
        let doc = self.get_document().await?;
        let sel = Selector::parse("meta[property=\"og:type\"]").unwrap();
        if let Some(el) = doc.select(&sel).next() {
            if let Some(content) = el.value().attr("content") {
                return Ok(match content {
                    "video.tv_series" => HdRezkaFormat::TvSeries,
                    "video.movie" => HdRezkaFormat::Movie,
                    other => HdRezkaFormat::Other(other.to_string()),
                });
            }
        }
        Err(HdRezkaError::ValueError("Content type not found".to_string()))
    }

    /// Get category (Film, Series, Cartoon, Anime).
    pub async fn category(&self) -> Result<HdRezkaCategory, HdRezkaError> {
        let parsed = Url::parse(&self.url)?;
        let path = parsed.path().trim_start_matches('/');
        let cat = path.split('/').next().unwrap_or("");
        Ok(match cat {
            "films" => HdRezkaCategory::Film,
            "series" => HdRezkaCategory::Series,
            "cartoons" => HdRezkaCategory::Cartoon,
            "animation" => HdRezkaCategory::Anime,
            other => HdRezkaCategory::Other(other.to_string()),
        })
    }

    /// Get rating.
    pub async fn rating(&self) -> Result<HdRezkaRating, HdRezkaError> {
        let doc = self.get_document().await?;
        let wrapper_sel = Selector::parse(".b-post__rating").unwrap();
        if let Some(wrapper) = doc.select(&wrapper_sel).next() {
            let num_sel = Selector::parse(".num").unwrap();
            let votes_sel = Selector::parse(".votes").unwrap();
            if let (Some(num_el), Some(votes_el)) = (
                wrapper.select(&num_sel).next(),
                wrapper.select(&votes_sel).next(),
            ) {
                let rating_text: String = num_el.text().collect();
                let votes_text: String = votes_el.text().collect();
                let votes_clean = votes_text.trim().trim_matches(|c| c == '(' || c == ')');
                if let (Ok(value), Ok(votes)) = (
                    rating_text.trim().parse::<f64>(),
                    votes_clean.parse::<i64>(),
                ) {
                    return Ok(HdRezkaRating::new(value, votes));
                }
            }
        }
        Ok(HdRezkaRating::empty())
    }

    /// Get translators.
    pub async fn translators(&self) -> Result<HashMap<i64, TranslatorInfo>, HdRezkaError> {
        let mut arr: HashMap<i64, TranslatorInfo> = HashMap::new();
        let mut fallback_name: Option<String> = None;

        {
            let doc = self.get_document().await?;

            let sel = Selector::parse("#translators-list").unwrap();
            if let Some(list) = doc.select(&sel).next() {
                let child_sel = Selector::parse("#translators-list > *").unwrap();
                for child in doc.select(&child_sel) {
                    if let Some(id_str) = child.value().attr("data-translator_id") {
                        if let Ok(id) = id_str.parse::<i64>() {
                            let mut name: String = child.text().collect::<String>().trim().to_string();
                            let classes: Vec<&str> = child.value().classes().collect();
                            let premium = classes.contains(&"b-prem_translator");

                            // Check for language img
                            let img_sel = Selector::parse("img").unwrap();
                            if let Some(img) = child.select(&img_sel).next() {
                                if let Some(lang) = img.value().attr("title") {
                                    if !name.contains(lang) {
                                        name = format!("{} ({})", name, lang);
                                    }
                                }
                            }

                            arr.insert(id, TranslatorInfo { name, premium });
                        }
                    }
                }
                let _ = list;
            }

            if arr.is_empty() {
                fallback_name = self.get_translation_name(&doc);
            }
        } // doc is dropped here before the next .await

        if arr.is_empty() {
            // Auto-detect translator
            let content = self.fetch_page().await?;
            if let (Some(name), Some(id)) = (
                fallback_name,
                self.get_translation_id(content).await?,
            ) {
                arr.insert(
                    id,
                    TranslatorInfo {
                        name,
                        premium: false,
                    },
                );
            }
        }

        Ok(arr)
    }

    fn get_translation_name(&self, doc: &Html) -> Option<String> {
        let table_sel = Selector::parse(".b-post__info").unwrap();
        if let Some(table) = doc.select(&table_sel).next() {
            let tr_sel = Selector::parse("tr").unwrap();
            for tr in table.select(&tr_sel) {
                let text: String = tr.text().collect();
                if text.contains("переводе") {
                    if let Some(after) = text.split("В переводе:").nth(1) {
                        return Some(after.trim().to_string());
                    }
                }
            }
        }
        None
    }

    async fn get_translation_id(&self, content: &str) -> Result<Option<i64>, HdRezkaError> {
        let content_type = self.content_type().await?;
        let event_name = match content_type {
            HdRezkaFormat::TvSeries => "initCDNSeriesEvents",
            HdRezkaFormat::Movie => "initCDNMoviesEvents",
            _ => return Ok(None),
        };

        let search_str = format!("sof.tv.{}", event_name);
        if let Some(pos) = content.find(&search_str) {
            let after = &content[pos + search_str.len()..];
            // Find the arguments between the parentheses
            if let Some(paren_content) = after.split('{').next() {
                let parts: Vec<&str> = paren_content.split(',').collect();
                if parts.len() > 1 {
                    if let Ok(id) = parts[1].trim().parse::<i64>() {
                        return Ok(Some(id));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Get translators indexed by name.
    pub async fn translators_names(
        &self,
    ) -> Result<HashMap<String, TranslatorByName>, HdRezkaError> {
        let translators = self.translators().await?;
        let mut result = HashMap::new();
        for (id, info) in translators {
            result.insert(
                info.name.clone(),
                TranslatorByName {
                    id,
                    premium: info.premium,
                },
            );
        }
        Ok(result)
    }

    /// Sort translators by priority.
    pub fn sort_translators(
        &self,
        translators: &HashMap<i64, TranslatorInfo>,
        priority: Option<&[i64]>,
        non_priority: Option<&[i64]>,
    ) -> Vec<(i64, TranslatorInfo)> {
        let prior_list = priority.unwrap_or(&self.translators_priority);
        let non_prior_list = non_priority.unwrap_or(&self.translators_non_priority);

        let mut prior_map: HashMap<i64, usize> = HashMap::new();
        for (index, item) in prior_list.iter().enumerate() {
            prior_map.insert(*item, index + 1);
        }

        let max_index = prior_map.len() + 1;

        for (index, item) in non_prior_list.iter().enumerate() {
            prior_map
                .entry(*item)
                .or_insert(max_index + index + 1);
        }

        let mut sorted: Vec<(i64, TranslatorInfo)> = translators
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();

        sorted.sort_by_key(|(id, _)| *prior_map.get(id).unwrap_or(&max_index));
        sorted
    }

    /// Decode the obfuscated video URL data.
    pub fn clear_trash(data: &str) -> String {
        let trash_list: &[&str] = &["@", "#", "!", "^", "$"];
        let mut trash_codes_set: Vec<String> = Vec::new();

        for i in 2..=3 {
            let cartesian = Self::cartesian_product(trash_list, i);
            for chars in cartesian {
                let joined = chars.join("");
                let encoded = BASE64.encode(joined.as_bytes());
                trash_codes_set.push(encoded);
            }
        }

        let cleaned = data.replace("#h", "");
        let parts: Vec<&str> = cleaned.split("//_//").collect();
        let mut trash_string = parts.join("");

        for code in &trash_codes_set {
            trash_string = trash_string.replace(code.as_str(), "");
        }

        // Try base64 decode
        // Pad if needed
        let padded = format!("{}==", trash_string);
        match BASE64.decode(padded.as_bytes()) {
            Ok(decoded) => String::from_utf8_lossy(&decoded).to_string(),
            Err(_) => trash_string,
        }
    }

    fn cartesian_product(elements: &[&str], repeat: usize) -> Vec<Vec<String>> {
        if repeat == 0 {
            return vec![vec![]];
        }
        let sub = Self::cartesian_product(elements, repeat - 1);
        let mut result = Vec::new();
        for elem in elements {
            for s in &sub {
                let mut new = vec![elem.to_string()];
                new.extend(s.clone());
                result.push(new);
            }
        }
        result
    }

    /// Get other parts of the film/series.
    pub async fn other_parts(&self) -> Result<Vec<OtherPart>, HdRezkaError> {
        let doc = self.get_document().await?;
        let mut parts = Vec::new();

        let sel = Selector::parse(".b-post__partcontent").unwrap();
        if let Some(container) = doc.select(&sel).next() {
            let item_sel = Selector::parse(".b-post__partcontent_item").unwrap();
            for item in container.select(&item_sel) {
                let classes: Vec<&str> = item.value().classes().collect();
                let title_sel = Selector::parse(".title").unwrap();
                let title = item
                    .select(&title_sel)
                    .next()
                    .map(|el| el.text().collect::<String>())
                    .unwrap_or_default();

                let url = if classes.contains(&"current") {
                    self.url.clone()
                } else {
                    item.value()
                        .attr("data-url")
                        .unwrap_or("")
                        .to_string()
                };

                parts.push(OtherPart { name: title, url });
            }
        }

        Ok(parts)
    }

    fn parse_episodes(
        seasons_html: &str,
        episodes_html: &str,
    ) -> (HashMap<i64, String>, HashMap<i64, HashMap<i64, String>>) {
        let seasons_doc = Html::parse_fragment(seasons_html);
        let episodes_doc = Html::parse_fragment(episodes_html);

        let mut seasons: HashMap<i64, String> = HashMap::new();
        let season_sel = Selector::parse(".b-simple_season__item").unwrap();
        for s in seasons_doc.select(&season_sel) {
            if let Some(tab_id) = s.value().attr("data-tab_id") {
                if let Ok(id) = tab_id.parse::<i64>() {
                    seasons.insert(id, s.text().collect::<String>());
                }
            }
        }

        let mut episodes: HashMap<i64, HashMap<i64, String>> = HashMap::new();
        let ep_sel = Selector::parse(".b-simple_episode__item").unwrap();
        for ep in episodes_doc.select(&ep_sel) {
            if let (Some(season_id_str), Some(episode_id_str)) = (
                ep.value().attr("data-season_id"),
                ep.value().attr("data-episode_id"),
            ) {
                if let (Ok(season_id), Ok(episode_id)) = (
                    season_id_str.parse::<i64>(),
                    episode_id_str.parse::<i64>(),
                ) {
                    episodes
                        .entry(season_id)
                        .or_default()
                        .insert(episode_id, ep.text().collect::<String>());
                }
            }
        }

        (seasons, episodes)
    }

    /// Get series info for all translators (only for TV series).
    pub async fn series_info(
        &self,
    ) -> Result<HashMap<i64, SeriesTranslatorInfo>, HdRezkaError> {
        let content_type = self.content_type().await?;
        if content_type != HdRezkaFormat::TvSeries {
            return Err(HdRezkaError::ValueError(
                "The `series_info` method is only available for TVSeries.".to_string(),
            ));
        }

        let film_id = self.id().await?;
        let translators = self.translators().await?;
        let mut arr: HashMap<i64, SeriesTranslatorInfo> = HashMap::new();

        for (tr_id, tr_val) in &translators {
            let mut form = HashMap::new();
            form.insert("id", film_id.to_string());
            form.insert("translator_id", tr_id.to_string());
            form.insert("action", "get_episodes".to_string());

            let mut data: Option<Value> = None;
            for attempt in 0..3 {
                if attempt > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
                }

                let response = match self
                    .client
                    .post(format!("{}/ajax/get_cdn_series/", self.origin))
                    .headers(self.build_header_map())
                    .header("Cookie", self.cookie_header())
                    .form(&form)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("series_info: request failed for translator {} (attempt {}): {}", tr_id, attempt + 1, e);
                        continue;
                    }
                };

                let body = match response.text().await {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("series_info: failed to read response body for translator {} (attempt {}): {}", tr_id, attempt + 1, e);
                        continue;
                    }
                };

                match serde_json::from_str::<Value>(&body) {
                    Ok(v) => {
                        data = Some(v);
                        break;
                    }
                    Err(e) => {
                        eprintln!("series_info: invalid JSON for translator {} (attempt {}): {}", tr_id, attempt + 1, e);
                        continue;
                    }
                }
            }

            if let Some(d) = data {
                if d["success"].as_bool() == Some(true) {
                    let seasons_html = d["seasons"].as_str().unwrap_or("");
                    let episodes_html = d["episodes"].as_str().unwrap_or("");
                    let (seasons, episodes) = Self::parse_episodes(seasons_html, episodes_html);

                    arr.insert(
                        *tr_id,
                        SeriesTranslatorInfo {
                            translator_name: tr_val.name.clone(),
                            premium: tr_val.premium,
                            seasons,
                            episodes,
                        },
                    );
                }
            }
        }

        Ok(arr)
    }

    /// Get structured episodes info (only for TV series).
    pub async fn episodes_info(&self) -> Result<Vec<SeasonEpisodesInfo>, HdRezkaError> {
        let content_type = self.content_type().await?;
        if content_type != HdRezkaFormat::TvSeries {
            return Err(HdRezkaError::ValueError(
                "The `episodes_info` method is only available for TVSeries.".to_string(),
            ));
        }

        let series_info = self.series_info().await?;
        let mut output: Vec<SeasonEpisodesInfo> = Vec::new();

        for (translator_id, translator_info) in &series_info {
            for (season, season_text) in &translator_info.seasons {
                // Find or create season entry
                let season_obj = if let Some(pos) = output.iter().position(|s| s.season == *season)
                {
                    &mut output[pos]
                } else {
                    output.push(SeasonEpisodesInfo {
                        season: *season,
                        season_text: season_text.clone(),
                        episodes: Vec::new(),
                    });
                    output.last_mut().unwrap()
                };

                if let Some(eps) = translator_info.episodes.get(season) {
                    for (episode, episode_text) in eps {
                        // Find or create episode entry
                        let episode_obj = if let Some(pos) =
                            season_obj.episodes.iter().position(|e| e.episode == *episode)
                        {
                            &mut season_obj.episodes[pos]
                        } else {
                            season_obj.episodes.push(EpisodeInfo {
                                episode: *episode,
                                episode_text: episode_text.clone(),
                                translations: Vec::new(),
                            });
                            season_obj.episodes.last_mut().unwrap()
                        };

                        episode_obj.translations.push(EpisodeTranslation {
                            translator_id: *translator_id,
                            translator_name: translator_info.translator_name.clone(),
                            premium: translator_info.premium,
                        });
                    }
                }
            }
        }

        Ok(output)
    }

    /// Get a stream for a movie or a specific episode.
    pub async fn get_stream(
        &self,
        season: Option<i64>,
        episode: Option<i64>,
        translation: Option<&str>,
        priority: Option<&[i64]>,
        non_priority: Option<&[i64]>,
    ) -> Result<HdRezkaStream, HdRezkaError> {
        let film_id = self.id().await?;
        let film_name = self.name().await?;
        let content_type = self.content_type().await?;

        match content_type {
            HdRezkaFormat::TvSeries => {
                match (season, episode) {
                    (Some(s), Some(e)) => {
                        let episodes_info = self.episodes_info().await?;
                        let season_eps = episodes_info
                            .iter()
                            .find(|si| si.season == s)
                            .ok_or_else(|| {
                                HdRezkaError::ValueError(format!(
                                    "Season \"{}\" is not found!",
                                    s
                                ))
                            })?;

                        let ep_info = season_eps
                            .episodes
                            .iter()
                            .find(|ei| ei.episode == e)
                            .ok_or_else(|| {
                                HdRezkaError::ValueError(format!(
                                    "Episode \"{}\" in season \"{}\" is not found!",
                                    e, s
                                ))
                            })?;

                        let tr_id = self.resolve_translator_id(
                            &ep_info.translations,
                            translation,
                            priority,
                            non_priority,
                        )?;

                        self.fetch_stream_series(film_id, &film_name, s, e, tr_id)
                            .await
                    }
                    (Some(_), None) => Err(HdRezkaError::TypeError(
                        "get_stream() missing one required argument (episode)".to_string(),
                    )),
                    (None, Some(_)) => Err(HdRezkaError::TypeError(
                        "get_stream() missing one required argument (season)".to_string(),
                    )),
                    (None, None) => Err(HdRezkaError::TypeError(
                        "get_stream() missing required arguments (season and episode)".to_string(),
                    )),
                }
            }
            HdRezkaFormat::Movie => {
                let translators = self.translators().await?;
                let translations: Vec<EpisodeTranslation> = translators
                    .iter()
                    .map(|(id, info)| EpisodeTranslation {
                        translator_id: *id,
                        translator_name: info.name.clone(),
                        premium: info.premium,
                    })
                    .collect();

                let tr_id = self.resolve_translator_id(
                    &translations,
                    translation,
                    priority,
                    non_priority,
                )?;

                self.fetch_stream_movie(film_id, &film_name, tr_id).await
            }
            _ => Err(HdRezkaError::TypeError(
                "Undefined content type".to_string(),
            )),
        }
    }

    fn resolve_translator_id(
        &self,
        translations: &[EpisodeTranslation],
        translation: Option<&str>,
        priority: Option<&[i64]>,
        non_priority: Option<&[i64]>,
    ) -> Result<i64, HdRezkaError> {
        let translators_dict: HashMap<i64, TranslatorInfo> = translations
            .iter()
            .map(|t| {
                (
                    t.translator_id,
                    TranslatorInfo {
                        name: t.translator_name.clone(),
                        premium: t.premium,
                    },
                )
            })
            .collect();

        if let Some(translation) = translation {
            // Check if numeric
            if let Ok(id) = translation.parse::<i64>() {
                if translators_dict.contains_key(&id) {
                    return Ok(id);
                }
                return Err(HdRezkaError::ValueError(format!(
                    "Translation with code \"{}\" is not defined",
                    translation
                )));
            }

            // Check by name
            if let Some(t) = translations
                .iter()
                .find(|t| t.translator_name == translation)
            {
                return Ok(t.translator_id);
            }

            return Err(HdRezkaError::ValueError(format!(
                "Translation \"{}\" is not defined",
                translation
            )));
        }

        // Use priority sorting
        let sorted = self.sort_translators(&translators_dict, priority, non_priority);
        sorted
            .first()
            .map(|(id, _)| *id)
            .ok_or_else(|| HdRezkaError::ValueError("No translators available".to_string()))
    }

    async fn make_stream_request(
        &self,
        form: &HashMap<&str, String>,
        season: Option<i64>,
        episode: Option<i64>,
        name: &str,
        translator_id: i64,
    ) -> Result<HdRezkaStream, HdRezkaError> {
        let mut last_err = HdRezkaError::FetchFailed;

        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
            }

            let response = match self
                .client
                .post(format!("{}/ajax/get_cdn_series/", self.origin))
                .headers(self.build_header_map())
                .header("Cookie", self.cookie_header())
                .form(form)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_err = HdRezkaError::Request(e);
                    continue;
                }
            };

            let body = match response.text().await {
                Ok(b) => b,
                Err(e) => {
                    last_err = HdRezkaError::Request(e);
                    continue;
                }
            };

            let data: Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(_) => {
                    last_err = HdRezkaError::ValueError("Invalid response from server (not JSON)".to_string());
                    continue;
                }
            };

            if data["success"].as_bool() != Some(true) || data["url"].as_str().is_none() {
                last_err = HdRezkaError::FetchFailed;
                continue;
            }

            let url_data = data["url"].as_str().unwrap();
            let subtitle_data = data["subtitle"].as_str();
            let subtitle_lns: Option<HashMap<String, String>> =
                if let Some(lns) = data.get("subtitle_lns") {
                    serde_json::from_value(lns.clone()).ok()
                } else {
                    None
                };

            let decoded = Self::clear_trash(url_data);
            let mut stream = HdRezkaStream::new(
                season,
                episode,
                name.to_string(),
                translator_id,
                subtitle_data,
                subtitle_lns.as_ref(),
            );

            for item in decoded.split(',') {
                if let Some(bracket_start) = item.find('[') {
                    if let Some(bracket_end) = item[bracket_start..].find(']') {
                        let quality = &item[bracket_start + 1..bracket_start + bracket_end];
                        let rest = &item[bracket_start + bracket_end + 1..];
                        for link in rest.split(" or ") {
                            let link = link.trim();
                            if link.ends_with(".mp4") {
                                stream.append(quality.to_string(), link.to_string());
                            }
                        }
                    }
                }
            }

            return Ok(stream);
        }

        Err(last_err)
    }

    /// Get stream directly by translator_id without re-fetching episodes info.
    /// Use when the translator_id is already known (e.g., from the frontend).
    pub async fn get_stream_direct(
        &self,
        translator_id: i64,
        season: Option<i64>,
        episode: Option<i64>,
    ) -> Result<HdRezkaStream, HdRezkaError> {
        let film_id = self.id().await?;
        let film_name = self.name().await?;

        match (season, episode) {
            (Some(s), Some(e)) => {
                self.fetch_stream_series(film_id, &film_name, s, e, translator_id)
                    .await
            }
            (None, None) => {
                self.fetch_stream_movie(film_id, &film_name, translator_id)
                    .await
            }
            _ => Err(HdRezkaError::TypeError(
                "Both season and episode must be provided, or neither".to_string(),
            )),
        }
    }

    async fn fetch_stream_series(
        &self,
        film_id: i64,
        name: &str,
        season: i64,
        episode: i64,
        translator_id: i64,
    ) -> Result<HdRezkaStream, HdRezkaError> {
        let mut form = HashMap::new();
        form.insert("id", film_id.to_string());
        form.insert("translator_id", translator_id.to_string());
        form.insert("season", season.to_string());
        form.insert("episode", episode.to_string());
        form.insert("action", "get_stream".to_string());

        self.make_stream_request(&form, Some(season), Some(episode), name, translator_id)
            .await
    }

    async fn fetch_stream_movie(
        &self,
        film_id: i64,
        name: &str,
        translator_id: i64,
    ) -> Result<HdRezkaStream, HdRezkaError> {
        let mut form = HashMap::new();
        form.insert("id", film_id.to_string());
        form.insert("translator_id", translator_id.to_string());
        form.insert("action", "get_movie".to_string());

        self.make_stream_request(&form, None, None, name, translator_id)
            .await
    }

    /// Get streams for all episodes in a season.
    pub async fn get_season_streams(
        &self,
        season: i64,
        translation: Option<&str>,
        priority: Option<&[i64]>,
        non_priority: Option<&[i64]>,
        ignore: bool,
        progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<HashMap<i64, Option<HdRezkaStream>>, HdRezkaError> {
        let episodes_info = self.episodes_info().await?;
        let season_info = episodes_info
            .iter()
            .find(|s| s.season == season)
            .ok_or_else(|| {
                HdRezkaError::ValueError(format!("Season \"{}\" is not found!", season))
            })?;

        // Group episodes by translator
        let mut episodes_by_translator: HashMap<i64, Vec<i64>> = HashMap::new();
        for ep in &season_info.episodes {
            for t in &ep.translations {
                episodes_by_translator
                    .entry(t.translator_id)
                    .or_default()
                    .push(ep.episode);
            }
        }

        // Resolve translator
        let translators_dict: HashMap<i64, TranslatorInfo> = episodes_by_translator
            .keys()
            .filter_map(|id| {
                season_info
                    .episodes
                    .iter()
                    .flat_map(|e| &e.translations)
                    .find(|t| t.translator_id == *id)
                    .map(|t| {
                        (
                            *id,
                            TranslatorInfo {
                                name: t.translator_name.clone(),
                                premium: t.premium,
                            },
                        )
                    })
            })
            .collect();

        let tr_id = if let Some(translation) = translation {
            if let Ok(id) = translation.parse::<i64>() {
                if translators_dict.contains_key(&id) {
                    id
                } else {
                    return Err(HdRezkaError::ValueError(format!(
                        "Translation with code \"{}\" is not defined",
                        translation
                    )));
                }
            } else {
                translators_dict
                    .iter()
                    .find(|(_, v)| v.name == translation)
                    .map(|(k, _)| *k)
                    .ok_or_else(|| {
                        HdRezkaError::ValueError(format!(
                            "Translation \"{}\" is not defined",
                            translation
                        ))
                    })?
            }
        } else {
            let sorted = self.sort_translators(&translators_dict, priority, non_priority);
            sorted
                .first()
                .map(|(id, _)| *id)
                .ok_or_else(|| {
                    HdRezkaError::ValueError("No translators available".to_string())
                })?
        };

        let episodes = episodes_by_translator
            .get(&tr_id)
            .cloned()
            .unwrap_or_default();
        let total = episodes.len();
        let mut streams: HashMap<i64, Option<HdRezkaStream>> = HashMap::new();

        if let Some(p) = progress {
            p(0, total);
        }

        for ep in &episodes {
            let result = self
                .get_stream(
                    Some(season),
                    Some(*ep),
                    Some(&tr_id.to_string()),
                    None,
                    None,
                )
                .await;

            match result {
                Ok(stream) => {
                    streams.insert(*ep, Some(stream));
                }
                Err(e) => {
                    // Retry once
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let retry = self
                        .get_stream(
                            Some(season),
                            Some(*ep),
                            Some(&tr_id.to_string()),
                            None,
                            None,
                        )
                        .await;

                    match retry {
                        Ok(stream) => {
                            streams.insert(*ep, Some(stream));
                        }
                        Err(retry_err) => {
                            if ignore {
                                // Keep retrying if ignore is set
                                let mut success = false;
                                for _ in 0..5 {
                                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                    if let Ok(stream) = self
                                        .get_stream(
                                            Some(season),
                                            Some(*ep),
                                            Some(&tr_id.to_string()),
                                            None,
                                            None,
                                        )
                                        .await
                                    {
                                        streams.insert(*ep, Some(stream));
                                        success = true;
                                        break;
                                    }
                                }
                                if !success {
                                    streams.insert(*ep, None);
                                }
                            } else {
                                eprintln!(
                                    "{} > ep:{}: {}",
                                    std::any::type_name_of_val(&retry_err),
                                    ep,
                                    retry_err
                                );
                                streams.insert(*ep, None);
                            }
                            let _ = e;
                        }
                    }
                }
            }

            if let Some(p) = progress {
                p(streams.len(), total);
            }
        }

        Ok(streams)
    }
}
