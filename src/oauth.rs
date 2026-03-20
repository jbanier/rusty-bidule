use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rand::{Rng, distr::Alphanumeric, rng};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};
use tracing::{debug, info, warn};
use url::Url;

use crate::config::{McpAuthConfig, McpOauthPublicConfig, McpServerConfig};

#[derive(Debug, Clone)]
pub struct OAuthProvider {
    client: Client,
    storage_root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokenStore {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicClientRegistration {
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AuthorizationServerMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthAuthorization {
    pub access_token: String,
}

impl OAuthProvider {
    pub fn new(data_dir: impl AsRef<Path>) -> Result<Self> {
        let storage_root = data_dir.as_ref().join("oauth");
        fs::create_dir_all(&storage_root)
            .with_context(|| format!("failed to create {}", storage_root.display()))?;
        info!(storage_root = %storage_root.display(), "initialized OAuth token storage");
        Ok(Self {
            client: Client::new(),
            storage_root,
        })
    }

    pub async fn authorize_server(
        &self,
        server: &McpServerConfig,
    ) -> Result<Option<OAuthAuthorization>> {
        let Some(McpAuthConfig::OauthPublic(auth)) = &server.auth else {
            return Ok(None);
        };
        debug!(server = %server.name, "authorizing MCP server through OAuth");

        if let Some(token) = self.load_token(&server.name)? {
            if !token_is_expired(&token) {
                debug!(server = %server.name, "reusing cached OAuth token");
                return Ok(Some(OAuthAuthorization {
                    access_token: token.access_token,
                }));
            }

            if let Some(refreshed) = self.try_refresh_token(server, auth, &token).await? {
                self.save_token(&server.name, &refreshed)?;
                info!(server = %server.name, "refreshed OAuth token");
                return Ok(Some(OAuthAuthorization {
                    access_token: refreshed.access_token,
                }));
            }
            warn!(server = %server.name, "cached OAuth token refresh failed");
        }

        let token = self.run_authorization_code_flow(server, auth).await?;
        self.save_token(&server.name, &token)?;
        info!(server = %server.name, "completed OAuth authorization flow");
        Ok(Some(OAuthAuthorization {
            access_token: token.access_token,
        }))
    }

    async fn run_authorization_code_flow(
        &self,
        server: &McpServerConfig,
        auth: &McpOauthPublicConfig,
    ) -> Result<OAuthTokenStore> {
        info!(server = %server.name, "starting OAuth authorization code flow");
        let metadata = self.fetch_metadata(server).await?;
        let registration = self
            .resolve_client_registration(server, auth, &metadata)
            .await?;
        let redirect_uri = resolve_redirect_uri(auth)?;
        let state = random_string(32);
        let code_verifier = random_string(64);
        let code_challenge = pkce_challenge(&code_verifier);

        let mut authorize_url =
            Url::parse(&metadata.authorization_endpoint).with_context(|| {
                format!(
                    "invalid authorization endpoint {}",
                    metadata.authorization_endpoint
                )
            })?;
        {
            let mut query = authorize_url.query_pairs_mut();
            query.append_pair("response_type", "code");
            query.append_pair("client_id", &registration.client_id);
            query.append_pair("redirect_uri", &redirect_uri);
            query.append_pair("code_challenge", &code_challenge);
            query.append_pair("code_challenge_method", "S256");
            query.append_pair("state", &state);
            if !auth.scopes.is_empty() {
                query.append_pair("scope", &auth.scopes.join(" "));
            }
            if let Some(resource) = &auth.resource {
                query.append_pair("resource", resource);
            }
        }

        if auth.open_browser {
            info!(server = %server.name, "opening browser for OAuth authorization");
            webbrowser::open(authorize_url.as_str())
                .with_context(|| format!("failed to open browser for {}", authorize_url))?;
        } else {
            eprintln!("Open this URL to authorize MCP access: {authorize_url}");
            info!(server = %server.name, "browser auto-open disabled; printed OAuth URL");
        }

        let callback = wait_for_callback(auth, &state).await?;
        self.exchange_code_for_token(
            &metadata,
            auth,
            &registration,
            &redirect_uri,
            &callback.code,
            &code_verifier,
        )
        .await
    }

    async fn try_refresh_token(
        &self,
        server: &McpServerConfig,
        auth: &McpOauthPublicConfig,
        token: &OAuthTokenStore,
    ) -> Result<Option<OAuthTokenStore>> {
        let Some(refresh_token) = &token.refresh_token else {
            return Ok(None);
        };
        let metadata = self.fetch_metadata(server).await?;
        let registration = self
            .resolve_client_registration(server, auth, &metadata)
            .await?;

        let mut form = vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", refresh_token.clone()),
            ("client_id", registration.client_id.clone()),
        ];
        if let Some(resource) = &auth.resource {
            form.push(("resource", resource.clone()));
        }
        if auth.token_endpoint_auth_method != "none" {
            if let Some(secret) = registration
                .client_secret
                .clone()
                .or_else(|| auth.client_secret.clone())
            {
                form.push(("client_secret", secret));
            }
        }

        let response = self
            .client
            .post(&metadata.token_endpoint)
            .form(&form)
            .send()
            .await
            .with_context(|| {
                format!(
                    "failed to refresh token against {}",
                    metadata.token_endpoint
                )
            })?;

        if !response.status().is_success() {
            warn!(server = %server.name, "refresh token request rejected");
            return Ok(None);
        }

        let payload: Value = response
            .json()
            .await
            .context("failed to parse refresh token response")?;
        parse_token_response(&payload, token.refresh_token.clone())
            .map(Some)
            .context("failed to decode refresh token payload")
    }

    async fn exchange_code_for_token(
        &self,
        metadata: &AuthorizationServerMetadata,
        auth: &McpOauthPublicConfig,
        registration: &DynamicClientRegistration,
        redirect_uri: &str,
        code: &str,
        code_verifier: &str,
    ) -> Result<OAuthTokenStore> {
        debug!(token_endpoint = %metadata.token_endpoint, "exchanging authorization code for access token");
        let mut form = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code.to_string()),
            ("redirect_uri", redirect_uri.to_string()),
            ("client_id", registration.client_id.clone()),
            ("code_verifier", code_verifier.to_string()),
        ];
        if let Some(resource) = &auth.resource {
            form.push(("resource", resource.clone()));
        }
        if auth.token_endpoint_auth_method != "none" {
            if let Some(secret) = registration
                .client_secret
                .clone()
                .or_else(|| auth.client_secret.clone())
            {
                form.push(("client_secret", secret));
            }
        }

        let response = self
            .client
            .post(&metadata.token_endpoint)
            .form(&form)
            .send()
            .await
            .with_context(|| {
                format!(
                    "failed to exchange auth code against {}",
                    metadata.token_endpoint
                )
            })?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            bail!("token endpoint returned HTTP {}: {}", status, text);
        }
        let payload: Value = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse token response body: {text}"))?;
        parse_token_response(&payload, None)
    }

    async fn fetch_metadata(
        &self,
        server: &McpServerConfig,
    ) -> Result<AuthorizationServerMetadata> {
        let server_url = Url::parse(&server.url)
            .with_context(|| format!("invalid MCP server URL {}", server.url))?;
        let metadata_url = server_url
            .join("/.well-known/oauth-authorization-server")
            .context("failed to construct OAuth metadata URL")?;
        let response = self
            .client
            .get(metadata_url.clone())
            .send()
            .await
            .with_context(|| format!("failed to fetch OAuth metadata from {metadata_url}"))?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            bail!("OAuth metadata endpoint returned HTTP {}: {}", status, text);
        }
        debug!(server = %server.name, metadata_url = %metadata_url, "fetched OAuth authorization metadata");
        serde_json::from_str(&text)
            .with_context(|| format!("failed to parse OAuth metadata body: {text}"))
    }

    async fn resolve_client_registration(
        &self,
        server: &McpServerConfig,
        auth: &McpOauthPublicConfig,
        metadata: &AuthorizationServerMetadata,
    ) -> Result<DynamicClientRegistration> {
        if let Some(client_id) = &auth.client_id {
            debug!(server = %server.name, "using statically configured OAuth client");
            return Ok(DynamicClientRegistration {
                client_id: client_id.clone(),
                client_secret: auth.client_secret.clone(),
            });
        }

        if let Some(existing) = self.load_registration(&server.name)? {
            debug!(server = %server.name, "reusing persisted OAuth client registration");
            return Ok(existing);
        }

        if !auth.use_dynamic_client_registration {
            bail!(
                "OAuth client_id is not configured and dynamic client registration is disabled for server '{}'",
                server.name
            );
        }

        let registration_endpoint = metadata.registration_endpoint.clone().ok_or_else(|| {
            anyhow!("authorization server did not provide a registration_endpoint")
        })?;
        let redirect_uri = resolve_redirect_uri(auth)?;
        let payload = json!({
            "client_name": format!("rusty-bidule-{}", server.name),
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": auth.token_endpoint_auth_method,
            "scope": auth.scopes.join(" "),
        });

        let response = self
            .client
            .post(&registration_endpoint)
            .json(&payload)
            .send()
            .await
            .with_context(|| {
                format!("failed to dynamically register client at {registration_endpoint}")
            })?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            bail!(
                "dynamic client registration returned HTTP {}: {}",
                status,
                text
            );
        }
        let body: Value = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse registration response body: {text}"))?;
        let registration = DynamicClientRegistration {
            client_id: body
                .get("client_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("registration response missing client_id"))?
                .to_string(),
            client_secret: body
                .get("client_secret")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        self.save_registration(&server.name, &registration)?;
        info!(server = %server.name, "completed dynamic OAuth client registration");
        Ok(registration)
    }

    fn token_path(&self, server_name: &str) -> PathBuf {
        self.storage_root
            .join(format!("{}_token.json", sanitize_name(server_name)))
    }

    fn registration_path(&self, server_name: &str) -> PathBuf {
        self.storage_root
            .join(format!("{}_client.json", sanitize_name(server_name)))
    }

    fn load_token(&self, server_name: &str) -> Result<Option<OAuthTokenStore>> {
        load_json::<OAuthTokenStore>(&self.token_path(server_name))
    }

    fn save_token(&self, server_name: &str, token: &OAuthTokenStore) -> Result<()> {
        save_json(&self.token_path(server_name), token)
    }

    fn load_registration(&self, server_name: &str) -> Result<Option<DynamicClientRegistration>> {
        load_json::<DynamicClientRegistration>(&self.registration_path(server_name))
    }

    fn save_registration(
        &self,
        server_name: &str,
        registration: &DynamicClientRegistration,
    ) -> Result<()> {
        save_json(&self.registration_path(server_name), registration)
    }
}

