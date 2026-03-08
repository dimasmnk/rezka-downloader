use thiserror::Error;

#[derive(Error, Debug)]
pub enum HdRezkaError {
    #[error("Login is required to access this page.")]
    LoginRequired,

    #[error("Login failed: {0}")]
    LoginFailed(String),

    #[error("Failed to fetch stream!")]
    FetchFailed,

    #[error("Failed to bypass captcha!")]
    CaptchaError,

    #[error("HTTP {code}: {message}")]
    Http { code: u16, message: String },

    #[error("Request error: {0}")]
    Request(#[from] reqwest::Error),

    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("{0}")]
    ValueError(String),

    #[error("{0}")]
    TypeError(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
