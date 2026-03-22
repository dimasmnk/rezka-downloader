use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Serialize)]
pub struct DownloadProgressEvent {
    pub id: String,
    pub status: DownloadStatus,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct DownloadTaskView {
    pub id: String,
    pub title: String,
    pub quality: String,
    pub status: DownloadStatus,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
    pub file_path: String,
}

pub struct DownloadEntry {
    pub id: String,
    pub title: String,
    pub quality: String,
    pub url: String,
    pub file_path: PathBuf,
    pub status: DownloadStatus,
    pub total_bytes: Arc<AtomicU64>,
    pub downloaded_bytes: Arc<AtomicU64>,
    pub cancel: Arc<AtomicBool>,
    pub error: Option<String>,
}

impl DownloadEntry {
    pub fn to_view(&self) -> DownloadTaskView {
        DownloadTaskView {
            id: self.id.clone(),
            title: self.title.clone(),
            quality: self.quality.clone(),
            status: self.status.clone(),
            downloaded_bytes: self.downloaded_bytes.load(Ordering::Relaxed),
            total_bytes: self.total_bytes.load(Ordering::Relaxed),
            error: self.error.clone(),
            file_path: self.file_path.to_string_lossy().to_string(),
        }
    }

    pub fn to_progress_event(&self) -> DownloadProgressEvent {
        DownloadProgressEvent {
            id: self.id.clone(),
            status: self.status.clone(),
            downloaded_bytes: self.downloaded_bytes.load(Ordering::Relaxed),
            total_bytes: self.total_bytes.load(Ordering::Relaxed),
            error: self.error.clone(),
        }
    }
}

pub struct DownloadManager {
    pub tasks: Vec<DownloadEntry>,
    pub processing: bool,
    next_id: u64,
    pub download_dir: PathBuf,
    pub thread_count: usize,
    pub origin: String,
}

impl DownloadManager {
    pub fn new(download_dir: PathBuf, thread_count: usize, origin: String) -> Self {
        Self {
            tasks: Vec::new(),
            processing: false,
            next_id: 1,
            download_dir,
            thread_count,
            origin,
        }
    }

    pub fn add_task(&mut self, url: String, title: String, quality: String) -> String {
        let id = format!("dl_{}", self.next_id);
        self.next_id += 1;

        let filename = sanitize_filename(&format!("{} [{}].mp4", title, quality));
        let file_path = self.download_dir.join(&filename);

        let entry = DownloadEntry {
            id: id.clone(),
            title,
            quality,
            url,
            file_path,
            status: DownloadStatus::Queued,
            total_bytes: Arc::new(AtomicU64::new(0)),
            downloaded_bytes: Arc::new(AtomicU64::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            error: None,
        };

        self.tasks.push(entry);
        id
    }

    pub fn get_views(&self) -> Vec<DownloadTaskView> {
        self.tasks.iter().map(|t| t.to_view()).collect()
    }

    pub fn cancel_task(&mut self, id: &str) -> bool {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            if task.status == DownloadStatus::Queued {
                task.status = DownloadStatus::Cancelled;
                return true;
            }
            if task.status == DownloadStatus::Downloading {
                task.cancel.store(true, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    pub fn remove_task(&mut self, id: &str) -> bool {
        if let Some(pos) = self.tasks.iter().position(|t| t.id == id) {
            let task = &self.tasks[pos];
            if task.status == DownloadStatus::Downloading {
                task.cancel.store(true, Ordering::Relaxed);
            }
            self.tasks.remove(pos);
            true
        } else {
            false
        }
    }
}

pub type DownloadManagerState = Arc<Mutex<DownloadManager>>;

pub fn start_queue_processing(dm: DownloadManagerState, app: AppHandle) {
    tokio::spawn(async move {
        process_queue(dm, app).await;
    });
}

async fn process_queue(dm: DownloadManagerState, app: AppHandle) {
    loop {
        let task_info = {
            let mut manager = dm.lock().unwrap();
            let task_idx = manager
                .tasks
                .iter()
                .position(|t| t.status == DownloadStatus::Queued);
            match task_idx {
                Some(idx) => {
                    manager.tasks[idx].status = DownloadStatus::Downloading;
                    manager.processing = true;
                    let thread_count = manager.thread_count;
                    let origin = manager.origin.clone();
                    let t = &manager.tasks[idx];
                    (
                        t.id.clone(),
                        t.url.clone(),
                        t.file_path.clone(),
                        t.downloaded_bytes.clone(),
                        t.total_bytes.clone(),
                        t.cancel.clone(),
                        thread_count,
                        origin,
                    )
                }
                None => {
                    manager.processing = false;
                    return;
                }
            }
        };

        let (id, url, file_path, downloaded, total, cancel, thread_count, origin) = task_info;

        // Emit status update
        emit_task_update(&dm, &id, &app);

        // Start progress monitor
        let monitor_cancel = Arc::new(AtomicBool::new(false));
        let monitor = {
            let dm = dm.clone();
            let app = app.clone();
            let id = id.clone();
            let mc = monitor_cancel.clone();
            tokio::spawn(async move {
                while !mc.load(Ordering::Relaxed) {
                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                    emit_task_update(&dm, &id, &app);
                }
            })
        };

        // Ensure download directory exists
        if let Some(parent) = file_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        // Perform download
        let result = download_file(&url, &file_path, thread_count, downloaded, total, cancel.clone(), &origin).await;

        // Stop monitor
        monitor_cancel.store(true, Ordering::Relaxed);
        monitor.abort();

        // Update task status
        {
            let mut manager = dm.lock().unwrap();
            if let Some(t) = manager.tasks.iter_mut().find(|t| t.id == id) {
                match result {
                    Ok(()) => {
                        if t.cancel.load(Ordering::Relaxed) {
                            t.status = DownloadStatus::Cancelled;
                        } else {
                            t.status = DownloadStatus::Completed;
                        }
                    }
                    Err(e) => {
                        if t.cancel.load(Ordering::Relaxed) {
                            t.status = DownloadStatus::Cancelled;
                        } else {
                            t.status = DownloadStatus::Failed;
                            t.error = Some(e);
                        }
                    }
                }
            }
        }

        // Emit final status
        emit_task_update(&dm, &id, &app);
    }
}

fn emit_task_update(dm: &DownloadManagerState, id: &str, app: &AppHandle) {
    let event = {
        let manager = dm.lock().unwrap();
        manager
            .tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.to_progress_event())
    };
    if let Some(evt) = event {
        let _ = app.emit("download-progress", evt);
    }
}

async fn download_file(
    url: &str,
    path: &Path,
    thread_count: usize,
    downloaded: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    origin: &str,
) -> Result<(), String> {
    use reqwest::header::{HeaderMap, HeaderValue, REFERER, USER_AGENT};

    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        ),
    );
    if !origin.is_empty() {
        if let Ok(referer) = HeaderValue::from_str(&format!("{}/", origin)) {
            headers.insert(REFERER, referer);
        }
    }

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(60))
        .tcp_nodelay(true)
        .build()
        .map_err(|e| e.to_string())?;