#[derive(Debug)]
struct CallbackResponse {
    code: String,
}

async fn wait_for_callback(
    auth: &McpOauthPublicConfig,
    expected_state: &str,
) -> Result<CallbackResponse> {
    let host = auth.redirect_host.as_deref().unwrap_or("127.0.0.1");
    let port = auth.redirect_port.unwrap_or(8766);
    let path = auth.redirect_path.as_deref().unwrap_or("/callback");
    let listener = TcpListener::bind((host, port))
        .await
        .with_context(|| format!("failed to bind OAuth callback listener on {host}:{port}"))?;
    info!(host, port, "waiting for OAuth callback");
    let timeout_window = Duration::from_secs(auth.callback_timeout_seconds);
    let (mut stream, _) = timeout(timeout_window, listener.accept())
        .await
        .context("timed out waiting for OAuth callback")?
        .context("failed to accept OAuth callback connection")?;

    let mut buffer = [0u8; 4096];
    let size = timeout(timeout_window, stream.read(&mut buffer))
        .await
        .context("timed out reading OAuth callback request")?
        .context("failed to read OAuth callback request")?;
    let request = String::from_utf8_lossy(&buffer[..size]);
    let request_line = request
        .lines()
        .next()
        .ok_or_else(|| anyhow!("OAuth callback request was empty"))?;
    let callback_target = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("OAuth callback request line was malformed"))?;
    let callback_url = Url::parse(&format!("http://{host}:{port}{callback_target}"))
        .context("failed to parse OAuth callback URL")?;

    let response = if callback_url.path() == path {
        let code = callback_url
            .query_pairs()
            .find_map(|(key, value)| (key == "code").then(|| value.into_owned()))
            .ok_or_else(|| anyhow!("OAuth callback did not include a code"))?;
        let state = callback_url
            .query_pairs()
            .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
            .ok_or_else(|| anyhow!("OAuth callback did not include a state"))?;
        if state != expected_state {
            bail!("OAuth callback state did not match the original authorization request");
        }
        info!("received valid OAuth callback");
        write_callback_response(&mut stream, true).await?;
        CallbackResponse { code }
    } else {
        write_callback_response(&mut stream, false).await?;
        bail!(
            "OAuth callback reached unexpected path {}",
            callback_url.path()
        );
    };

    Ok(response)
}

