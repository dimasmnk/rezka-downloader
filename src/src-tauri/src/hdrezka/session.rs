use std::collections::HashMap;
use url::Url;

use crate::hdrezka::api::HdRezkaApi;
use crate::hdrezka::errors::HdRezkaError;
use crate::hdrezka::search::{HdRezkaSearch, SearchOutcome};
use crate::hdrezka::types::*;

/// Session manager for HDRezka — login once, reuse credentials.
pub struct HdRezkaSession {
    pub origin: Option<String>,
    pub proxy: Option<String>,
    pub cookies: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub translators_priority: Vec<i64>,
    pub translators_non_priority: Vec<i64>,
}

impl HdRezkaSession {
    pub fn new(
        origin: Option<&str>,
        proxy: Option<String>,
        headers: HashMap<String, String>,
        cookies: HashMap<String, String>,
        translators_priority: Option<Vec<i64>>,
        translators_non_priority: Option<Vec<i64>>,
    ) -> Self {
        let parsed_origin = origin.map(|o| {
            let parsed = Url::parse(o).unwrap_or_else(|_| Url::parse("http://localhost").unwrap());
            format!(
                "{}://{}",
                parsed.scheme(),
                parsed.host_str().unwrap_or("localhost")
            )
        });

        let mut merged_cookies = default_cookies();
        for (k, v) in cookies {
            merged_cookies.insert(k, v);
        }

        let mut merged_headers = default_headers();
        for (k, v) in headers {
            merged_headers.insert(k, v);
        }

        Self {
            origin: parsed_origin,
            proxy,
            cookies: merged_cookies,
            headers: merged_headers,
            translators_priority: translators_priority
                .unwrap_or_else(default_translators_priority),
            translators_non_priority: translators_non_priority
                .unwrap_or_else(default_translators_non_priority),
        }
    }

    /// Create a lightweight clone for read-only operations like fetching user info.
    pub fn clone_for_info(&self) -> Self {
        Self {
            origin: self.origin.clone(),
            proxy: self.proxy.clone(),
            cookies: self.cookies.clone(),
            headers: self.headers.clone(),
            translators_priority: self.translators_priority.clone(),
            translators_non_priority: self.translators_non_priority.clone(),
        }
    }

    /// Login with email and password. Requires origin to be set.
    pub async fn login(&mut self, email: &str, password: &str) -> Result<bool, HdRezkaError> {
        let origin = self
            .origin
            .as_ref()
            .ok_or_else(|| HdRezkaError::ValueError("For login origin is required".to_string()))?
            .clone();

        let mut api = HdRezkaApi::new(
            &origin,
            self.proxy.clone(),
            self.headers.clone(),
            HashMap::new(),
            None,
            None,
        );

        if api.login(email, password).await? {
            // Merge cookies from the login response
            for (k, v) in &api.cookies {
                self.cookies.insert(k.clone(), v.clone());
            }
            return Ok(true);
        }

        Ok(false)
    }

    /// Get an HdRezkaApi instance for a URL, using session credentials.
    pub fn get(&self, url: &str) -> Result<HdRezkaApi, HdRezkaError> {
        let final_url = if let Some(ref origin) = self.origin {
            let parsed = Url::parse(url);
            match parsed {
                Ok(u) => {
                    // Replace origin but keep path
                    format!("{}/{}", origin.trim_end_matches('/'), u.path().trim_start_matches('/'))
                }
                Err(_) => {
                    // Assume it's just a path
                    format!(
                        "{}/{}",
                        origin.trim_end_matches('/'),
                        url.trim_start_matches('/')
                    )
                }
            }
        } else {
            url.to_string()
        };

        Ok(HdRezkaApi::new(
            &final_url,
            self.proxy.clone(),
            self.headers.clone(),
            self.cookies.clone(),
            Some(self.translators_priority.clone()),
            Some(self.translators_non_priority.clone()),
        ))
    }

    /// Search for films. Requires origin to be set.
    pub async fn search(
        &self,
        query: &str,
        find_all: bool,
    ) -> Result<SearchOutcome, HdRezkaError> {
        let origin = self
            .origin
            .as_ref()
            .ok_or_else(|| {
                HdRezkaError::ValueError("For search origin is required".to_string())
            })?;

        let search = HdRezkaSearch::new(
            origin,
            self.proxy.clone(),
            self.headers.clone(),
            self.cookies.clone(),
        );

        search.search(query, find_all).await
    }
}