    // Probe with a small range request to detect content-length and range support
    // Many CDNs support ranges but don't advertise it via HEAD accept-ranges header
    let mut content_length = 0u64;
    let mut supports_ranges = false;

    if thread_count > 1 {
        if let Ok(probe_resp) = client
            .get(url)
            .header("Range", "bytes=0-0")
            .send()
            .await
        {
            let status = probe_resp.status().as_u16();
            if status == 206 {
                // Server supports range requests
                supports_ranges = true;
                // Parse content-length from content-range header: "bytes 0-0/TOTAL"
                if let Some(cr) = probe_resp
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                {
                    if let Some(slash) = cr.rfind('/') {
                        if let Ok(size) = cr[slash + 1..].parse::<u64>() {
                            content_length = size;
                        }
                    }
                }
            } else if status == 200 {
                // Server ignored range, get content-length from normal response
                content_length = probe_resp
                    .headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
            }
        }
    }

    // Fallback: try HEAD if probe didn't give us content-length
    if content_length == 0 {
        if let Ok(head_resp) = client.head(url).send().await {
            content_length = head_resp
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
        }
    }

    if content_length > 0 {
        total.store(content_length, Ordering::Relaxed);
    }

    if content_length > 0 && supports_ranges && thread_count > 1 {
        download_multi(&client, url, path, content_length, thread_count, downloaded, cancel).await
    } else {
        download_single(&client, url, path, downloaded, total, cancel).await
    }
}

const SEGMENT_SIZE: u64 = 4 * 1024 * 1024; // 4MB segments for dynamic chunking
const MAX_RETRIES: u32 = 10;
const INITIAL_BACKOFF_MS: u64 = 1000;
const MAX_BACKOFF_MS: u64 = 30000;

fn is_transient_error(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

async fn send_with_retry(
    client: &reqwest::Client,
    url: &str,
    range_header: Option<String>,
    cancel: &AtomicBool,
) -> Result<reqwest::Response, String> {
    let mut last_err = String::new();
    for attempt in 0..MAX_RETRIES {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".to_string());
        }

        let mut req = client.get(url);
        if let Some(ref range) = range_header {
            req = req.header("Range", range.as_str());
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if is_transient_error(status) {
                    last_err = format!("HTTP {}", status);
                    let backoff = std::cmp::min(
                        INITIAL_BACKOFF_MS * 2u64.pow(attempt),
                        MAX_BACKOFF_MS,
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(backoff)).await;
                    continue;
                }
                return Ok(resp);
            }
            Err(e) => {
                if e.is_connect() || e.is_timeout() {
                    last_err = e.to_string();
                    let backoff = std::cmp::min(
                        INITIAL_BACKOFF_MS * 2u64.pow(attempt),
                        MAX_BACKOFF_MS,
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(backoff)).await;
                    continue;
                }
                return Err(e.to_string());
            }
        }
    }
    Err(format!("{} (after {} retries)", last_err, MAX_RETRIES))
}

