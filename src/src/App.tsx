import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

// --- Types ---

interface UserInfo {
  username: string;
  avatar: string | null;
}

interface AppConfig {
  origin: string;
  session_path: string | null;
  download_dir: string | null;
  thread_count: number;
}

interface FastSearchResult {
  title: string;
  url: string;
  rating: number | null;
}

interface AdvancedSearchResult {
  title: string;
  url: string;
  image: string;
  category: string | null;
}

interface TranslatorItem {
  id: number;
  name: string;
  premium: boolean;
}

interface EpisodeTranslation {
  translator_id: number;
  translator_name: string;
  premium: boolean;
}

interface EpisodeInfo {
  episode: number;
  episode_text: string;
  translations: EpisodeTranslation[];
}

interface SeasonEpisodesInfo {
  season: number;
  season_text: string;
  episodes: EpisodeInfo[];
}

interface MovieInfo {
  title: string;
  orig_title: string | null;
  image: string | null;
  year: number | null;
  description: string | null;
  content_type: string;
  translators: TranslatorItem[];
  seasons: SeasonEpisodesInfo[] | null;
  rating: number | null;
}

interface QualityOption {
  quality: string;
  urls: string[];
}

interface DownloadTask {
  id: string;
  title: string;
  quality: string;
  status: "queued" | "downloading" | "completed" | "failed" | "cancelled";
  downloaded_bytes: number;
  total_bytes: number;
  error: string | null;
  file_path: string;
  speed: number;
}

interface DownloadProgressEvent {
  id: string;
  status: "queued" | "downloading" | "completed" | "failed" | "cancelled";
  downloaded_bytes: number;
  total_bytes: number;
  error: string | null;
}

// --- Icons ---

function SettingsIcon() {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function UserIcon() {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
      <circle cx="12" cy="7" r="4" />
    </svg>
  );
}

function BackIcon() {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="19" y1="12" x2="5" y2="12" />
      <polyline points="12 19 5 12 12 5" />
    </svg>
  );
}

function SearchIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="11" cy="11" r="8" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="7 10 12 15 17 10" />
      <line x1="12" y1="15" x2="12" y2="3" />
    </svg>
  );
}

function QueueIcon() {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="7 10 12 15 17 10" />
      <line x1="12" y1="15" x2="12" y2="3" />
    </svg>
  );
}

// --- Hooks ---

function useDebounce<T>(value: T, delay: number): T {
  const [debouncedValue, setDebouncedValue] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedValue(value), delay);
    return () => clearTimeout(timer);
  }, [value, delay]);
  return debouncedValue;
}

// --- App ---

type View = "loading" | "main" | "login" | "settings" | "search-results" | "movie" | "downloads";

