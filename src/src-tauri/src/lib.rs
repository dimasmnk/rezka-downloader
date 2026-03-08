pub mod config;
pub mod download;
pub mod hdrezka;

use config::{AppConfig, SessionData};
use download::{DownloadManager, DownloadManagerState};
use hdrezka::{HdRezkaSession, SearchOutcome};
use hdrezka::types::*;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

pub struct AppState {
    pub session: Option<HdRezkaSession>,
    pub config: AppConfig,
    pub app_data_dir: PathBuf,
}

type AppStateWrapper = Mutex<AppState>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub username: String,
    pub avatar: Option<String>,
}

async fn fetch_user_info(session: &HdRezkaSession) -> Result<UserInfo, String> {
    let origin = session
        .origin
        .as_ref()
        .ok_or("Origin not set")?;

    // First check: if we have dle_user_id cookie, login was successful
    let has_session_cookie = session.cookies.contains_key("dle_user_id");
    if !has_session_cookie {
        return Err("Not logged in (no session cookie)".to_string());
    }

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .gzip(true)
        .build()
        .map_err(|e| e.to_string())?;

    let cookie_header: String = session
        .cookies
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("; ");

    let mut headers = HeaderMap::new();
    for (k, v) in &session.headers {
        if let (Ok(name), Ok(val)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            headers.insert(name, val);
        }
    }

    let response = client
        .get(origin.as_str())
        .headers(headers)
        .header("Cookie", &cookie_header)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let body = response.text().await.map_err(|e| e.to_string())?;
    let doc = Html::parse_document(&body);

    let username = extract_username(&doc);
    let avatar = extract_avatar(&doc);

    // Fallback: if we couldn't parse the username from HTML but have cookies,
    // use the dle_user_id as a fallback username
    let final_username = username.unwrap_or_else(|| {
        session
            .cookies
            .get("dle_user_id")
            .cloned()
            .unwrap_or_else(|| "User".to_string())
    });

    Ok(UserInfo {
        username: final_username,
        avatar,
    })
}

fn extract_username(doc: &Html) -> Option<String> {
    // Try various selectors for HDRezka's logged-in user panel
    let selectors = [
        // Profile link in the auth/user bar
        ".b-tophead-auth .b-tophead-auth__item a",
        ".b-tophead__auth .b-tophead__auth-item a",
        ".b-tophead__auth-item-link span",
        ".b-tophead__auth a span",
        ".b-tophead__userpanel .user-name",
        ".b-user-panel .user-name",
    ];
    for sel_str in &selectors {
        if let Ok(sel) = Selector::parse(sel_str) {
            for el in doc.select(&sel) {
                let text: String = el.text().collect::<String>().trim().to_string();
                // Skip empty, generic links like "Login", "Sign In"
                if !text.is_empty()
                    && !text.eq_ignore_ascii_case("login")
                    && !text.eq_ignore_ascii_case("sign in")
                    && !text.eq_ignore_ascii_case("войти")
                    && !text.eq_ignore_ascii_case("вход")
                {
                    return Some(text);
                }
            }
        }
    }

    // Fallback: look for any link to /user/ profile page
    if let Ok(sel) = Selector::parse("a[href*='/user/']") {
        for el in doc.select(&sel) {
            // Only consider links inside the header area
            let text: String = el.text().collect::<String>().trim().to_string();
            if !text.is_empty() && text.len() < 50 {
                return Some(text);
            }
        }
    }

    None
}

fn extract_avatar(doc: &Html) -> Option<String> {
    let selectors = [
        ".b-tophead-auth img",
        ".b-tophead__auth img",
        ".b-tophead__auth-item-link img",
        ".b-tophead__userpanel img",
        ".b-user-panel img",
    ];
    for sel_str in &selectors {
        if let Ok(sel) = Selector::parse(sel_str) {
            if let Some(el) = doc.select(&sel).next() {
                if let Some(src) = el.value().attr("src") {
                    return Some(src.to_string());
                }
            }
        }
    }
    None
}

