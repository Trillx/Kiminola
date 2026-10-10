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
use tauri::{ipc::Channel, State};

use crate::db::{meeting_store, settings, templates, DbState};

const CONFIG_KEY: &str = "llm_config";
const KEYRING_SERVICE: &str = "kiminola";
const KEYRING_ACCOUNT: &str = "provider_api_key";
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
    match settings::get(pool, CONFIG_KEY).await? {
        Some(json) => serde_json::from_str(&json).map_err(|e| format!("bad config: {e}")),
        None => Ok(ProviderConfig::default()),
    }
}

async fn save_config(pool: &sqlx::SqlitePool, config: &ProviderConfig) -> Result<(), String> {
    let json = serde_json::to_string(config).map_err(|e| e.to_string())?;
    settings::set(pool, CONFIG_KEY, &json).await
}

fn build_provider(config: &ProviderConfig) -> Result<OpenAiCompatibleProvider, String> {
    let key = load_api_key()?.unwrap_or_default();
    OpenAiCompatibleProvider::new(config, key)
}

#[tauri::command]
pub async fn get_llm_config(state: State<'_, DbState>) -> Result<ProviderConfigView, String> {
    let pool = state.pool().await?;
    let config = load_config(&pool).await?;
    let has_api_key = load_api_key()?.is_some();
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
    let pool = state.pool().await?;
    save_config(&pool, &config).await?;
    save_api_key(api_key)?;
    Ok(())
}