async fn write_callback_response(stream: &mut tokio::net::TcpStream, success: bool) -> Result<()> {
    let body = if success {
        "Authorization completed. You can return to rusty-bidule."
    } else {
        "Authorization failed. You can close this window and inspect the client logs."
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .await
        .context("failed to write OAuth callback response")
}

fn resolve_redirect_uri(auth: &McpOauthPublicConfig) -> Result<String> {
    if !auth.redirect_uri.trim().is_empty() {
        return Ok(auth.redirect_uri.clone());
    }
    let host = auth.redirect_host.as_deref().unwrap_or("localhost");
    let port = auth.redirect_port.unwrap_or(8766);
    let path = auth.redirect_path.as_deref().unwrap_or("/callback");
    Ok(format!("http://{host}:{port}{path}"))
}

fn parse_token_response(
    payload: &Value,
    fallback_refresh_token: Option<String>,
) -> Result<OAuthTokenStore> {
    let access_token = payload
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("token response missing access_token"))?
        .to_string();
    let refresh_token = payload
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or(fallback_refresh_token);
    let token_type = payload
        .get("token_type")
        .and_then(Value::as_str)
        .unwrap_or("Bearer")
        .to_string();
    let expires_at = payload
        .get("expires_in")
        .and_then(Value::as_i64)
        .map(|seconds| Utc::now() + ChronoDuration::seconds(seconds.max(0)));
    let scope = payload
        .get("scope")
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok(OAuthTokenStore {
        access_token,
        refresh_token,
        token_type,
        expires_at,
        scope,
    })
}