#[tauri::command]
async fn login(
    email: String,
    password: String,
    state: State<'_, AppStateWrapper>,
) -> Result<UserInfo, String> {
    let (origin, proxy, headers, cookies) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (
            s.config.origin.clone(),
            None::<String>,
            HashMap::new(),
            HashMap::new(),
        )
    };

    let mut session = HdRezkaSession::new(
        Some(&origin),
        proxy,
        headers,
        cookies,
        None,
        None,
    );

    session
        .login(&email, &password)
        .await
        .map_err(|e| e.to_string())?;

    let user_info = fetch_user_info(&session).await?;

    // Save session to disk
    let session_path = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.config.session_file_path(&s.app_data_dir)
    };

    let session_data = SessionData {
        cookies: session.cookies.clone(),
        origin: origin.clone(),
    };
    session_data.save(&session_path)?;

    // Store session in state
    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        s.session = Some(session);
    }

    Ok(user_info)
}

#[tauri::command]
async fn logout(state: State<'_, AppStateWrapper>) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.session = None;
    let session_path = s.config.session_file_path(&s.app_data_dir);
    SessionData::delete(&session_path)?;
    Ok(())
}

#[tauri::command]
async fn get_user_info(state: State<'_, AppStateWrapper>) -> Result<UserInfo, String> {
    let session = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.session.as_ref().ok_or("Not logged in")?.clone_for_info()
    };
    fetch_user_info(&session).await
}

#[tauri::command]
async fn restore_session(state: State<'_, AppStateWrapper>) -> Result<Option<UserInfo>, String> {
    let session_path = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.config.session_file_path(&s.app_data_dir)
    };

    let session_data = match SessionData::load(&session_path) {
        Some(data) => data,
        None => return Ok(None),
    };

    let session = HdRezkaSession::new(
        Some(&session_data.origin),
        None,
        HashMap::new(),
        session_data.cookies,
        None,
        None,
    );

    let user_info = match fetch_user_info(&session).await {
        Ok(info) => info,
        Err(_) => {
            // Session expired or invalid — clean up
            let _ = SessionData::delete(&session_path);
            return Ok(None);
        }
    };

    {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        s.session = Some(session);
    }

    Ok(Some(user_info))
}

#[tauri::command]
async fn get_config(state: State<'_, AppStateWrapper>) -> Result<AppConfig, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.config.clone())
}

#[tauri::command]
async fn set_config(
    config: AppConfig,
    state: State<'_, AppStateWrapper>,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    config.save(&s.app_data_dir)?;
    s.config = config;
    Ok(())
}

#[tauri::command]
async fn fast_search(
    query: String,
    state: State<'_, AppStateWrapper>,
) -> Result<Vec<FastSearchResult>, String> {
    let session = {
        let s = state.lock().map_err(|e| e.to_string())?;
        match &s.session {
            Some(sess) => sess.clone_for_info(),
            None => {
                // Create a session without login using just the origin
                HdRezkaSession::new(
                    Some(&s.config.origin),
                    None,
                    HashMap::new(),
                    HashMap::new(),
                    None,
                    None,
                )
            }
        }
    };

    let outcome = session
        .search(&query, false)
        .await
        .map_err(|e| e.to_string())?;

    match outcome {
        SearchOutcome::Fast(results) => Ok(results),
        _ => Ok(vec![]),
    }
}

