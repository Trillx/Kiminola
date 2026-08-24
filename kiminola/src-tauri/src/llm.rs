//! OpenAI-compatible LLM provider seam (SPEC.md §3 / §6).
//!
//! Provider config (kind, base URL, model) is stored in SQLite `settings`.
//! The API key is stored in the OS keychain via `keyring`.
//! Streaming completions emit `llm:chunk`, `llm:done`, and `llm:error` events.

use std::time::Duration;

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use reqwest::header::{self, HeaderMap};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::db::{ensure_pool, update_enhanced_notes_impl, DbState};

const CONFIG_KEY: &str = "llm_config";
const KEYRING_SERVICE: &str = "kiminola";
const KEYRING_ACCOUNT: &str = "provider_api_key";

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

/// Provider settings returned to the UI. The credential itself never leaves
/// the OS keychain; only its presence is exposed for accurate status copy.
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

/// A single chat message for the completion endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Events yielded by a streaming completion.
#[derive(Debug, Clone, PartialEq, Eq)]
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
            headers.insert(
                header::AUTHORIZATION,
                format!("Bearer {api_key}")
                    .parse()
                    .map_err(|e| format!("invalid api key: {e}"))?,
            );
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            // Without these, a stalled provider stream hangs the enhancement
            // forever. read_timeout bounds idle time between chunks, not the
            // total generation length, so slow-but-alive models still finish.
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("failed to build http client: {e}"))?;

        Ok(Self {
            client,
            base_url: config.base_url.trim_end_matches('/').to_string(),
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

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("keyring entry failed: {e}"))
}

fn load_api_key() -> Result<Option<String>, String> {
    match keyring_entry()?.get_password() {
        Ok(key) if key.is_empty() => Ok(None),
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("failed to read api key: {e}")),
    }
}

fn save_api_key(api_key: Option<String>) -> Result<(), String> {
    let entry = keyring_entry()?;
    match api_key {
        None => Ok(()),
        Some(key) if key.trim().is_empty() => entry.delete_credential().or_else(|e| match e {
            keyring::Error::NoEntry => Ok(()),
            _ => Err(format!("failed to delete api key: {e}")),
        }),
        Some(key) => entry
            .set_password(&key)
            .map_err(|e| format!("failed to save api key: {e}")),
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
    let key = load_api_key()?.unwrap_or_default();
    OpenAiCompatibleProvider::new(config, key)
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
    let config = load_config(&pool).await?;
    let has_api_key = load_api_key()?.is_some();
    Ok(ProviderConfigView::new(config, has_api_key))
}

#[tauri::command]
pub async fn set_llm_config(
    state: State<'_, DbState>,
    config: ProviderConfig,
    api_key: Option<String>,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    save_config(&pool, &config).await?;
    save_api_key(api_key)?;
    Ok(())
}

#[tauri::command]
pub async fn test_llm_config(app: AppHandle, state: State<'_, DbState>) -> Result<(), String> {
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
                if let Err(e) = app.emit("llm:chunk", &chunk) {
                    return Err(format!("failed to emit chunk: {e}"));
                }
            }
            LlmEvent::Done => break,
            LlmEvent::Error(e) => return Err(e),
        }
    }

    if full.trim().is_empty() {
        return Err("provider returned no content".to_string());
    }
    app.emit("llm:done", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn enhance_meeting(
    app: AppHandle,
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

    // Bridge the provider stream to Tauri events and persist the final result.
    let app = app.clone();
    let pool = pool.clone();
    tauri::async_runtime::spawn(async move {
        let mut full = String::new();
        let mut stream = stream;
        while let Some(event) = stream.next().await {
            match event {
                LlmEvent::Chunk(chunk) => {
                    full.push_str(&chunk);
                    if let Err(e) = app.emit("llm:chunk", &chunk) {
                        eprintln!("failed to emit llm:chunk: {e}");
                    }
                }
                LlmEvent::Done => {
                    if full.trim().is_empty() {
                        let _ = app.emit("llm:error", "provider returned no content");
                        return;
                    }
                    if let Err(e) = update_enhanced_notes_impl(&pool, meeting_id, &full).await {
                        let _ = app.emit("llm:error", e);
                        return;
                    }
                    let _ = app.emit("llm:done", ());
                    return;
                }
                LlmEvent::Error(msg) => {
                    let _ = app.emit("llm:error", msg);
                    return;
                }
            }
        }

        // Stream ended without an explicit [DONE] marker.
        if full.trim().is_empty() {
            let _ = app.emit("llm:error", "provider returned no content");
        } else {
            match update_enhanced_notes_impl(&pool, meeting_id, &full).await {
                Ok(()) => {
                    let _ = app.emit("llm:done", ());
                }
                Err(e) => {
                    let _ = app.emit("llm:error", e);
                }
            }
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

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
