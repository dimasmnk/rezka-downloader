use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use scraper::{Html, Selector};
use std::collections::HashMap;
use url::Url;

use crate::hdrezka::errors::HdRezkaError;
use crate::hdrezka::types::*;

/// Search client for HDRezka.
pub struct HdRezkaSearch {
    origin: String,
    proxy: Option<String>,
    cookies: HashMap<String, String>,
    headers: HashMap<String, String>,
    client: reqwest::Client,
}

impl HdRezkaSearch {
    pub fn new(
        origin: &str,
        proxy: Option<String>,
        headers: HashMap<String, String>,
        cookies: HashMap<String, String>,
    ) -> Self {
        let parsed = Url::parse(origin).unwrap_or_else(|_| Url::parse("http://localhost").unwrap());
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
            origin,
            proxy,
            cookies: merged_cookies,
            headers: merged_headers,
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

    /// Fast search — returns a simple list of results.
    pub async fn fast_search(&self, query: &str) -> Result<Vec<FastSearchResult>, HdRezkaError> {
        let mut form = HashMap::new();
        form.insert("q", query);

        let url = format!("{}/engine/ajax/search.php", self.origin);
        eprintln!("[fast_search] POST {} q={}", url, query);

        let response = self
            .client
            .post(&url)
            .headers(self.build_header_map())
            .header("Cookie", self.cookie_header())
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Referer", format!("{}/", self.origin))
            .form(&form)
            .send()
            .await?;

        eprintln!("[fast_search] status={}", response.status());

        if !response.status().is_success() {
            return Err(HdRezkaError::Http {
                code: response.status().as_u16(),
                message: response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string(),
            });
        }

        let body = response.text().await?;
        let preview: String = body.chars().take(500).collect();
        eprintln!("[fast_search] body length={}, preview: {}", body.len(), preview);
        let doc = Html::parse_document(&body);
        let mut results = Vec::new();

        let item_sel = Selector::parse(".b-search__section_list li").unwrap();
        let enty_sel = Selector::parse("span.enty").unwrap();
        let link_sel = Selector::parse("a").unwrap();
        let rating_sel = Selector::parse("span.rating").unwrap();

        for item in doc.select(&item_sel) {
            let title = item
                .select(&enty_sel)
                .next()
                .map(|el| el.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            let url = item
                .select(&link_sel)
                .next()
                .and_then(|el| el.value().attr("href"))
                .unwrap_or("")
                .to_string();

            let rating = item.select(&rating_sel).next().and_then(|el| {
                let text: String = el.text().collect();
                text.trim().parse::<f64>().ok()
            });

            results.push(FastSearchResult {
                title,
                url,
                rating,
            });
        }

        Ok(results)
    }

    /// Advanced search — returns paginated results.
    pub async fn advanced_search(
        &self,
        query: &str,
    ) -> Result<SearchResult, HdRezkaError> {
        Ok(SearchResult {
            origin: self.origin.clone(),
            query: query.to_string(),
            _proxy: self.proxy.clone(),
            headers: self.headers.clone(),
            cookies: self.cookies.clone(),
            client: self.client.clone(),
            cached_pages: std::sync::Mutex::new(HashMap::new()),
        })
    }

    /// Combined search — fast or advanced based on find_all flag.
    pub async fn search(
        &self,
        query: &str,
        find_all: bool,
    ) -> Result<SearchOutcome, HdRezkaError> {
        if find_all {
            let result = self.advanced_search(query).await?;
            Ok(SearchOutcome::Advanced(result))
        } else {
            let results = self.fast_search(query).await?;
            Ok(SearchOutcome::Fast(results))
        }
    }
}

/// Outcome of a search — either fast results or advanced paginated results.
pub enum SearchOutcome {
    Fast(Vec<FastSearchResult>),
    Advanced(SearchResult),
}

/// Paginated search result.
pub struct SearchResult {
    origin: String,
    pub query: String,
    _proxy: Option<String>,
    headers: HashMap<String, String>,
    cookies: HashMap<String, String>,
    client: reqwest::Client,
    cached_pages: std::sync::Mutex<HashMap<usize, Option<Vec<AdvancedSearchResult>>>>,
}

impl SearchResult {
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

    /// Get a specific page of results (1-indexed).
    pub async fn get_page(
        &self,
        page: usize,
    ) -> Result<Option<Vec<AdvancedSearchResult>>, HdRezkaError> {
        // Check cache
        {
            let cache = self.cached_pages.lock().unwrap();
            if let Some(cached) = cache.get(&page) {
                return Ok(cached.clone());
            }
        }

        let params = [
            ("do", "search"),
            ("subaction", "search"),
            ("q", &self.query),
            ("page", &page.to_string()),
        ];

        let url = format!("{}/search/", self.origin);
        eprintln!("[advanced_search] GET {} q={} page={}", url, self.query, page);

        let response = self
            .client
            .get(&url)
            .query(&params)
            .headers(self.build_header_map())
            .header("Cookie", self.cookie_header())
            .header("Referer", format!("{}/", self.origin))
            .send()
            .await?;

        eprintln!("[advanced_search] status={}, final_url={}", response.status(), response.url());

        if !response.status().is_success() {
            return Err(HdRezkaError::Http {
                code: response.status().as_u16(),
                message: response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string(),
            });
        }

        let body = response.text().await?;
        let preview: String = body.chars().take(500).collect();
        eprintln!("[advanced_search] body length={}, preview: {}", body.len(), preview);
        let doc = Html::parse_document(&body);

        // Check for login/captcha
        let title_sel = Selector::parse("title").unwrap();
        if let Some(title_el) = doc.select(&title_sel).next() {
            let title_text = title_el.text().collect::<String>();
            if title_text == "Sign In" {
                return Err(HdRezkaError::LoginRequired);
            }
            if title_text == "Verify" {
                return Err(HdRezkaError::CaptchaError);
            }
        }

        let item_sel = Selector::parse(".b-content__inline_item").unwrap();
        let items: Vec<_> = doc.select(&item_sel).collect();

        let result = if items.is_empty() {
            None
        } else {
            let mut results = Vec::new();
            for item in items {
                results.push(Self::process_item(&item));
            }
            Some(results)
        };

        // Cache result
        {
            let mut cache = self.cached_pages.lock().unwrap();
            cache.insert(page, result.clone());
        }

        Ok(result)
    }

    fn process_item(item: &scraper::ElementRef) -> AdvancedSearchResult {
        let link_sel = Selector::parse(".b-content__inline_item-link a").unwrap();
        let cover_sel = Selector::parse(".b-content__inline_item-cover img").unwrap();
        let cat_sel = Selector::parse(".cat").unwrap();

        let (title, url) = if let Some(link) = item.select(&link_sel).next() {
            (
                link.text().collect::<String>().trim().to_string(),
                link.value().attr("href").unwrap_or("").to_string(),
            )
        } else {
            (String::new(), String::new())
        };

        let image = item
            .select(&cover_sel)
            .next()
            .and_then(|el| el.value().attr("src"))
            .unwrap_or("")
            .to_string();

        let category = item.select(&cat_sel).next().and_then(|el| {
            let classes: Vec<&str> = el.value().classes().filter(|c| *c != "cat").collect();
            if classes.is_empty() {
                None
            } else {
                Some(Self::detect_type(&classes))
            }
        });

        AdvancedSearchResult {
            title,
            url,
            image,
            category,
        }
    }

    fn detect_type(classes: &[&str]) -> HdRezkaCategory {
        if classes.contains(&"films") {
            return HdRezkaCategory::Film;
        }
        if classes.contains(&"series") {
            return HdRezkaCategory::Series;
        }
        if classes.contains(&"cartoons") {
            return HdRezkaCategory::Cartoon;
        }
        if classes.contains(&"animation") {
            return HdRezkaCategory::Anime;
        }
        HdRezkaCategory::Other(classes.first().unwrap_or(&"unknown").to_string())
    }

    /// Get all pages as a vec of vecs.
    pub async fn all_pages(&self) -> Result<Vec<Vec<AdvancedSearchResult>>, HdRezkaError> {
        let mut pages = Vec::new();
        let mut page_num = 1;

        loop {
            match self.get_page(page_num).await? {
                Some(page) => {
                    pages.push(page);
                    page_num += 1;
                }
                None => break,
            }
        }

        Ok(pages)
    }

    /// Get all results flattened.
    pub async fn all(&self) -> Result<Vec<AdvancedSearchResult>, HdRezkaError> {
        let pages = self.all_pages().await?;
        Ok(pages.into_iter().flatten().collect())
    }
}

impl std::fmt::Display for SearchResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SearchResult({})", self.query)
    }
}
