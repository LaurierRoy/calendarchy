use crate::config::GoogleConfig;
use crate::error::{CalendarchyError, Result};
use crate::google::types::{TokenInfo, TokenResponse};
use crate::logging::{log_request, log_response};
use chrono::Utc;
use reqwest::Client;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CALENDAR_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events https://www.googleapis.com/auth/calendar.readonly";
const REDIRECT_URI: &str = "http://127.0.0.1:18457";

pub struct GoogleAuth {
    client: Client,
    config: GoogleConfig,
}

impl GoogleAuth {
    pub fn new(config: GoogleConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    /// Get the authorization URL that the user should open in their browser
    pub fn auth_url(&self) -> String {
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent",
            AUTH_URL,
            urlencoding::encode(&self.config.client_id),
            urlencoding::encode(REDIRECT_URI),
            urlencoding::encode(CALENDAR_SCOPE),
        )
    }

    /// Start a localhost server, wait for the OAuth callback, and exchange the code for tokens.
    /// Returns the tokens on success.
    pub async fn authenticate_with_browser(&self) -> Result<TokenInfo> {
        let listener = TcpListener::bind("127.0.0.1:18457").await
            .map_err(|e| CalendarchyError::Auth(format!("Failed to start auth server: {}", e)))?;

        // Wait for the callback
        let (mut stream, _) = listener.accept().await
            .map_err(|e| CalendarchyError::Auth(format!("Failed to accept connection: {}", e)))?;

        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await
            .map_err(|e| CalendarchyError::Auth(format!("Failed to read request: {}", e)))?;

        let request = String::from_utf8_lossy(&buf[..n]);

        // Extract the authorization code from the request
        let code = extract_code(&request)
            .ok_or_else(|| CalendarchyError::Auth("No authorization code in callback".to_string()))?;

        // Send a response to the browser
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
            <html><body style=\"font-family:system-ui;display:flex;justify-content:center;align-items:center;height:100vh;margin:0\">\
            <h2>Authenticated! You can close this tab.</h2></body></html>";
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;

        // Exchange the code for tokens
        self.exchange_code(&code).await
    }

    /// Exchange an authorization code for tokens
    async fn exchange_code(&self, code: &str) -> Result<TokenInfo> {
        log_request("POST", TOKEN_URL);
        let response = self
            .client
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("code", code),
                ("grant_type", "authorization_code"),
                ("redirect_uri", REDIRECT_URI),
            ])
            .send()
            .await?;
        log_response(response.status().as_u16(), TOKEN_URL);

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(CalendarchyError::Auth(format!(
                "Failed to exchange code: {}",
                body
            )));
        }

        let token_response: TokenResponse = response.json().await?;
        Ok(TokenInfo {
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            expires_at: Utc::now() + chrono::Duration::seconds(token_response.expires_in as i64),
            token_type: token_response.token_type,
        })
    }

    /// Refresh an expired token
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<TokenInfo> {
        log_request("POST", &format!("{} (refresh)", TOKEN_URL));
        let response = self
            .client
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await?;
        log_response(response.status().as_u16(), TOKEN_URL);

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(CalendarchyError::Auth(format!(
                "Failed to refresh token: {}",
                body
            )));
        }

        let token_response: TokenResponse = response.json().await?;
        Ok(TokenInfo {
            access_token: token_response.access_token,
            refresh_token: Some(refresh_token.to_string()), // Keep original
            expires_at: Utc::now() + chrono::Duration::seconds(token_response.expires_in as i64),
            token_type: token_response.token_type,
        })
    }
}

/// Extract the authorization code from an HTTP request line
fn extract_code(request: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;
    for param in query.split('&') {
        if let Some(value) = param.strip_prefix("code=") {
            return Some(urlencoding::decode(value).ok()?.to_string());
        }
    }
    None
}
