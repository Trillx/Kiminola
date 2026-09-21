//! OpenAI-compatible LLM provider seam (SPEC.md §3 / §6).
//!
//! Provider config (kind, base URL, model) is stored in SQLite `settings`.
//! The API key is stored in the OS keychain via `keyring`.
//! Each completion streams through its own Tauri IPC channel.

use std::time::Duration;

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use reqwest::header::{self, HeaderMap};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{ipc::Channel, State};

use crate::db::{ensure_pool, update_enhanced_notes_impl, DbState};

const CONFIG_KEY: &str = "llm_config";
const KEYRING_SERVICE: &str = "kiminola";
const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models?limit=1000";

/// Supported OpenAI-compatible providers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAi,
    OpenRouter,
    Ollama,
    LmStudio,
}

impl ProviderKind {
    fn default_base_url(&self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "https://api.openai.com/v1",
            ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
            ProviderKind::Ollama => "http://localhost:11434/v1",
            ProviderKind::LmStudio => "http://localhost:1234/v1",
        }
    }

    fn default_model(&self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "gpt-4o-mini",
            ProviderKind::OpenRouter => "openai/gpt-4o-mini",
            ProviderKind::Ollama => "llama3.1",
            ProviderKind::LmStudio => "default",
        }
    }
}

impl Default for ProviderKind {
    fn default() -> Self {
        ProviderKind::OpenAi
    }
}

/// The non-secret part of the provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        let kind = ProviderKind::default();
        Self {
            kind,
            base_url: kind.default_base_url().to_string(),
            model: kind.default_model().to_string(),
        }
    }
}

/// Provider settings returned to the UI. Stored credentials are never returned;
/// only key presence for this provider/endpoint identity is exposed.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderConfigView {
    #[serde(flatten)]
    pub config: ProviderConfig,
    pub has_api_key: bool,
}