#[tauri::command]
async fn search(
    query: String,
    state: State<'_, AppStateWrapper>,
) -> Result<Vec<AdvancedSearchResult>, String> {
    let session = {
        let s = state.lock().map_err(|e| e.to_string())?;
        match &s.session {
            Some(sess) => sess.clone_for_info(),
            None => {
                HdRezkaSession::new(
                    Some(&s.config.origin),
                    None,
                    HashMap::new(),
                    HashMap::new(),
                    None,
                    None,
                )
            }
        }
    };

    let outcome = session
        .search(&query, true)
        .await
        .map_err(|e| e.to_string())?;

    match outcome {
        SearchOutcome::Advanced(result) => {
            result.all().await.map_err(|e| e.to_string())
        }
        _ => Ok(vec![]),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslatorItem {
    pub id: i64,
    pub name: String,
    pub premium: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieInfo {
    pub title: String,
    pub orig_title: Option<String>,
    pub image: Option<String>,
    pub year: Option<i32>,
    pub description: Option<String>,
    pub content_type: String,
    pub translators: Vec<TranslatorItem>,
    pub seasons: Option<Vec<SeasonEpisodesInfo>>,
    pub rating: Option<f64>,
}

#[tauri::command]
async fn get_bookmarks(
    state: State<'_, AppStateWrapper>,
) -> Result<Vec<AdvancedSearchResult>, String> {
    let session = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.session.as_ref().ok_or("Not logged in")?.clone_for_info()
    };

    let origin = session.origin.as_ref().ok_or("Origin not set")?.clone();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .gzip(true)
        .build()
        .map_err(|e| e.to_string())?;

    let cookie_header: String = session
        .cookies
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("; ");

    let mut headers = HeaderMap::new();
    for (k, v) in &session.headers {
        if let (Ok(name), Ok(val)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            headers.insert(name, val);
        }
    }

    let url = format!("{}/favorites/", origin);
    let response = client
        .get(&url)
        .headers(headers)
        .header("Cookie", &cookie_header)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let body = response.text().await.map_err(|e| e.to_string())?;
    let doc = Html::parse_document(&body);

    let item_sel = Selector::parse(".b-content__inline_item").unwrap();
    let link_sel = Selector::parse(".b-content__inline_item-link a").unwrap();
    let cover_sel = Selector::parse(".b-content__inline_item-cover img").unwrap();
    let cat_sel = Selector::parse(".cat").unwrap();

    let mut results = Vec::new();
    for item in doc.select(&item_sel) {
        let (title, item_url) = if let Some(link) = item.select(&link_sel).next() {
            (
                link.text().collect::<String>().trim().to_string(),
                link.value().attr("href").unwrap_or("").to_string(),
            )
        } else {
            continue;
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
            } else if classes.contains(&"films") {
                Some(HdRezkaCategory::Film)
            } else if classes.contains(&"series") {
                Some(HdRezkaCategory::Series)
            } else {
                None
            }
        });

        results.push(AdvancedSearchResult {
            title,
            url: item_url,
            image,
            category,
        });
    }

    Ok(results)
}