async fn download_single(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    downloaded: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut response = send_with_retry(client, url, None, &cancel).await?;

    let status = response.status().as_u16();
    if status != 200 && status != 206 {
        return Err(format!("HTTP {}", status));
    }

    // Try to get content-length from the GET response if not already set
    if total.load(Ordering::Relaxed) == 0 {
        if let Some(cl) = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            total.store(cl, Ordering::Relaxed);
        }
    }

    let file = tokio::fs::File::create(path)
        .await
        .map_err(|e| e.to_string())?;
    let mut writer = BufWriter::with_capacity(256 * 1024, file);

    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if cancel.load(Ordering::Relaxed) {
            drop(writer);
            let _ = tokio::fs::remove_file(path).await;
            return Err("Cancelled".to_string());
        }
        writer.write_all(&chunk)
            .await
            .map_err(|e| e.to_string())?;
        downloaded.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }

    writer.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn download_multi(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    total_size: u64,
    thread_count: usize,
    downloaded: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    // Dynamic segmenting: many small segments distributed across workers.
    // This keeps all workers busy and avoids per-connection throttling by CDNs.
    let segment_count = ((total_size + SEGMENT_SIZE - 1) / SEGMENT_SIZE) as usize;
    let next_segment = Arc::new(AtomicUsize::new(0));

    // Pre-allocate output file to total size
    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .await
        .map_err(|e| e.to_string())?;
    file.set_len(total_size).await.map_err(|e| e.to_string())?;
    let file = Arc::new(tokio::sync::Mutex::new(file));

    let mut handles = Vec::new();
    for _ in 0..thread_count {
        let client = client.clone();
        let url = url.to_string();
        let downloaded = downloaded.clone();
        let cancel = cancel.clone();
        let next_segment = next_segment.clone();
        let file = file.clone();
        let total = total_size;
        let seg_count = segment_count;

        handles.push(tokio::spawn(async move {
            loop {
                let seg_idx = next_segment.fetch_add(1, Ordering::SeqCst);
                if seg_idx >= seg_count {
                    return Ok::<(), String>(());
                }
                let start = seg_idx as u64 * SEGMENT_SIZE;
                let end = std::cmp::min(start + SEGMENT_SIZE - 1, total - 1);

                // Retry individual segments on failure
                let mut segment_retries = 0u32;
                loop {
                    match download_segment(&client, &url, &file, start, end, &downloaded, &cancel).await {
                        Ok(()) => break,
                        Err(_) if cancel.load(Ordering::Relaxed) => {
                            return Err("Cancelled".to_string());
                        }
                        Err(e) => {
                            segment_retries += 1;
                            if segment_retries >= 5 {
                                return Err(e);
                            }
                            // The segment will overwrite the same file region on retry,
                            // so the file stays correct despite partial progress.
                            let backoff = std::cmp::min(
                                INITIAL_BACKOFF_MS * 2u64.pow(segment_retries - 1),
                                MAX_BACKOFF_MS,
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                        }
                    }
                }
            }
        }));
    }

    // Wait for all workers
    let mut errors = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => errors.push(e),
            Err(e) => errors.push(e.to_string()),
        }
    }

    if cancel.load(Ordering::Relaxed) {
        let _ = tokio::fs::remove_file(path).await;
        return Err("Cancelled".to_string());
    }

    if !errors.is_empty() {
        let _ = tokio::fs::remove_file(path).await;
        return Err(errors.join("; "));
    }

    // Ensure all data is flushed
    {
        let mut f = file.lock().await;
        f.flush().await.map_err(|e| e.to_string())?;
    }

    Ok(())
}

async fn download_segment(
    client: &reqwest::Client,
    url: &str,
    file: &tokio::sync::Mutex<tokio::fs::File>,
    start: u64,
    end: u64,
    downloaded: &AtomicU64,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let range = format!("bytes={}-{}", start, end);
    let mut response = send_with_retry(client, url, Some(range), cancel).await?;

    let status = response.status().as_u16();
    if status != 200 && status != 206 {
        return Err(format!("HTTP {}", status));
    }

    let mut buffer = Vec::with_capacity(512 * 1024);
    let mut offset = start;

    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".to_string());
        }
        buffer.extend_from_slice(&chunk);
        downloaded.fetch_add(chunk.len() as u64, Ordering::Relaxed);

        // Flush buffer to file when large enough to reduce lock contention
        if buffer.len() >= 512 * 1024 {
            let mut f = file.lock().await;
            f.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(|e| e.to_string())?;
            f.write_all(&buffer)
                .await
                .map_err(|e| e.to_string())?;
            offset += buffer.len() as u64;
            buffer.clear();
        }
    }

    // Flush remaining data
    if !buffer.is_empty() {
        let mut f = file.lock().await;
        f.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|e| e.to_string())?;
        f.write_all(&buffer)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}