fn token_is_expired(token: &OAuthTokenStore) -> bool {
    token
        .expires_at
        .map(|expires_at| expires_at <= Utc::now() + ChronoDuration::seconds(60))
        .unwrap_or(false)
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn random_string(len: usize) -> String {
    rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch,
            _ => '_',
        })
        .collect()
}

fn load_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(value))
}

fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let payload = serde_json::to_string_pretty(value)?;
    fs::write(path, payload).with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        OAuthProvider, parse_token_response, pkce_challenge, sanitize_name, token_is_expired,
    };

    #[test]
    fn parses_token_response_and_expiry() {
        let token = parse_token_response(
            &json!({
                "access_token": "abc",
                "refresh_token": "refresh",
                "expires_in": 3600,
                "token_type": "Bearer"
            }),
            None,
        )
        .unwrap();

        assert_eq!(token.access_token, "abc");
        assert_eq!(token.refresh_token.as_deref(), Some("refresh"));
        assert!(!token_is_expired(&token));
    }

    #[test]
    fn pkce_challenge_is_url_safe() {
        let challenge = pkce_challenge("verifier");
        assert!(!challenge.contains('='));
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
    }

    #[test]
    fn oauth_storage_paths_are_sanitized() {
        let dir = tempdir().unwrap();
        let provider = OAuthProvider::new(dir.path()).unwrap();
        assert!(
            provider
                .token_path("wiz/prod")
                .to_string_lossy()
                .contains("wiz_prod_token.json")
        );
        assert_eq!(sanitize_name("wiz/prod"), "wiz_prod");
    }
}