function App() {
  const [view, setView] = useState<View>("loading");
  const [prevView, setPrevView] = useState<View>("main");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [userInfo, setUserInfo] = useState<UserInfo | null>(null);
  const [config, setConfig] = useState<AppConfig>({ origin: "https://hdrezka.ag", session_path: null, download_dir: null, thread_count: 4 });
  const [configOrigin, setConfigOrigin] = useState("");
  const [configSessionPath, setConfigSessionPath] = useState("");
  const [configDownloadDir, setConfigDownloadDir] = useState("");
  const [configThreadCount, setConfigThreadCount] = useState("4");

  // Search state
  const [searchQuery, setSearchQuery] = useState("");
  const [fastResults, setFastResults] = useState<FastSearchResult[]>([]);
  const [showDropdown, setShowDropdown] = useState(false);
  const [searchResults, setSearchResults] = useState<AdvancedSearchResult[]>([]);
  const [searchLoading, setSearchLoading] = useState(false);

  // Movie state
  const [movieUrl, setMovieUrl] = useState("");
  const [movieInfo, setMovieInfo] = useState<MovieInfo | null>(null);
  const [movieLoading, setMovieLoading] = useState(false);
  const [movieError, setMovieError] = useState("");
  const [selectedSeason, setSelectedSeason] = useState<number | null>(null);
  const [selectedTranslator, setSelectedTranslator] = useState<number | null>(null);
  const [selectedEpisode, setSelectedEpisode] = useState<number | null>(null);
  const [qualities, setQualities] = useState<QualityOption[]>([]);
  const [qualitiesLoading, setQualitiesLoading] = useState(false);
  const [qualitiesError, setQualitiesError] = useState("");

  // Download state
  const [downloads, setDownloads] = useState<DownloadTask[]>([]);
  const [seasonDownloading, setSeasonDownloading] = useState(false);
  const [seasonQueueProgress, setSeasonQueueProgress] = useState<{ queued: number; total: number } | null>(null);
  const [seasonQualities, setSeasonQualities] = useState<QualityOption[]>([]);
  const [seasonQualitiesLoading, setSeasonQualitiesLoading] = useState(false);
  const speedTrackRef = useRef<Record<string, { bytes: number; time: number }>>({}); 

  const searchInputRef = useRef<HTMLInputElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);

  const debouncedQuery = useDebounce(searchQuery, 300);

  useEffect(() => {
    restoreSession();
  }, []);

  // Listen for download progress events
  useEffect(() => {
    const unlisten = listen<DownloadProgressEvent>("download-progress", (event) => {
      const { id, downloaded_bytes, status } = event.payload;
      const now = Date.now();
      let speed = 0;
      const prev = speedTrackRef.current[id];
      if (prev && status === "downloading") {
        const dt = (now - prev.time) / 1000;
        if (dt > 0) {
          speed = (downloaded_bytes - prev.bytes) / dt;
        }
      }
      speedTrackRef.current[id] = { bytes: downloaded_bytes, time: now };
      if (status !== "downloading") {
        delete speedTrackRef.current[id];
      }
      setDownloads((prev) =>
        prev.map((d) =>
          d.id === event.payload.id
            ? { ...d, status: event.payload.status, downloaded_bytes: event.payload.downloaded_bytes, total_bytes: event.payload.total_bytes, error: event.payload.error, speed }
            : d
        )
      );
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // Listen for season queue progress events
  useEffect(() => {
    const unlisten = listen<{ queued: number; total: number; episode: number }>("season-queue-progress", (event) => {
      setSeasonQueueProgress({ queued: event.payload.queued, total: event.payload.total });
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // Auto-load qualities for movies when translator changes
  useEffect(() => {
    if (!movieInfo || movieLoading) return;
    if (movieInfo.content_type !== "movie") return;
    if (selectedTranslator == null) return;
    loadQualities(null, null);
  }, [selectedTranslator]);

  // Auto-load qualities for TV series when episode or translator changes
  useEffect(() => {
    if (!movieInfo || movieLoading) return;
    if (movieInfo.content_type !== "tv_series") return;
    if (selectedTranslator == null || selectedSeason == null || selectedEpisode == null) return;
    loadQualities(selectedSeason, selectedEpisode);
  }, [selectedTranslator, selectedSeason, selectedEpisode]);

  // Auto-load season qualities when season or translator changes (for season download)
  useEffect(() => {
    if (!movieInfo || movieLoading) return;
    if (movieInfo.content_type !== "tv_series") return;
    if (selectedTranslator == null || selectedSeason == null) return;
    if (!movieInfo.seasons) return;
    const season = movieInfo.seasons.find((s) => s.season === selectedSeason);
    if (!season || season.episodes.length === 0) return;
    // Pick the first episode to probe available qualities
    const firstEp = [...season.episodes].sort((a, b) => a.episode - b.episode)[0];
    if (!firstEp.translations.some((t) => t.translator_id === selectedTranslator)) return;
    loadSeasonQualities(selectedSeason, firstEp.episode);
  }, [selectedTranslator, selectedSeason]);

  // Fast search on debounced query
  useEffect(() => {
    if (debouncedQuery.length < 2) {
      setFastResults([]);
      setShowDropdown(false);
      return;
    }
    // Don't fast-search if it looks like a URL
    if (isUrl(debouncedQuery)) return;

    let cancelled = false;
    (async () => {
      try {
        const results: FastSearchResult[] = await invoke("fast_search", { query: debouncedQuery });
        if (!cancelled) {
          setFastResults(results);
          setShowDropdown(results.length > 0);
        }
      } catch (err) {
        if (!cancelled) {
          setFastResults([]);
          console.error("fast_search error:", err);
        }
      }
    })();
    return () => { cancelled = true; };
  }, [debouncedQuery]);

  // Close dropdown on outside click
  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(e.target as Node) &&
        searchInputRef.current &&
        !searchInputRef.current.contains(e.target as Node)
      ) {
        setShowDropdown(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, []);

  function isUrl(text: string): boolean {
    return /^https?:\/\//.test(text.trim());
  }

  async function restoreSession() {
    try {
      const cfg: AppConfig = await invoke("get_config");
      setConfig(cfg);
      setConfigOrigin(cfg.origin);
      setConfigSessionPath(cfg.session_path || "");
      setConfigDownloadDir(cfg.download_dir || "");
      setConfigThreadCount(String(cfg.thread_count || 4));

      const info: UserInfo | null = await invoke("restore_session");
      if (info) {
        setUserInfo(info);
      }
    } catch {
      // no saved session
    } finally {
      setView("main");
    }
  }

  async function handleLogin(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      const info: UserInfo = await invoke("login", { email, password });
      setUserInfo(info);
      setPassword("");
      setView("main");
    } catch (err: any) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  async function handleLogout() {
    try {
      await invoke("logout");
      setUserInfo(null);
      setView("main");
    } catch (err: any) {
      setError(String(err));
    }
  }

  async function handleSaveConfig(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    try {
      const newConfig: AppConfig = {
        origin: configOrigin,
        session_path: configSessionPath || null,
        download_dir: configDownloadDir || null,
        thread_count: parseInt(configThreadCount) || 4,
      };
      await invoke("set_config", { config: newConfig });
      setConfig(newConfig);
      setView("main");
    } catch (err: any) {
      setError(String(err));
    }
  }

  async function handleSearchSubmit(e: React.FormEvent) {
    e.preventDefault();
    const query = searchQuery.trim();
    if (!query) return;

    setShowDropdown(false);

    if (isUrl(query)) {
      openMovie(query);
      return;
    }

    setSearchLoading(true);
    setError("");
    setPrevView(view);
    setView("search-results");
    try {
      const results: AdvancedSearchResult[] = await invoke("search", { query });
      setSearchResults(results);
    } catch (err: any) {
      setSearchResults([]);
      setError(String(err));
    } finally {
      setSearchLoading(false);
    }
  }

  const openMovie = useCallback(async (url: string) => {
    setMovieUrl(url);
    setMovieInfo(null);
    setMovieError("");
    setMovieLoading(true);
    setSelectedSeason(null);
    setSelectedTranslator(null);
    setSelectedEpisode(null);
    setQualities([]);
    setPrevView(view);
    setView("movie");

    try {
      const info: MovieInfo = await invoke("get_movie_info", { url });
      setMovieInfo(info);
      if (info.translators.length > 0) {
        setSelectedTranslator(info.translators[0].id);
      }
      if (info.seasons && info.seasons.length > 0) {
        const sorted = [...info.seasons].sort((a, b) => a.season - b.season);
        setSelectedSeason(sorted[0].season);
      }
    } catch (err: any) {
      setMovieError(String(err));
    } finally {
      setMovieLoading(false);
    }
  }, [view]);

  function handleDropdownClick(result: FastSearchResult) {
    setShowDropdown(false);
    setSearchQuery("");
    openMovie(result.url);
  }

  function handleSearchResultClick(result: AdvancedSearchResult) {
    openMovie(result.url);
  }

  function navigateBack() {
    if (view === "movie" && prevView === "search-results") {
      setView("search-results");
    } else {
      setView("main");
    }
  }

  async function loadQualities(
    season: number | null,
    episode: number | null
  ) {
    setQualitiesLoading(true);
    setQualities([]);
    setQualitiesError("");
    try {
      const res: QualityOption[] = await invoke("get_stream_info", {
        url: movieUrl,
        translatorId: selectedTranslator,
        season,
        episode,
      });
      setQualities(res);
    } catch (err) {
      console.error("loadQualities error:", err);
      setQualities([]);
      setQualitiesError(String(err));
    } finally {
      setQualitiesLoading(false);
    }
  }

  async function loadSeasonQualities(
    season: number,
    episode: number
  ) {
    setSeasonQualitiesLoading(true);
    setSeasonQualities([]);
    try {
      const res: QualityOption[] = await invoke("get_stream_info", {
        url: movieUrl,
        translatorId: selectedTranslator,
        season,
        episode,
      });
      setSeasonQualities(res);
    } catch {
      setSeasonQualities([]);
    } finally {
      setSeasonQualitiesLoading(false);
    }
  }

  async function handleStartDownload(videoUrl: string, quality: string) {
    const title = movieInfo
      ? selectedEpisode != null && selectedSeason != null
        ? `${movieInfo.title} S${selectedSeason}E${selectedEpisode}`
        : movieInfo.title
      : "Unknown";

    try {
      const taskId: string = await invoke("start_download", {
        videoUrl,
        title,
        quality,
      });
      setDownloads((prev) => [
        ...prev,
        {
          id: taskId,
          title,
          quality,
          status: "queued" as const,
          downloaded_bytes: 0,
          total_bytes: 0,
          error: null,
          file_path: "",
          speed: 0,
        },
      ]);
    } catch (err: any) {
      console.error("start_download failed:", err);
    }
  }

  async function handleDownloadSeason(quality: string) {
    if (!movieInfo || selectedSeason == null || selectedTranslator == null) return;
    setSeasonDownloading(true);
    setSeasonQueueProgress(null);
    try {
      const results: { episode: number; task_id: string | null; quality: string | null; error: string | null }[] =
        await invoke("start_season_download", {
          url: movieUrl,
          translatorId: selectedTranslator,
          season: selectedSeason,
          quality,
        });
      const newTasks: DownloadTask[] = results
        .filter((r) => r.task_id)
        .map((r) => ({
          id: r.task_id!,
          title: `${movieInfo.title} S${selectedSeason}E${r.episode}`,
          quality: r.quality || quality,
          status: "queued" as const,
          downloaded_bytes: 0,
          total_bytes: 0,
          error: null,
          file_path: "",
          speed: 0,
        }));
      setDownloads((prev) => [...prev, ...newTasks]);
    } catch (err: any) {
      console.error("start_season_download failed:", err);
    } finally {
      setSeasonDownloading(false);
      setSeasonQueueProgress(null);
    }
  }

  async function handleCancelDownload(id: string) {
    try {
      await invoke("cancel_download", { id });
    } catch (err: any) {
      console.error("cancel_download failed:", err);
    }
  }

  async function handleRemoveDownload(id: string) {
    try {
      await invoke("remove_download", { id });
      setDownloads((prev) => prev.filter((d) => d.id !== id));
    } catch (err: any) {
      console.error("remove_download failed:", err);
    }
  }

  async function refreshDownloads() {
    try {
      const tasks: DownloadTask[] = await invoke("get_downloads");
      setDownloads(tasks);
    } catch {
      // ignore
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  }

  function formatSpeed(bytesPerSec: number): string {
    if (bytesPerSec <= 0) return "0 MB/s";
    const mbps = bytesPerSec / (1024 * 1024);
    return mbps >= 0.1 ? mbps.toFixed(1) + " MB/s" : (bytesPerSec / 1024).toFixed(0) + " KB/s";
  }

  function activeDownloadCount(): number {
    return downloads.filter((d) => d.status === "queued" || d.status === "downloading").length;
  }

  // --- Render ---

  if (view === "loading") {
    return (
      <main className="container">
        <div className="loading">Loading...</div>
      </main>
    );
  }

  if (view === "settings") {
    return (
      <main className="container">
        <div className="top-bar">
          <button className="icon-btn" onClick={() => setView("main")} title="Back">
            <BackIcon />
          </button>
          <h1 className="view-title">Settings</h1>
          <div className="icon-btn-placeholder" />
        </div>
        <div className="view-content">
          <form className="form" onSubmit={handleSaveConfig}>
            <div className="form-group">
              <label htmlFor="origin">HDRezka Origin URL</label>
              <input
                id="origin"
                type="url"
                value={configOrigin}
                onChange={(e) => setConfigOrigin(e.currentTarget.value)}
                placeholder="https://hdrezka.ag"
              />
            </div>
            <div className="form-group">
              <label htmlFor="session-path">Session File Path (optional)</label>
              <input
                id="session-path"
                type="text"
                value={configSessionPath}
                onChange={(e) => setConfigSessionPath(e.currentTarget.value)}
                placeholder="Leave empty for default location"
              />
            </div>
            <div className="form-group">
              <label htmlFor="download-dir">Download Directory (optional)</label>
              <input
                id="download-dir"
                type="text"
                value={configDownloadDir}
                onChange={(e) => setConfigDownloadDir(e.currentTarget.value)}
                placeholder="Leave empty for system Downloads folder"
              />
            </div>
            <div className="form-group">
              <label htmlFor="thread-count">Download Threads</label>
              <input
                id="thread-count"
                type="number"
                min="1"
                max="64"
                value={configThreadCount}
                onChange={(e) => setConfigThreadCount(e.currentTarget.value)}
              />
            </div>
            {error && <p className="error">{error}</p>}
            <div className="button-row">
              <button type="submit">Save</button>
              <button
                type="button"
                className="secondary"
                onClick={() => {
                  setError("");
                  setView("main");
                }}
              >
                Cancel
              </button>
            </div>
          </form>
        </div>
      </main>
    );
  }

  if (view === "login") {
    return (
      <main className="container">
        <div className="top-bar">
          <button className="icon-btn" onClick={() => setView("main")} title="Back">
            <BackIcon />
          </button>
          <h1 className="view-title">
            {userInfo ? "Account" : "Login"}
          </h1>
          <div className="icon-btn-placeholder" />
        </div>
        <div className="view-content">
          {userInfo ? (
            <>
              <div className="user-card">
                {userInfo.avatar && (
                  <img className="user-avatar" src={userInfo.avatar} alt="Avatar" />
                )}
                <h2>{userInfo.username}</h2>
                <p className="user-origin">{config.origin}</p>
              </div>
              <div className="button-row">
                <button className="danger" onClick={handleLogout}>
                  Logout
                </button>
              </div>
            </>
          ) : (
            <form className="form" onSubmit={handleLogin}>
              <div className="form-group">
                <label htmlFor="email">Email</label>
                <input
                  id="email"
                  type="email"
                  value={email}
                  onChange={(e) => setEmail(e.currentTarget.value)}
                  placeholder="your@email.com"
                  required
                />
              </div>
              <div className="form-group">
                <label htmlFor="password">Password</label>
                <input
                  id="password"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.currentTarget.value)}
                  placeholder="Password"
                  required
                />
              </div>
              {error && <p className="error">{error}</p>}
              <button type="submit" disabled={loading}>
                {loading ? "Logging in..." : "Login"}
              </button>
            </form>
          )}
        </div>
      </main>
    );
  }

  if (view === "movie") {
    return (
      <main className="container">
        <div className="top-bar">
          <button className="icon-btn" onClick={navigateBack} title="Back">
            <BackIcon />
          </button>
          <h1 className="view-title">
            {movieInfo ? movieInfo.title : "Loading..."}
          </h1>
          <div className="top-bar-actions">
            <button
              className="icon-btn downloads-btn"
              onClick={() => { refreshDownloads(); setView("downloads"); }}
              title="Downloads"
            >
              <QueueIcon />
              {activeDownloadCount() > 0 && (
                <span className="badge">{activeDownloadCount()}</span>
              )}
            </button>
          </div>
        </div>
        <div className="view-content movie-view">
          {movieLoading && <div className="loading">Loading movie info...</div>}
          {movieError && <p className="error">{movieError}</p>}
          {movieInfo && (
            <div className="movie-detail">
              <div className="movie-header">
                {movieInfo.image && (
                  <img className="movie-poster" src={movieInfo.image} alt={movieInfo.title} />
                )}
                <div className="movie-meta">
                  <h2 className="movie-title">{movieInfo.title}</h2>
                  {movieInfo.orig_title && (
                    <p className="movie-orig-title">{movieInfo.orig_title}</p>
                  )}
                  <div className="movie-tags">
                    {movieInfo.year && <span className="tag">{movieInfo.year}</span>}
                    <span className="tag">
                      {movieInfo.content_type === "tv_series" ? "TV Series" : "Movie"}
                    </span>
                    {movieInfo.rating != null && (
                      <span className="tag tag-rating">★ {movieInfo.rating.toFixed(1)}</span>
                    )}
                  </div>
                  {movieInfo.description && (
                    <p className="movie-description">{movieInfo.description}</p>
                  )}
                </div>
              </div>

              {/* Translators */}
              {movieInfo.translators.length > 0 && (
                <div className="movie-section">
                  <h3>Translation</h3>
                  <div className="translator-list">
                    {movieInfo.translators.map((tr) => (
                      <button
                        key={tr.id}
                        className={`translator-btn ${selectedTranslator === tr.id ? "active" : ""}`}
                        onClick={() => setSelectedTranslator(tr.id)}
                      >
                        {tr.name}
                        {tr.premium && <span className="premium-badge">P</span>}
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {/* Movie: auto-loaded qualities with download buttons */}
              {movieInfo.content_type === "movie" && (
                <div className="movie-section">
                  <h3>Quality</h3>
                  {qualitiesLoading && <div className="loading-small">Loading qualities...</div>}
                  {!qualitiesLoading && qualities.length > 0 && (
                    <div className="quality-list">
                      {qualities.map((q) => (
                        <div key={q.quality} className="quality-item">
                          <span className="tag tag-quality">{q.quality}</span>
                          <button
                            className="small-btn download-btn"
                            onClick={() => handleStartDownload(q.urls[0], q.quality)}
                            title="Download"
                          >
                            <DownloadIcon />
                          </button>
                        </div>
                      ))}
                    </div>
                  )}
                  {!qualitiesLoading && qualities.length === 0 && selectedTranslator != null && !movieLoading && (
                    <div className="loading-small">No qualities available</div>
                  )}
                </div>
              )}

              {/* TV Series: seasons & episodes */}
              {movieInfo.content_type === "tv_series" && movieInfo.seasons && (
                <div className="movie-section">
                  <h3>Seasons</h3>
                  <div className="season-tabs">
                    {[...movieInfo.seasons]
                      .sort((a, b) => a.season - b.season)
                      .map((s) => (
                        <button
                          key={s.season}
                          className={`season-btn ${selectedSeason === s.season ? "active" : ""}`}
                          onClick={() => {
                            setSelectedSeason(s.season);
                            setSelectedEpisode(null);
                            setQualities([]);
                            setQualitiesError("");
                          }}
                        >
                          {s.season_text.trim() || `Season ${s.season}`}
                        </button>
                      ))}
                  </div>

                  {/* Download full season */}
                  {selectedSeason != null && selectedTranslator != null && (
                    <div className="season-download">
                      {seasonQualitiesLoading && (
                        <div className="loading-small">Loading qualities...</div>
                      )}
                      {!seasonQualitiesLoading && seasonQualities.length > 0 && (
                        <div className="season-download-row">
                          <span className="season-download-label">Download full season:</span>
                          <div className="quality-list">
                            {seasonQualities.map((q) => (
                              <div key={q.quality} className="quality-item">
                                <span className="tag tag-quality">{q.quality}</span>
                                <button
                                  className="small-btn download-btn"
                                  onClick={() => handleDownloadSeason(q.quality)}
                                  disabled={seasonDownloading}
                                  title={`Download all episodes in ${q.quality}`}
                                >
                                  <DownloadIcon />
                                </button>
                              </div>
                            ))}
                          </div>
                          {seasonDownloading && (
                            <span className="loading-small">
                              {seasonQueueProgress
                                ? `Queueing episodes... ${seasonQueueProgress.queued}/${seasonQueueProgress.total}`
                                : "Queueing episodes..."}
                            </span>
                          )}
                        </div>
                      )}
                    </div>
                  )}

                  {selectedSeason != null && (() => {
                    const season = movieInfo.seasons!.find((s) => s.season === selectedSeason);
                    if (!season) return null;
                    const episodes = [...season.episodes].sort((a, b) => a.episode - b.episode);
                    return (
                      <div className="episodes-list">
                        {episodes.map((ep) => (
                          <div
                            key={ep.episode}
                            className={`episode-item ${selectedEpisode === ep.episode ? "active" : ""}`}
                            onClick={() => setSelectedEpisode(ep.episode)}
                            style={{ cursor: "pointer" }}
                          >
                            <span className="episode-label">
                              {ep.episode_text.trim() || `Episode ${ep.episode}`}
                            </span>
                          </div>
                        ))}
                      </div>
                    );
                  })()}

                  {/* Qualities for selected episode */}
                  {selectedEpisode != null && (
                    <div className="quality-section">
                      {qualitiesLoading && <div className="loading-small">Loading qualities...</div>}
                      {!qualitiesLoading && qualities.length > 0 && (
                        <div className="quality-list">
                          {qualities.map((q) => (
                            <div key={q.quality} className="quality-item">
                              <span className="tag tag-quality">{q.quality}</span>
                              <button
                                className="small-btn download-btn"
                                onClick={() => handleStartDownload(q.urls[0], q.quality)}
                                title="Download"
                              >
                                <DownloadIcon />
                              </button>
                            </div>
                          ))}
                        </div>
                      )}
                      {!qualitiesLoading && qualities.length === 0 && qualitiesError && (
                        <div className="loading-small" style={{ color: "#c0392b" }}>{qualitiesError}</div>
                      )}
                      {!qualitiesLoading && qualities.length === 0 && !qualitiesError && (
                        <div className="loading-small">No qualities available</div>
                      )}
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      </main>
    );
  }

  if (view === "downloads") {
    return (
      <main className="container">
        <div className="top-bar">
          <button className="icon-btn" onClick={() => setView("main")} title="Back">
            <BackIcon />
          </button>
          <h1 className="view-title">Downloads</h1>
          <div className="icon-btn-placeholder" />
        </div>
        <div className="view-content downloads-view">
          {downloads.length === 0 && (
            <p className="no-results">No downloads yet</p>
          )}
          {downloads.length > 0 && (
            <div className="downloads-list">
              {[...downloads].reverse().map((dl) => (
                <div key={dl.id} className={`download-item download-${dl.status}`}>
                  <div className="download-info">
                    <span className="download-title">{dl.title}</span>
                    <span className="download-quality tag tag-quality">{dl.quality}</span>
                  </div>
                  <div className="download-progress-row">
                    {(dl.status === "downloading" || dl.status === "queued") && (
                      <div className="progress-bar">
                        <div
                          className="progress-fill"
                          style={{ width: dl.total_bytes > 0 ? `${(dl.downloaded_bytes / dl.total_bytes) * 100}%` : "0%" }}
                        />
                      </div>
                    )}
                    <span className="download-status-text">
                      {dl.status === "downloading" && dl.total_bytes > 0
                        ? `${formatBytes(dl.downloaded_bytes)} / ${formatBytes(dl.total_bytes)} (${Math.round((dl.downloaded_bytes / dl.total_bytes) * 100)}%) — ${formatSpeed(dl.speed)}`
                        : dl.status === "downloading" && dl.downloaded_bytes > 0
                          ? `${formatBytes(dl.downloaded_bytes)} downloaded — ${formatSpeed(dl.speed)}`
                          : dl.status === "downloading"
                            ? "Starting download..."
                            : dl.status === "queued"
                              ? "Queued"
                              : dl.status === "completed"
                                ? `Completed${dl.total_bytes > 0 ? " — " + formatBytes(dl.total_bytes) : ""}`
                                : dl.status === "failed"
                                  ? `Failed${dl.error ? ": " + dl.error : ""}`
                                  : "Cancelled"}
                    </span>
                  </div>
                  <div className="download-actions">
                    {(dl.status === "downloading" || dl.status === "queued") && (
                      <button
                        className="small-btn danger"
                        onClick={() => handleCancelDownload(dl.id)}
                      >
                        Cancel
                      </button>
                    )}
                    {(dl.status === "completed" || dl.status === "failed" || dl.status === "cancelled") && (
                      <button
                        className="small-btn"
                        onClick={() => handleRemoveDownload(dl.id)}
                      >
                        Remove
                      </button>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </main>
    );
  }

  if (view === "search-results") {
    return (
      <main className="container">
        <div className="top-bar">
          <button className="icon-btn" onClick={() => setView("main")} title="Back">
            <BackIcon />
          </button>
          <h1 className="view-title">Search Results</h1>
          <div className="icon-btn-placeholder" />
        </div>
        <div className="view-content search-results-view">
          {searchLoading && <div className="loading">Searching...</div>}
          {error && <p className="error">{error}</p>}
          {!searchLoading && !error && searchResults.length === 0 && (
            <p className="no-results">No results found</p>
          )}
          {!searchLoading && searchResults.length > 0 && (
            <div className="search-results-grid">
              {searchResults.map((result, i) => (
                <div
                  key={i}
                  className="search-result-card"
                  onClick={() => handleSearchResultClick(result)}
                >
                  {result.image && (
                    <img className="search-result-image" src={result.image} alt={result.title} />
                  )}
                  <div className="search-result-info">
                    <p className="search-result-title">{result.title}</p>
                    {result.category && (
                      <span className="tag tag-small">{formatCategory(result.category)}</span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </main>
    );
  }

  // Main view
  return (
    <main className="container">
      <div className="top-bar">
        <div className="top-bar-spacer" />
        <div className="top-bar-actions">
          <button
            className="icon-btn downloads-btn"
            onClick={() => { refreshDownloads(); setView("downloads"); }}
            title="Downloads"
          >
            <QueueIcon />
            {activeDownloadCount() > 0 && (
              <span className="badge">{activeDownloadCount()}</span>
            )}
          </button>
          <button
            className="icon-btn"
            onClick={() => setView("login")}
            title={userInfo ? userInfo.username : "Login"}
          >
            {userInfo?.avatar ? (
              <img className="icon-avatar" src={userInfo.avatar} alt="Avatar" />
            ) : (
              <UserIcon />
            )}
          </button>
          <button className="icon-btn" onClick={() => setView("settings")} title="Settings">
            <SettingsIcon />
          </button>
        </div>
      </div>
      <div className="view-content main-search-view">
        <h1 className="app-title">HDRezka</h1>
        <form className="search-form" onSubmit={handleSearchSubmit}>
          <div className="search-input-wrapper">
            <input
              ref={searchInputRef}
              className="search-input"
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.currentTarget.value)}
              onFocus={() => {
                if (fastResults.length > 0) setShowDropdown(true);
              }}
              placeholder="Search movies or paste a link..."
              autoFocus
            />
            <button type="submit" className="search-btn" title="Search">
              <SearchIcon />
            </button>
          </div>
          {showDropdown && fastResults.length > 0 && (
            <div className="search-dropdown" ref={dropdownRef}>
              {fastResults.map((result, i) => (
                <div
                  key={i}
                  className="dropdown-item"
                  onClick={() => handleDropdownClick(result)}
                >
                  <span className="dropdown-title">{result.title}</span>
                  {result.rating != null && (
                    <span className="dropdown-rating">★ {result.rating.toFixed(1)}</span>
                  )}
                </div>
              ))}
            </div>
          )}
        </form>
      </div>
    </main>
  );
}

function formatCategory(cat: any): string {
  if (typeof cat === "string") return cat;
  if (typeof cat === "object") {
    const key = Object.keys(cat)[0];
    if (key === "Other") return cat[key];
    return key;
  }
  return "";
}

export default App;