#[tauri::command]
async fn get_movie_info(
    url: String,
    state: State<'_, AppStateWrapper>,
) -> Result<MovieInfo, String> {
    let session = {
        let s = state.lock().map_err(|e| e.to_string())?;
        match &s.session {
            Some(sess) => sess.clone_for_info(),
            None => {
                HdRezkaSession::new(
                    Some(&s.config.origin),
                    None,
                    HashMap::new(),
                    HashMap::new(),
                    None,
                    None,
                )
            }
        }
    };

    let api = session.get(&url).map_err(|e| e.to_string())?;

    let title = api.name().await.map_err(|e| e.to_string())?;
    let orig_title = api.orig_name().await.unwrap_or(None);
    let image = api.thumbnail_hq().await.ok().or_else(|| None);
    let year = api.release_year().await.unwrap_or(None);
    let description = api.description().await.ok();
    let content_type_val = api.content_type().await.map_err(|e| e.to_string())?;
    let rating_val = api.rating().await.unwrap_or(HdRezkaRating::empty());

    let translators_map = api.translators().await.map_err(|e| e.to_string())?;
    let sorted = api.sort_translators(&translators_map, None, None);
    let translators: Vec<TranslatorItem> = sorted
        .into_iter()
        .map(|(id, info)| TranslatorItem {
            id,
            name: info.name,
            premium: info.premium,
        })
        .collect();

    let seasons = if content_type_val.is_tv_series() {
        Some(api.episodes_info().await.map_err(|e| e.to_string())?)
    } else {
        None
    };

    let content_type_str = match content_type_val {
        HdRezkaFormat::Movie => "movie".to_string(),
        HdRezkaFormat::TvSeries => "tv_series".to_string(),
        HdRezkaFormat::Other(s) => s,
    };

    Ok(MovieInfo {
        title,
        orig_title,
        image,
        year,
        description,
        content_type: content_type_str,
        translators,
        seasons,
        rating: rating_val.value,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityOption {
    pub quality: String,
    pub urls: Vec<String>,
}

fn extract_resolution(q: &str) -> i32 {
    let re = regex::Regex::new(r"(\d+)").unwrap();
    re.captures(q)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0)
}

#[tauri::command]
async fn get_stream_info(
    url: String,
    translator_id: Option<i64>,
    season: Option<i64>,
    episode: Option<i64>,
    state: State<'_, AppStateWrapper>,
) -> Result<Vec<QualityOption>, String> {
    let session = {
        let s = state.lock().map_err(|e| e.to_string())?;
        match &s.session {
            Some(sess) => sess.clone_for_info(),
            None => {
                HdRezkaSession::new(
                    Some(&s.config.origin),
                    None,
                    HashMap::new(),
                    HashMap::new(),
                    None,
                    None,
                )
            }
        }
    };

    let api = session.get(&url).map_err(|e| e.to_string())?;
    let stream = if let Some(tr_id) = translator_id {
        // Use direct method: skips redundant episodes_info() re-fetch
        api.get_stream_direct(tr_id, season, episode)
            .await
            .map_err(|e| e.to_string())?
    } else {
        api.get_stream(season, episode, None, None, None)
            .await
            .map_err(|e| e.to_string())?
    };

    let mut qualities: Vec<QualityOption> = stream
        .videos()
        .iter()
        .map(|(quality, urls)| QualityOption {
            quality: quality.clone(),
            urls: urls.clone(),
        })
        .collect();

    qualities.sort_by(|a, b| {
        let res_a = extract_resolution(&a.quality);
        let res_b = extract_resolution(&b.quality);
        match res_b.cmp(&res_a) {
            std::cmp::Ordering::Equal => {
                // "Ultra" variants come first within the same resolution
                let ultra_a = a.quality.to_lowercase().contains("ultra");
                let ultra_b = b.quality.to_lowercase().contains("ultra");
                ultra_b.cmp(&ultra_a)
            }
            other => other,
        }
    });

    Ok(qualities)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonQueueProgress {
    pub queued: usize,
    pub total: usize,
    pub episode: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonDownloadResult {
    pub episode: i64,
    pub task_id: Option<String>,
    pub quality: Option<String>,
    pub error: Option<String>,
}

#[tauri::command]
async fn start_season_download(
    url: String,
    translator_id: i64,
    season: i64,
    quality: String,
    app: tauri::AppHandle,
    state: State<'_, AppStateWrapper>,
    dm_state: State<'_, DownloadManagerState>,
) -> Result<Vec<SeasonDownloadResult>, String> {
    let session = {
        let s = state.lock().map_err(|e| e.to_string())?;
        match &s.session {
            Some(sess) => sess.clone_for_info(),
            None => {
                HdRezkaSession::new(
                    Some(&s.config.origin),
                    None,
                    HashMap::new(),
                    HashMap::new(),
                    None,
                    None,
                )
            }
        }
    };

    let api = session.get(&url).map_err(|e| e.to_string())?;
    let movie_title = api.name().await.map_err(|e| e.to_string())?;

    // Get episodes info to know which episodes are in this season for this translator
    let episodes_info = api.episodes_info().await.map_err(|e| e.to_string())?;
    let season_info = episodes_info
        .iter()
        .find(|s| s.season == season)
        .ok_or_else(|| format!("Season {} not found", season))?;

    let mut episodes: Vec<i64> = season_info
        .episodes
        .iter()
        .filter(|ep| ep.translations.iter().any(|t| t.translator_id == translator_id))
        .map(|ep| ep.episode)
        .collect();
    episodes.sort();

    let default_download_dir = app
        .path()
        .download_dir()
        .unwrap_or_else(|_| PathBuf::from("Downloads"));

    let (download_dir, thread_count, origin) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let dir = match &s.config.download_dir {
            Some(d) if !d.is_empty() => PathBuf::from(d),
            _ => default_download_dir,
        };
        (dir, s.config.thread_count as usize, s.config.origin.clone())
    };

    let mut results = Vec::new();
    let total_episodes = episodes.len();

    for ep in &episodes {
        // Small delay between episodes to avoid rate limiting
        if ep != episodes.first().unwrap() {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }

        // Try to get stream with exact quality, retry if quality is missing
        let mut best_stream = None;
        for attempt in 0..3u64 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt)).await;
            }
            match api
                .get_stream_direct(translator_id, Some(season), Some(*ep))
                .await
            {
                Ok(stream) => {
                    if stream.videos().contains_key(&quality) {
                        // Exact quality found — use it immediately
                        best_stream = Some(stream);
                        break;
                    }
                    // Quality not found — keep this as fallback but retry
                    if best_stream.is_none() {
                        best_stream = Some(stream);
                    }
                }
                Err(e) => {
                    eprintln!("get_stream_direct failed for S{}E{} (attempt {}): {}", season, ep, attempt + 1, e);
                    // On last attempt, if we have no stream at all, record error
                    if attempt == 2 && best_stream.is_none() {
                        results.push(SeasonDownloadResult {
                            episode: *ep,
                            task_id: None,
                            quality: None,
                            error: Some(e.to_string()),
                        });

                        let _ = app.emit("season-queue-progress", SeasonQueueProgress {
                            queued: results.len(),
                            total: total_episodes,
                            episode: *ep,
                        });
                        continue;
                    }
                }
            }
        }

        // If we broke out due to error on all attempts with no stream
        let stream = match best_stream {
            Some(s) => s,
            None => continue,
        };

        {
            // Find the requested quality: exact match first, then fallback to same resolution
            let video_url = stream
                .videos()
                .get(&quality)
                .and_then(|urls| urls.first().cloned())
                .or_else(|| {
                    // Fallback only if exact quality not available at all:
                    // pick the best quality at the same resolution
                    let target_res = extract_resolution(&quality);
                    if target_res > 0 {
                        let mut candidates: Vec<_> = stream.videos().iter()
                            .filter(|(q, _)| extract_resolution(q) == target_res)
                            .collect();
                        candidates.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()));
                        candidates.first().and_then(|(_, urls)| urls.first().cloned())
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    // Last fallback: best available quality overall
                    let mut sorted: Vec<_> = stream.videos().iter().collect();
                    sorted.sort_by(|(a, _), (b, _)| {
                        let res_cmp = extract_resolution(b).cmp(&extract_resolution(a));
                        if res_cmp == std::cmp::Ordering::Equal {
                            b.len().cmp(&a.len())
                        } else {
                            res_cmp
                        }
                    });
                    sorted.first().and_then(|(_, urls)| urls.first().cloned())
                });

            match video_url {
                Some(vurl) => {
                    let title = format!("{} S{}E{}", movie_title, season, ep);
                    let actual_quality = stream
                        .videos()
                        .iter()
                        .find(|(_, urls)| urls.contains(&vurl))
                        .map(|(q, _)| q.clone())
                        .unwrap_or_else(|| quality.clone());

                    let should_start;
                    let task_id;
                    {
                        let mut dm = dm_state.lock().map_err(|e| e.to_string())?;
                        dm.download_dir = download_dir.clone();
                        dm.thread_count = thread_count;
                        dm.origin = origin.clone();
                        should_start = !dm.processing;
                        task_id = dm.add_task(vurl, title, actual_quality.clone());
                    }

                    if should_start {
                        download::start_queue_processing(dm_state.inner().clone(), app.clone());
                    }

                    results.push(SeasonDownloadResult {
                        episode: *ep,
                        task_id: Some(task_id),
                        quality: Some(actual_quality),
                        error: None,
                    });
                }
                None => {
                    results.push(SeasonDownloadResult {
                        episode: *ep,
                        task_id: None,
                        quality: None,
                        error: Some("No video URL available".to_string()),
                    });
                }
            }
        }

        let _ = app.emit("season-queue-progress", SeasonQueueProgress {
            queued: results.len(),
            total: total_episodes,
            episode: *ep,
        });
    }

    Ok(results)
}