#[tauri::command]
pub async fn test_llm_config(
    state: State<'_, DbState>,
    on_event: Channel<LlmEvent>,
) -> Result<(), String> {
    let pool = state.pool().await?;
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


const ENHANCEMENT_SYSTEM_PROMPT: &str =
    "You are a concise meeting-note assistant. Respond in Markdown.";

/// A Note enhancement whose provider stream is open. Running it forwards
/// chunks to the caller and persists the enhanced notes when the stream
/// completes.
pub(crate) struct Enhancement {
    pool: sqlx::SqlitePool,
    meeting_id: i64,
    stream: BoxStream<'static, LlmEvent>,
}

/// Start a Note enhancement for `meeting_id` with `provider`.
///
/// Selects the template (`template_id`, or the default first template), sends
/// the channel-labelled transcript plus the Notepad through the template's
/// `{transcript}`/`{notes}` placeholders, and opens the provider stream.
/// Unknown Meetings or templates and rejected provider requests fail here,
/// before any event is delivered or anything is persisted.
pub(crate) async fn begin_enhancement(
    pool: &sqlx::SqlitePool,
    provider: &dyn ChatProvider,
    meeting_id: i64,
    template_id: Option<i64>,
) -> Result<Enhancement, String> {
    let meeting = meeting_store::get_meeting(pool, meeting_id).await?;
    let template = templates::resolve(pool, template_id).await?;

    let transcript = meeting
        .transcript
        .iter()
        .map(|segment| format!("{}: {}", segment.channel, segment.text))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = template
        .prompt
        .replace("{transcript}", &transcript)
        .replace("{notes}", &meeting.notepad);
    let messages = [
        Message {
            role: "system".into(),
            content: ENHANCEMENT_SYSTEM_PROMPT.into(),
        },
        Message {
            role: "user".into(),
            content: prompt,
        },
    ];

    let stream = provider.complete(&messages).await?;
    Ok(Enhancement {
        pool: pool.clone(),
        meeting_id,
        stream,
    })
}

impl Enhancement {
    /// Forward every chunk to `on_event`, then send exactly one terminal event:
    /// `Done` after the full text was saved as the Meeting's enhanced notes, or
    /// `Error` (provider error, empty output, or save failure). On error the
    /// previous enhanced notes are kept. Persistence does not depend on the
    /// channel still being open, so closing the page cannot lose a result.
    pub(crate) async fn run(mut self, on_event: &Channel<LlmEvent>) {
        let mut full = String::new();
        while let Some(event) = self.stream.next().await {
            match event {
                LlmEvent::Chunk(chunk) => {
                    full.push_str(&chunk);
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
            meeting_store::update_enhanced_notes(&self.pool, self.meeting_id, &full).await
        };
        let event = match result {
            Ok(()) => LlmEvent::Done,
            Err(error) => LlmEvent::Error(error),
        };
        let _ = on_event.send(event);
    }
}

#[tauri::command]
pub async fn enhance_meeting(
    on_event: Channel<LlmEvent>,
    state: State<'_, DbState>,
    meeting_id: i64,
    template_id: Option<i64>,
) -> Result<(), String> {
    let pool = state.pool().await?;
    let config = load_config(&pool).await?;
    let provider = build_provider(&config)?;
    let enhancement = begin_enhancement(&pool, &provider, meeting_id, template_id).await?;
    tauri::async_runtime::spawn(async move { enhancement.run(&on_event).await });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::meeting_store::{MeetingToSave, NewSegment};
    use crate::db::test_support::TestDb;
    use std::sync::{Arc, Mutex};

    fn capture_channel() -> (Channel<LlmEvent>, Arc<Mutex<Vec<LlmEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
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

    /// Scripted provider that records every request it receives.
    struct FakeProvider {
        events: Vec<LlmEvent>,
        requests: Mutex<Vec<Vec<Message>>>,
    }

    impl FakeProvider {
        fn new(events: Vec<LlmEvent>) -> Self {
            Self {
                events,
                requests: Mutex::new(Vec::new()),
            }
        }

        fn user_prompts(&self) -> Vec<String> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|messages| messages.last().unwrap().content.clone())
                .collect()
        }
    }

    #[async_trait]
    impl ChatProvider for FakeProvider {
        async fn complete(
            &self,
            messages: &[Message],
        ) -> Result<BoxStream<'static, LlmEvent>, String> {
            self.requests.lock().unwrap().push(messages.to_vec());
            Ok(Box::pin(stream::iter(self.events.clone())))
        }
    }

    async fn meeting(
        pool: &sqlx::SqlitePool,
        notepad: &str,
        previous_enhancement: Option<&str>,
    ) -> i64 {
        let segments = [
            NewSegment {
                channel: "you".into(),
                text: "Can you send the deck?".into(),
                start_ms: Some(0),
                end_ms: Some(900),
            },
            NewSegment {
                channel: "others".into(),
                text: "Sure, by Friday.".into(),
                start_ms: Some(1_000),
                end_ms: Some(1_800),
            },
        ];
        let id = meeting_store::save_meeting(
            pool,
            MeetingToSave {
                title: "Sync",
                duration_seconds: 60,
                notepad,
                segments: &segments,
                ..MeetingToSave::default()
            },
        )
        .await
        .unwrap();
        if let Some(previous) = previous_enhancement {
            meeting_store::update_enhanced_notes(pool, id, previous)
                .await
                .unwrap();
        }
        id
    }

    async fn enhanced_notes(pool: &sqlx::SqlitePool, id: i64) -> Option<String> {
        meeting_store::get_meeting(pool, id)
            .await
            .unwrap()
            .enhanced_markdown
    }

    #[tokio::test]
    async fn enhancement_fills_the_selected_template_and_persists_the_stream() {
        let db = TestDb::new("enhance-template").await;
        let id = meeting(&db.pool, "deck owner?", None).await;
        let template = templates::create(&db.pool, "Terse", "T:\n{transcript}\nN:{notes}")
            .await
            .unwrap();
        let provider = FakeProvider::new(vec![
            LlmEvent::Chunk("## Actions\n".into()),
            LlmEvent::Chunk("- Deck by Friday".into()),
            LlmEvent::Done,
        ]);
        let (channel, events) = capture_channel();

        begin_enhancement(&db.pool, &provider, id, Some(template.id))
            .await
            .unwrap()
            .run(&channel)
            .await;

        assert_eq!(
            provider.user_prompts(),
            vec!["T:\nyou: Can you send the deck?\nothers: Sure, by Friday.\nN:deck owner?"]
        );
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                LlmEvent::Chunk("## Actions\n".into()),
                LlmEvent::Chunk("- Deck by Friday".into()),
                LlmEvent::Done
            ]
        );
        assert_eq!(
            enhanced_notes(&db.pool, id).await.as_deref(),
            Some("## Actions\n- Deck by Friday")
        );
        let detail = meeting_store::get_meeting(&db.pool, id).await.unwrap();
        assert_eq!(detail.notepad, "deck owner?", "the Notepad is never replaced");
    }

    #[tokio::test]
    async fn enhancement_defaults_to_the_first_template_and_works_without_notes() {
        let db = TestDb::new("enhance-default").await;
        let id = meeting(&db.pool, "", None).await;
        let default = templates::resolve(&db.pool, None).await.unwrap();
        let provider = FakeProvider::new(vec![LlmEvent::Chunk("Summary".into())]);
        let (channel, events) = capture_channel();

        begin_enhancement(&db.pool, &provider, id, None)
            .await
            .unwrap()
            .run(&channel)
            .await;

        let expected = default
            .prompt
            .replace(
                "{transcript}",
                "you: Can you send the deck?\nothers: Sure, by Friday.",
            )
            .replace("{notes}", "");
        assert_eq!(provider.user_prompts(), vec![expected]);
        // No explicit DONE marker: the end of the stream completes the result.
        assert_eq!(
            *events.lock().unwrap(),
            vec![LlmEvent::Chunk("Summary".into()), LlmEvent::Done]
        );
        assert_eq!(enhanced_notes(&db.pool, id).await.as_deref(), Some("Summary"));
    }

    #[tokio::test]
    async fn unknown_meeting_or_template_fails_before_contacting_the_provider() {
        let db = TestDb::new("enhance-missing").await;
        let id = meeting(&db.pool, "", Some("Previous")).await;
        let provider = FakeProvider::new(vec![LlmEvent::Done]);

        let error = begin_enhancement(&db.pool, &provider, id, Some(9_999))
            .await
            .err()
            .expect("unknown template");
        assert_eq!(error, "template not found");
        let error = begin_enhancement(&db.pool, &provider, 9_999, None)
            .await
            .err()
            .expect("unknown meeting");
        assert_eq!(error, "meeting not found");

        assert!(provider.user_prompts().is_empty());
        assert_eq!(enhanced_notes(&db.pool, id).await.as_deref(), Some("Previous"));
    }

    #[tokio::test]
    async fn failed_or_empty_streams_keep_the_previous_enhancement() {
        let db = TestDb::new("enhance-failure").await;
        for (script, expected_terminal) in [
            (
                vec![
                    LlmEvent::Chunk("partial".into()),
                    LlmEvent::Error("offline".into()),
                ],
                LlmEvent::Error("offline".into()),
            ),
            (
                vec![LlmEvent::Chunk("  ".into()), LlmEvent::Done],
                LlmEvent::Error("provider returned no content".into()),
            ),
        ] {
            let id = meeting(&db.pool, "", Some("Previous enhancement")).await;
            let provider = FakeProvider::new(script);
            let (channel, events) = capture_channel();
            begin_enhancement(&db.pool, &provider, id, None)
                .await
                .unwrap()
                .run(&channel)
                .await;
            assert_eq!(events.lock().unwrap().last(), Some(&expected_terminal));
            assert_eq!(
                enhanced_notes(&db.pool, id).await.as_deref(),
                Some("Previous enhancement")
            );
        }
    }

    #[tokio::test]
    async fn concurrent_enhancements_keep_channels_and_persistence_separate() {
        let db = TestDb::new("enhance-concurrent").await;
        let a_id = meeting(&db.pool, "Original notes", Some("Previous enhancement")).await;
        let b_id = meeting(&db.pool, "Original notes", Some("Previous enhancement")).await;
        let a_provider = FakeProvider::new(vec![
            LlmEvent::Chunk("partial".into()),
            LlmEvent::Error("offline".into()),
        ]);
        let b_provider =
            FakeProvider::new(vec![LlmEvent::Chunk("B complete".into()), LlmEvent::Done]);
        let (channel_a, events_a) = capture_channel();
        let (channel_b, events_b) = capture_channel();

        let a = begin_enhancement(&db.pool, &a_provider, a_id, None)
            .await
            .unwrap();
        let b = begin_enhancement(&db.pool, &b_provider, b_id, None)
            .await
            .unwrap();
        tokio::join!(a.run(&channel_a), b.run(&channel_b));

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
            enhanced_notes(&db.pool, a_id).await.as_deref(),
            Some("Previous enhancement")
        );
        assert_eq!(
            enhanced_notes(&db.pool, b_id).await.as_deref(),
            Some("B complete")
        );
        for id in [a_id, b_id] {
            let notepad = meeting_store::get_meeting(&db.pool, id).await.unwrap().notepad;
            assert_eq!(notepad, "Original notes");
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
}