impl ProviderConfigView {
    fn new(config: ProviderConfig, has_api_key: bool) -> Self {
        Self {
            config,
            has_api_key,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenRouterModel {
    pub id: String,
    pub name: String,
    pub context_length: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenRouterModelLinks {
    next: Option<String>,
}

#[derive(Deserialize)]
struct OpenRouterModelsResponse {
    data: Vec<OpenRouterModel>,
    #[serde(default)]
    links: OpenRouterModelLinks,
}

fn parse_openrouter_models_page(body: &str) -> Result<OpenRouterModelsResponse, String> {
    serde_json::from_str(body).map_err(|e| format!("invalid OpenRouter model list: {e}"))
}

fn sort_openrouter_models(models: &mut [OpenRouterModel]) {
    models.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn openrouter_models_page_url(value: &str) -> Result<reqwest::Url, String> {
    let base = reqwest::Url::parse("https://openrouter.ai")
        .map_err(|e| format!("invalid OpenRouter origin: {e}"))?;
    let url = base
        .join(value)
        .map_err(|e| format!("invalid OpenRouter model page: {e}"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("openrouter.ai")
        || url.path() != "/api/v1/models"
    {
        return Err("OpenRouter returned an invalid model page link".to_string());
    }
    Ok(url)
}

async fn fetch_openrouter_models() -> Result<Vec<OpenRouterModel>, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;
    let mut next = Some(OPENROUTER_MODELS_URL.to_string());
    let mut visited = std::collections::HashSet::new();
    let mut models = Vec::new();

    while let Some(page) = next.take() {
        let url = openrouter_models_page_url(&page)?;
        if visited.len() >= 100 || !visited.insert(url.to_string()) {
            return Err("OpenRouter returned an invalid pagination sequence".to_string());
        }
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("failed to load OpenRouter models: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "OpenRouter model list returned {}",
                response.status()
            ));
        }
        let body = response
            .text()
            .await
            .map_err(|e| format!("failed to read OpenRouter model list: {e}"))?;
        let catalog = parse_openrouter_models_page(&body)?;
        models.extend(catalog.data);
        next = catalog.links.next;
    }

    sort_openrouter_models(&mut models);
    Ok(models)
}

/// A single chat message for the completion endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Events yielded by a streaming completion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum LlmEvent {
    Chunk(String),
    Done,
    Error(String),
}

/// Abstraction over any streaming chat provider.
#[async_trait]
pub trait ChatProvider: Send + Sync {
    /// Return a stream of completion events. The stream is decoupled from Tauri
    /// event emission; callers bridge events to the UI as needed.
    async fn complete(&self, messages: &[Message]) -> Result<BoxStream<'static, LlmEvent>, String>;
}

/// OpenAI-compatible chat provider (covers OpenAI, OpenRouter, Ollama, LM Studio).
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl OpenAiCompatibleProvider {
    fn new(config: &ProviderConfig, api_key: String) -> Result<Self, String> {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        if !api_key.is_empty() {
            let mut authorization = header::HeaderValue::from_str(&format!("Bearer {api_key}"))
                .map_err(|_| "invalid api key".to_string())?;
            authorization.set_sensitive(true);
            headers.insert(header::AUTHORIZATION, authorization);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            // Even same-origin redirects may escape the credential's endpoint path.
            .redirect(reqwest::redirect::Policy::none())
            // Without these, a stalled provider stream hangs the enhancement
            // forever. read_timeout bounds idle time between chunks, not the
            // total generation length, so slow-but-alive models still finish.
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("failed to build http client: {e}"))?;

        Ok(Self {
            client,
            base_url: normalized_base_url(config)?,
            model: config.model.clone(),
        })
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: String,
    messages: &'a [Message],
    stream: bool,
}

#[derive(Deserialize)]
struct ChatCompletionChunk {
    choices: Vec<Choice>,
}

/// Providers such as OpenRouter can deliver failures as an SSE event inside an
/// otherwise successful (200) stream: `data: {"error": {"message": ...}}`.
#[derive(Deserialize)]
struct StreamError {
    error: StreamErrorBody,
}

#[derive(Deserialize)]
struct StreamErrorBody {
    message: String,
}

#[derive(Deserialize)]
struct Choice {
    delta: Delta,
}

#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
}

#[async_trait]
impl ChatProvider for OpenAiCompatibleProvider {
    async fn complete(&self, messages: &[Message]) -> Result<BoxStream<'static, LlmEvent>, String> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = ChatRequest {
            model: self.model.clone(),
            messages,
            stream: true,
        };

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".into());
            return Err(format!("provider returned {status}: {text}"));
        }

        let byte_stream = response.bytes_stream();
        let pending = String::new();

        let stream = stream::unfold(
            (byte_stream, pending),
            |(mut byte_stream, mut pending)| async move {
                loop {
                    // Process complete SSE lines already in the buffer.
                    while let Some(pos) = pending.find('\n') {
                        let line = pending[..pos].trim().to_string();
                        pending = pending[pos + 1..].to_string();
                        if line.is_empty() || !line.starts_with("data: ") {
                            continue;
                        }
                        let payload = &line["data: ".len()..];
                        if payload == "[DONE]" {
                            return Some((LlmEvent::Done, (byte_stream, pending)));
                        }
                        match serde_json::from_str::<ChatCompletionChunk>(payload) {
                            Ok(chunk) => {
                                for choice in chunk.choices {
                                    if let Some(text) = choice.delta.content {
                                        return Some((
                                            LlmEvent::Chunk(text),
                                            (byte_stream, pending),
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                if let Ok(err) = serde_json::from_str::<StreamError>(payload) {
                                    return Some((
                                        LlmEvent::Error(format!(
                                            "provider error: {}",
                                            err.error.message
                                        )),
                                        (byte_stream, pending),
                                    ));
                                }
                                eprintln!("[llm] failed to parse chunk: {e}");
                            }
                        }
                    }

                    // Need more bytes.
                    match byte_stream.next().await {
                        Some(Ok(chunk)) => {
                            pending.push_str(&String::from_utf8_lossy(&chunk));
                        }
                        Some(Err(e)) => {
                            return Some((
                                LlmEvent::Error(format!("stream error: {e}")),
                                (byte_stream, pending),
                            ));
                        }
                        None => return None,
                    }
                }
            },
        );

        Ok(Box::pin(stream))
    }
}

// Scope to the complete effective base URL, not just its origin: paths may be
// separate tenants. Use the same normalization when constructing HTTP requests.
fn normalized_base_url(config: &ProviderConfig) -> Result<String, String> {
    let url = reqwest::Url::parse(config.base_url.trim())
        .map_err(|_| "Base URL must be an absolute HTTP or HTTPS URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Base URL must be an absolute HTTP or HTTPS URL".into());
    }
    if !url.username().is_empty() || url.password().is_some()
        || url.query().is_some() || url.fragment().is_some()
    {
        return Err("Base URL cannot contain credentials, a query, or a fragment".into());
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn credential_account(config: &ProviderConfig) -> Result<String, String> {
    let identity = serde_json::to_vec(&(config.kind, normalized_base_url(config)?))
        .map_err(|_| "invalid provider identity".to_string())?;
    // A versioned, bounded account name avoids delimiter collisions and keyring
    // account-length limits without storing endpoint text in account metadata.
    Ok(format!("provider_api_key_v2_{}", hex::encode(Sha256::digest(identity))))
}

// A narrow seam keeps native credential IO out of synthetic regression tests.
trait ApiKeyStore: Send + Sync {
    fn read(&self, account: &str) -> Result<Option<String>, String>;
    // None deletes only the named credential; omission is handled by save_api_key.
    fn write(&self, account: &str, key: Option<&str>) -> Result<(), String>;
}

struct OsApiKeyStore;

fn keyring_entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, account)
        .map_err(|_| "Could not access Windows Credential Manager".to_string())
}

impl ApiKeyStore for OsApiKeyStore {
    fn read(&self, account: &str) -> Result<Option<String>, String> {
        match keyring_entry(account)?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("Could not read the provider API key from Windows Credential Manager".into()),
        }
    }

    fn write(&self, account: &str, key: Option<&str>) -> Result<(), String> {
        let entry = keyring_entry(account)?;
        match key {
            None => entry.delete_credential().or_else(|e| match e {
                keyring::Error::NoEntry => Ok(()),
                _ => Err("Could not delete the provider API key from Windows Credential Manager".to_string()),
            }),
            Some(key) => entry.set_password(key)
                .map_err(|_| "Could not save the provider API key to Windows Credential Manager".to_string()),
        }
    }
}

fn load_api_key(config: &ProviderConfig, store: &impl ApiKeyStore) -> Result<Option<String>, String> {
    // Deliberately never read/migrate the old unscoped account. Its provenance
    // cannot be inferred from mutable saved config after a prior provider switch.
    // Leave it untouched; the user must re-enter a key for this exact identity.
    Ok(store.read(&credential_account(config)?)?.filter(|key| !key.trim().is_empty()))
}

fn save_api_key(
    config: &ProviderConfig,
    api_key: Option<String>,
    store: &impl ApiKeyStore,
) -> Result<(), String> {
    let account = credential_account(config)?;
    match api_key {
        None => Ok(()),
        Some(key) if key.trim().is_empty() => store.write(&account, None),
        Some(key) => store.write(&account, Some(&key)),
    }
}

async fn load_config(pool: &sqlx::SqlitePool) -> Result<ProviderConfig, String> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(CONFIG_KEY)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;

    match value {
        Some(json) => serde_json::from_str(&json).map_err(|e| format!("bad config: {e}")),
        None => Ok(ProviderConfig::default()),
    }
}

async fn save_config(pool: &sqlx::SqlitePool, config: &ProviderConfig) -> Result<(), String> {
    let json = serde_json::to_string(config).map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(CONFIG_KEY)
    .bind(json)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn build_provider(config: &ProviderConfig) -> Result<OpenAiCompatibleProvider, String> {
    build_provider_with_store(config, &OsApiKeyStore)
}

fn build_provider_with_store(
    config: &ProviderConfig,
    store: &impl ApiKeyStore,
) -> Result<OpenAiCompatibleProvider, String> {
    let key = load_api_key(config, store)?;
    if key.is_none() && matches!(config.kind, ProviderKind::OpenAi | ProviderKind::OpenRouter) {
        return Err("Enter an API key for this provider and Base URL in AI provider settings".into());
    }
    OpenAiCompatibleProvider::new(config, key.unwrap_or_default())
}

/// Builds the message list sent to the LLM from transcript, notes, and a
/// template prompt containing `{transcript}` and `{notes}` placeholders.
pub struct PromptBuilder;

impl PromptBuilder {
    pub fn build(transcript: &str, notes: &str, template_prompt: &str) -> Vec<Message> {
        let prompt = template_prompt
            .replace("{transcript}", transcript)
            .replace("{notes}", notes);
        vec![
            Message {
                role: "system".into(),
                content: "You are a concise meeting-note assistant. Respond in Markdown.".into(),
            },
            Message {
                role: "user".into(),
                content: prompt,
            },
        ]
    }
}

#[tauri::command]
pub async fn get_llm_config(state: State<'_, DbState>) -> Result<ProviderConfigView, String> {
    let pool = ensure_pool(&state.pool).await?;
    get_llm_config_impl(&pool, &OsApiKeyStore).await
}

async fn get_llm_config_impl(
    pool: &sqlx::SqlitePool,
    store: &impl ApiKeyStore,
) -> Result<ProviderConfigView, String> {
    let config = load_config(pool).await?;
    // Allow repair of old invalid endpoints in the form, but never look up keys
    // for them. Real credential-store failures still propagate to the retry UI.
    let has_api_key = if normalized_base_url(&config).is_ok() {
        load_api_key(&config, store)?.is_some()
    } else {
        false
    };
    Ok(ProviderConfigView::new(config, has_api_key))
}

#[tauri::command]
pub async fn list_openrouter_models() -> Result<Vec<OpenRouterModel>, String> {
    fetch_openrouter_models().await
}

#[tauri::command]
pub async fn set_llm_config(
    state: State<'_, DbState>,
    config: ProviderConfig,
    api_key: Option<String>,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    set_llm_config_impl(&pool, &config, api_key, &OsApiKeyStore).await
}

async fn set_llm_config_impl(
    pool: &sqlx::SqlitePool,
    config: &ProviderConfig,
    api_key: Option<String>,
    store: &impl ApiKeyStore,
) -> Result<(), String> {
    // A keyring failure must not activate the new config. If SQLite subsequently
    // fails, the key remains bound to its own identity, never another endpoint.
    save_api_key(config, api_key, store)?;
    save_config(pool, config).await
}

#[tauri::command]
pub async fn test_llm_config(
    state: State<'_, DbState>,
    on_event: Channel<LlmEvent>,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    let config = load_config(&pool).await?;
    let provider = build_provider(&config)?;

    let messages = vec![Message {
        role: "user".into(),
        content: "Say hello.".into(),
    }];

    let mut stream = provider.complete(&messages).await?;
    let mut full = String::new();
    while let Some(event) = stream.next().await {
        match event {
            LlmEvent::Chunk(chunk) => {
                full.push_str(&chunk);
                on_event
                    .send(LlmEvent::Chunk(chunk))
                    .map_err(|e| e.to_string())?;
            }
            LlmEvent::Done => break,
            LlmEvent::Error(e) => return Err(e),
        }
    }

    if full.trim().is_empty() {
        return Err("provider returned no content".to_string());
    }
    on_event.send(LlmEvent::Done).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn enhance_meeting(
    on_event: Channel<LlmEvent>,
    state: State<'_, DbState>,
    meeting_id: i64,
    template_id: Option<i64>,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    let config = load_config(&pool).await?;
    let provider = build_provider(&config)?;

    let meeting = crate::db::get_meeting_impl(&pool, meeting_id).await?;
    let templates = crate::db::list_templates_impl(&pool).await?;
    let template = if let Some(id) = template_id {
        templates
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| "template not found".to_string())?
    } else {
        templates
            .into_iter()
            .next()
            .ok_or_else(|| "no templates available".to_string())?
    };

    let transcript = meeting
        .transcript
        .iter()
        .map(|s| format!("{}: {}", s.channel, s.text))
        .collect::<Vec<_>>()
        .join("\n");
    let notes = meeting.notepad;

    let messages = PromptBuilder::build(&transcript, &notes, &template.prompt);
    let stream = provider.complete(&messages).await?;

    tauri::async_runtime::spawn(async move {
        stream_enhancement(&pool, meeting_id, stream, &on_event).await;
    });
    Ok(())
}

async fn stream_enhancement(
    pool: &sqlx::SqlitePool,
    meeting_id: i64,
    mut stream: BoxStream<'static, LlmEvent>,
    on_event: &Channel<LlmEvent>,
) {
    let mut full = String::new();
    while let Some(event) = stream.next().await {
        match event {
            LlmEvent::Chunk(chunk) => {
                full.push_str(&chunk);
                // Persistence should finish even if the originating page was closed.
                let _ = on_event.send(LlmEvent::Chunk(chunk));
            }
            LlmEvent::Done => break,
            LlmEvent::Error(message) => {
                let _ = on_event.send(LlmEvent::Error(message));
                return;
            }
        }
    }
    // Some compatible providers end the stream without an explicit DONE marker.
    let result = if full.trim().is_empty() {
        Err("provider returned no content".to_string())
    } else {
        update_enhanced_notes_impl(pool, meeting_id, &full).await
    };
    let event = match result {
        Ok(()) => LlmEvent::Done,
        Err(error) => LlmEvent::Error(error),
    };
    let _ = on_event.send(event);
}

#[cfg(test)]
mod credential_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // Never instantiate the OS adapter or contact a provider in these tests.
    #[derive(Default)]
    struct MemoryKeys {
        values: Mutex<HashMap<String, String>>,
        reads: Mutex<Vec<String>>,
        fail_read: bool,
        fail_write: bool,
    }

    impl ApiKeyStore for MemoryKeys {
        fn read(&self, account: &str) -> Result<Option<String>, String> {
            self.reads.lock().unwrap().push(account.to_owned());
            if self.fail_read { return Err("synthetic credential read failure".into()); }
            Ok(self.values.lock().unwrap().get(account).cloned())
        }

        fn write(&self, account: &str, key: Option<&str>) -> Result<(), String> {
            if self.fail_write { return Err("synthetic credential write failure".into()); }
            let mut values = self.values.lock().unwrap();
            match key {
                Some(key) => { values.insert(account.to_owned(), key.to_owned()); }
                None => { values.remove(account); }
            }
            Ok(())
        }
    }

    fn config(kind: ProviderKind, base_url: &str) -> ProviderConfig {
        ProviderConfig { kind, base_url: base_url.into(), model: "synthetic-model".into() }
    }

    #[test]
    fn credentials_belong_to_provider_and_full_endpoint_not_model() {
        let store = MemoryKeys::default();
        let original = ProviderConfig::default();
        save_api_key(&original, Some("synthetic-cloud-key".into()), &store).unwrap();
        assert_eq!(load_api_key(&original, &store).unwrap().as_deref(), Some("synthetic-cloud-key"));
        for other in [
            config(ProviderKind::OpenRouter, &original.base_url),
            config(ProviderKind::OpenAi, "https://other.example.invalid/v1"),
            config(ProviderKind::OpenAi, "https://api.openai.com/other-tenant/v1"),
            config(ProviderKind::OpenAi, "http://api.openai.com/v1"),
            config(ProviderKind::OpenAi, "https://api.openai.com:8443/v1"),
            config(ProviderKind::Ollama, "http://localhost:11434/v1"),
            config(ProviderKind::LmStudio, "http://localhost:1234/v1"),
        ] {
            assert!(load_api_key(&other, &store).unwrap().is_none());
        }
        let changed_model = ProviderConfig { model: "another-model".into(), ..original.clone() };
        assert_eq!(load_api_key(&changed_model, &store).unwrap(), load_api_key(&original, &store).unwrap());
        let equivalent = config(ProviderKind::OpenAi, "https://API.OPENAI.COM:443/v1/");
        assert_eq!(credential_account(&equivalent).unwrap(), credential_account(&original).unwrap());
        let provider = build_provider_with_store(&equivalent, &store).unwrap();
        assert_eq!(provider.base_url, original.base_url);
    }

    #[test]
    fn legacy_unscoped_key_is_never_read_or_migrated() {
        let store = MemoryKeys::default();
        store.write("provider_api_key", Some("synthetic-legacy-key")).unwrap();
        for kind in [ProviderKind::OpenAi, ProviderKind::OpenRouter, ProviderKind::Ollama, ProviderKind::LmStudio] {
            let destination = config(kind, kind.default_base_url());
            assert!(load_api_key(&destination, &store).unwrap().is_none());
        }
        assert!(store.reads.lock().unwrap().iter().all(|account| account != "provider_api_key"));
        assert_eq!(store.values.lock().unwrap().len(), 1);
    }

    #[test]
    fn omission_and_deletion_affect_only_the_destination_credential() {
        let store = MemoryKeys::default();
        let a = ProviderConfig::default();
        let b = config(ProviderKind::OpenRouter, ProviderKind::OpenRouter.default_base_url());
        save_api_key(&a, Some("synthetic-a".into()), &store).unwrap();
        save_api_key(&b, None, &store).unwrap();
        assert!(load_api_key(&b, &store).unwrap().is_none());
        save_api_key(&b, Some("synthetic-b".into()), &store).unwrap();
        save_api_key(&b, None, &store).unwrap();
        assert_eq!(load_api_key(&b, &store).unwrap().as_deref(), Some("synthetic-b"));
        save_api_key(&b, Some("  ".into()), &store).unwrap();
        assert!(load_api_key(&b, &store).unwrap().is_none());
        assert_eq!(load_api_key(&a, &store).unwrap().as_deref(), Some("synthetic-a"));
    }

    #[test]
    fn cloud_requires_its_own_key_while_local_can_be_unauthenticated() {
        let store = MemoryKeys::default();
        save_api_key(&ProviderConfig::default(), Some("synthetic-cloud-key".into()), &store).unwrap();
        let cloud = config(ProviderKind::OpenRouter, ProviderKind::OpenRouter.default_base_url());
        assert!(build_provider_with_store(&cloud, &store).is_err());
        let local = config(ProviderKind::Ollama, ProviderKind::Ollama.default_base_url());
        assert!(build_provider_with_store(&local, &store).is_ok());
        save_api_key(&local, Some("synthetic-local-key".into()), &store).unwrap();
        assert_eq!(load_api_key(&local, &store).unwrap().as_deref(), Some("synthetic-local-key"));
        assert!(build_provider_with_store(&local, &store).is_ok());
        let failed_store = MemoryKeys { fail_read: true, ..Default::default() };
        assert!(build_provider_with_store(&local, &failed_store).is_err());
    }

    #[test]
    fn ambiguous_or_credential_bearing_urls_are_rejected_before_key_lookup() {
        let store = MemoryKeys::default();
        for url in ["", "relative/v1", "ftp://example.invalid/v1", "https://user:password@example.invalid/v1", "https://example.invalid/v1?key=secret", "https://example.invalid/v1#fragment"] {
            let invalid = config(ProviderKind::OpenAi, url);
            assert!(load_api_key(&invalid, &store).is_err());
            assert!(save_api_key(&invalid, Some("synthetic-key".into()), &store).is_err());
        }
        assert!(store.reads.lock().unwrap().is_empty());
        assert!(store.values.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn key_write_failure_keeps_saved_config_unchanged() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1)
            .connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .execute(&pool).await.unwrap();
        let original = ProviderConfig::default();
        save_config(&pool, &original).await.unwrap();
        let next = config(ProviderKind::OpenRouter, ProviderKind::OpenRouter.default_base_url());
        let failed_store = MemoryKeys { fail_write: true, ..Default::default() };
        assert!(set_llm_config_impl(&pool, &next, Some("synthetic-new-key".into()), &failed_store).await.is_err());
        assert_eq!(load_config(&pool).await.unwrap().kind, original.kind);
        let store = MemoryKeys::default();
        store.write("provider_api_key", Some("synthetic-legacy-key")).unwrap();
        set_llm_config_impl(&pool, &next, None, &store).await.unwrap();
        let view = get_llm_config_impl(&pool, &store).await.unwrap();
        assert_eq!(view.config.kind, next.kind);
        assert!(!view.has_api_key);
        set_llm_config_impl(&pool, &next, Some("synthetic-scoped-key".into()), &store).await.unwrap();
        let view = get_llm_config_impl(&pool, &store).await.unwrap();
        assert!(view.has_api_key);
        assert!(serde_json::to_value(view).unwrap().get("api_key").is_none());
        // Pre-upgrade configurations may have URLs that new saves reject. Keep
        // them editable without attempting any credential lookup for that URL.
        let invalid_legacy = config(ProviderKind::OpenAi, "https://example.invalid/v1?old=setting");
        save_config(&pool, &invalid_legacy).await.unwrap();
        let reads_before = store.reads.lock().unwrap().len();
        let view = get_llm_config_impl(&pool, &store).await.unwrap();
        assert!(!view.has_api_key);
        assert_eq!(view.config.base_url, invalid_legacy.base_url);
        assert_eq!(store.reads.lock().unwrap().len(), reads_before);
        pool.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    fn capture_channel() -> (
        Channel<LlmEvent>,
        std::sync::Arc<std::sync::Mutex<Vec<LlmEvent>>>,
    ) {
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = events.clone();
        let channel = Channel::new(move |body| {
            let tauri::ipc::InvokeResponseBody::Json(json) = body else {
                panic!("expected JSON");
            };
            received
                .lock()
                .unwrap()
                .push(serde_json::from_str(&json).unwrap());
            Ok(())
        });
        (channel, events)
    }

    async fn enhancement_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::migrations::run(&pool).await.unwrap();
        for id in [101, 202] {
            sqlx::query("INSERT INTO meetings (id, title, space_id, created_at) SELECT ?, 'Meeting', id, '2026-09-04' FROM spaces WHERE name = 'Personal' LIMIT 1")
                .bind(id).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO notes (meeting_id, raw_markdown, enhanced_markdown, updated_at) VALUES (?, 'Original notes', 'Previous enhancement', '2026-09-04')")
                .bind(id).execute(&pool).await.unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn concurrent_enhancements_keep_channels_and_persistence_separate() {
        let pool = enhancement_pool().await;
        let (channel_a, events_a) = capture_channel();
        let (channel_b, events_b) = capture_channel();
        let a = stream::iter(vec![LlmEvent::Chunk("A only".into()), LlmEvent::Done]).boxed();
        let b = stream::iter(vec![LlmEvent::Chunk("B only".into()), LlmEvent::Done]).boxed();
        tokio::join!(
            stream_enhancement(&pool, 101, a, &channel_a),
            stream_enhancement(&pool, 202, b, &channel_b),
        );
        assert_eq!(
            *events_a.lock().unwrap(),
            vec![LlmEvent::Chunk("A only".into()), LlmEvent::Done]
        );
        assert_eq!(
            *events_b.lock().unwrap(),
            vec![LlmEvent::Chunk("B only".into()), LlmEvent::Done]
        );
        for (id, expected) in [(101, "A only"), (202, "B only")] {
            let meeting = crate::db::get_meeting_impl(&pool, id).await.unwrap();
            assert_eq!(meeting.enhanced_markdown.as_deref(), Some(expected));
            assert_eq!(meeting.notepad, "Original notes");
        }
        pool.close().await;
    }

    #[tokio::test]
    async fn enhancement_error_does_not_complete_or_overwrite_the_other_meeting() {
        let pool = enhancement_pool().await;
        let (channel_a, events_a) = capture_channel();
        let (channel_b, events_b) = capture_channel();
        let a = stream::iter(vec![
            LlmEvent::Chunk("partial".into()),
            LlmEvent::Error("offline".into()),
        ])
        .boxed();
        let b = stream::iter(vec![LlmEvent::Chunk("B complete".into())]).boxed();
        tokio::join!(
            stream_enhancement(&pool, 101, a, &channel_a),
            stream_enhancement(&pool, 202, b, &channel_b)
        );
        assert_eq!(
            *events_a.lock().unwrap(),
            vec![
                LlmEvent::Chunk("partial".into()),
                LlmEvent::Error("offline".into())
            ]
        );
        assert_eq!(
            *events_b.lock().unwrap(),
            vec![LlmEvent::Chunk("B complete".into()), LlmEvent::Done]
        );
        assert_eq!(
            crate::db::get_meeting_impl(&pool, 101)
                .await
                .unwrap()
                .enhanced_markdown
                .as_deref(),
            Some("Previous enhancement")
        );
        assert_eq!(
            crate::db::get_meeting_impl(&pool, 202)
                .await
                .unwrap()
                .enhanced_markdown
                .as_deref(),
            Some("B complete")
        );
        pool.close().await;
    }

    struct FakeProvider {
        events: Vec<LlmEvent>,
    }

    #[async_trait]
    impl ChatProvider for FakeProvider {
        async fn complete(
            &self,
            _messages: &[Message],
        ) -> Result<BoxStream<'static, LlmEvent>, String> {
            let events = self.events.clone();
            Ok(Box::pin(stream::iter(events)))
        }
    }

    #[test]
    fn provider_config_view_exposes_only_key_presence() {
        let view = ProviderConfigView::new(ProviderConfig::default(), true);
        let json = serde_json::to_value(view).unwrap();
        assert_eq!(json["has_api_key"], true);
        assert_eq!(json["kind"], "open_ai");
        assert!(json.get("api_key").is_none());
    }

    #[test]
    fn openrouter_catalog_page_exposes_next_link_and_models_sort_by_name() {
        let page = parse_openrouter_models_page(
            r#"{
                "data": [
                    {"id": "openai/gpt-4o", "name": "OpenAI: GPT-4o", "context_length": 128000},
                    {"id": "anthropic/claude-3.5-sonnet", "name": "Anthropic: Claude 3.5 Sonnet", "context_length": 200000}
                ],
                "links": {"next": "/api/v1/models?offset=2&limit=2"}
            }"#,
        )
        .unwrap();
        assert_eq!(
            page.links.next.as_deref(),
            Some("/api/v1/models?offset=2&limit=2")
        );

        let mut models = page.data;
        sort_openrouter_models(&mut models);
        assert_eq!(
            models,
            vec![
                OpenRouterModel {
                    id: "anthropic/claude-3.5-sonnet".into(),
                    name: "Anthropic: Claude 3.5 Sonnet".into(),
                    context_length: Some(200000),
                },
                OpenRouterModel {
                    id: "openai/gpt-4o".into(),
                    name: "OpenAI: GPT-4o".into(),
                    context_length: Some(128000),
                },
            ]
        );
    }

    #[test]
    fn openrouter_pagination_stays_on_the_catalog_endpoint() {
        let next = openrouter_models_page_url("/api/v1/models?offset=1000&limit=1000").unwrap();
        assert_eq!(next.host_str(), Some("openrouter.ai"));
        assert_eq!(next.path(), "/api/v1/models");
        assert!(openrouter_models_page_url("https://example.com/api/v1/models").is_err());
        assert!(openrouter_models_page_url("/api/v1/models/other").is_err());
    }

    #[test]
    fn prompt_builder_includes_transcript_and_notes() {
        let messages = PromptBuilder::build(
            "Alice: hello\nBob: hi",
            "action items",
            "Transcript:\n{transcript}\n\nNotes:\n{notes}",
        );
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        assert!(messages[1].role == "user");
        assert!(messages[1].content.contains("Alice: hello"));
        assert!(messages[1].content.contains("action items"));
    }

    #[tokio::test]
    async fn fake_provider_yields_scripted_stream() {
        let provider = FakeProvider {
            events: vec![
                LlmEvent::Chunk("Hello".into()),
                LlmEvent::Chunk(" world".into()),
                LlmEvent::Done,
            ],
        };
        let messages = PromptBuilder::build("t", "n", "{transcript} {notes}");
        let mut stream = provider.complete(&messages).await.unwrap();

        let mut collected = Vec::new();
        while let Some(event) = stream.next().await {
            collected.push(event);
        }

        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0], LlmEvent::Chunk("Hello".into()));
        assert_eq!(collected[1], LlmEvent::Chunk(" world".into()));
        assert_eq!(collected[2], LlmEvent::Done);
    }

    #[tokio::test]
    async fn fake_provider_propagates_error_event() {
        let provider = FakeProvider {
            events: vec![LlmEvent::Error("boom".into())],
        };
        let messages = PromptBuilder::build("t", "n", "{transcript} {notes}");
        let mut stream = provider.complete(&messages).await.unwrap();

        let event = stream.next().await.unwrap();
        assert_eq!(event, LlmEvent::Error("boom".into()));
        assert!(stream.next().await.is_none());
    }
}