#[tauri::command]
async fn start_download(
    video_url: String,
    title: String,
    quality: String,
    app: tauri::AppHandle,
    state: State<'_, AppStateWrapper>,
    dm_state: State<'_, DownloadManagerState>,
) -> Result<String, String> {
    let default_download_dir = app
        .path()
        .download_dir()
        .unwrap_or_else(|_| PathBuf::from("Downloads"));

    let (download_dir, thread_count, origin) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let dir = match &s.config.download_dir {
            Some(d) if !d.is_empty() => PathBuf::from(d),
            _ => default_download_dir,
        };
        (dir, s.config.thread_count as usize, s.config.origin.clone())
    };

    let should_start;
    let task_id;

    {
        let mut dm = dm_state.lock().map_err(|e| e.to_string())?;
        dm.download_dir = download_dir;
        dm.thread_count = thread_count;
        dm.origin = origin;
        should_start = !dm.processing;
        task_id = dm.add_task(video_url, title, quality);
    }

    if should_start {
        download::start_queue_processing(dm_state.inner().clone(), app.clone());
    }

    Ok(task_id)
}

#[tauri::command]
async fn get_downloads(
    dm_state: State<'_, DownloadManagerState>,
) -> Result<Vec<download::DownloadTaskView>, String> {
    let dm = dm_state.lock().map_err(|e| e.to_string())?;
    Ok(dm.get_views())
}

#[tauri::command]
async fn cancel_download(
    id: String,
    dm_state: State<'_, DownloadManagerState>,
) -> Result<(), String> {
    let mut dm = dm_state.lock().map_err(|e| e.to_string())?;
    dm.cancel_task(&id);
    Ok(())
}

#[tauri::command]
async fn remove_download(
    id: String,
    dm_state: State<'_, DownloadManagerState>,
) -> Result<(), String> {
    let mut dm = dm_state.lock().map_err(|e| e.to_string())?;
    dm.remove_task(&id);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");

            let config = AppConfig::load(&app_data_dir);

            let download_dir = match &config.download_dir {
                Some(d) if !d.is_empty() => PathBuf::from(d),
                _ => app
                    .path()
                    .download_dir()
                    .unwrap_or_else(|_| app_data_dir.join("downloads")),
            };

            let dm = DownloadManager::new(download_dir, config.thread_count as usize, config.origin.clone());

            app.manage(Mutex::new(AppState {
                session: None,
                config,
                app_data_dir,
            }));

            app.manage(Arc::new(Mutex::new(dm)) as DownloadManagerState);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            login,
            logout,
            get_user_info,
            restore_session,
            get_config,
            set_config,
            fast_search,
            search,
            get_movie_info,
            get_stream_info,
            start_download,
            start_season_download,
            get_downloads,
            cancel_download,
            remove_download,
            get_bookmarks,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
