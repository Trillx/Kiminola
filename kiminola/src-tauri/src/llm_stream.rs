//! Byte-oriented SSE framing shared by Meeting notes and Dictation cleanup.
//! Decode UTF-8 only after an entire line has arrived, not per network chunk.

use futures::stream::{self, BoxStream, Stream, StreamExt};

use super::LlmEvent;
use std::time::Duration;

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub frame_bytes: usize,
    pub output_bytes: usize,
    pub wire_bytes: usize,
    pub timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            frame_bytes: 64 * 1024,
            output_bytes: 1024 * 1024,
            wire_bytes: 16 * 1024 * 1024,
            timeout: Duration::from_secs(15 * 60),
        }
    }
}

#[derive(Default)]
struct Decoder {
    line: Vec<u8>,
    data: String,
    skip_lf: bool,
    terminal: bool,
    stopped: bool,
    has_fields: bool,
    seen_line: bool,
    error_event: bool,
    frame_bytes: usize,
    output_bytes: usize,
    wire_bytes: usize,
    limits: Limits,
}

impl Decoder {
    fn fail(&mut self, message: &str) -> LlmEvent {
        self.terminal = true;
        LlmEvent::Error(message.into())
    }

    fn byte(&mut self, byte: u8) -> Option<LlmEvent> {
        if self.skip_lf && byte == b'\n' {
            self.skip_lf = false;
            return None;
        }
        self.frame_bytes += 1;
        if self.frame_bytes > self.limits.frame_bytes {
            return Some(self.fail("provider stream frame exceeded the limit"));
        }
        self.skip_lf = byte == 13;
        if byte != b'\n' && byte != 13 {
            self.line.push(byte);
            return None;
        }
        let line = std::mem::take(&mut self.line);
        let Ok(line) = std::str::from_utf8(&line) else {
            return Some(self.fail("provider stream contained invalid UTF-8"));
        };
        let line = if !std::mem::replace(&mut self.seen_line, true) {
            line.strip_prefix('\u{feff}').unwrap_or(line)
        } else {
            line
        };
        if line.is_empty() {
            return self.frame();
        }
        self.has_fields = true;
        if let Some(value) = line.strip_prefix("event:") {
            self.error_event = value.trim() == "error";
        }
        if let Some(value) = line.strip_prefix("data:") {
            if !self.data.is_empty() {
                self.data.push('\n');
            }
            self.data.push_str(value.strip_prefix(' ').unwrap_or(value));
        }
        None
    }

    fn frame(&mut self) -> Option<LlmEvent> {
        self.has_fields = false;
        self.frame_bytes = 0;
        let data = std::mem::take(&mut self.data);
        if std::mem::take(&mut self.error_event) {
            return Some(self.fail("provider reported a stream error"));
        }
        if data.is_empty() {
            return None;
        }
        if data.trim() == "[DONE]" {
            return Some(self.finish());
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&data) else {
            return Some(self.fail("provider returned an invalid stream frame"));
        };
        if value.get("error").is_some_and(|error| !error.is_null()) {
            return Some(self.fail("provider reported a stream error"));
        }
        let Some(choices) = value.get("choices").and_then(|v| v.as_array()) else {
            return Some(self.fail("provider returned an invalid stream frame"));
        };
        // OpenAI's optional usage-only frame follows the stop frame.
        if choices.is_empty() && value.get("usage").is_some_and(|v| v.is_object()) {
            return None;
        }
        if choices.len() != 1 {
            return Some(self.fail("provider returned unexpected completion choices"));
        }
        let choice = &choices[0];
        if choice.get("index").is_some_and(|v| v.as_u64() != Some(0)) {
            return Some(self.fail("provider returned unexpected completion choices"));
        }
        if choice.get("error").is_some_and(|v| !v.is_null()) {
            return Some(self.fail("provider reported a stream error"));
        }
        let Some(delta) = choice.get("delta").and_then(|v| v.as_object()) else {
            return Some(self.fail("provider returned an invalid stream frame"));
        };
        for field in ["refusal", "tool_calls", "function_call"] {
            let absent = match delta.get(field) {
                None | Some(serde_json::Value::Null) => true,
                Some(serde_json::Value::String(text)) if field == "refusal" => text.is_empty(),
                Some(serde_json::Value::Array(calls)) if field == "tool_calls" => calls.is_empty(),
                _ => false,
            };
            if !absent {
                return Some(self.fail("provider returned a refusal or non-text completion"));
            }
        }
        let text = match delta.get("content") {
            Some(serde_json::Value::String(text)) => text.as_str(),
            None | Some(serde_json::Value::Null) => "",
            _ => return Some(self.fail("provider returned non-text content")),
        };
        if self.stopped {
            return Some(self.fail("provider sent content after completion"));
        }
        match choice.get("finish_reason") {
            None | Some(serde_json::Value::Null) => {}
            Some(serde_json::Value::String(reason)) if reason == "stop" => self.stopped = true,
            _ => return Some(self.fail("provider did not finish the text successfully")),
        }
        if text.len() > self.limits.output_bytes.saturating_sub(self.output_bytes) {
            return Some(self.fail("provider output exceeded the limit"));
        }
        self.output_bytes += text.len();
        (!text.is_empty()).then(|| LlmEvent::Chunk(text.to_owned()))
    }

    // A parsed stop frame is the successful finish signal. [DONE] or clean
    // HTTP EOF can close it; neither can certify a stream without that signal.
    fn finish(&mut self) -> LlmEvent {
        if !self.stopped || !self.line.is_empty() || self.has_fields {
            return self.fail("provider stream ended before successful completion");
        }
        self.terminal = true;
        LlmEvent::Done
    }
}

pub(super) fn decode<S, B, E>(source: S, limits: Limits) -> BoxStream<'static, LlmEvent>
where
    S: Stream<Item = Result<B, E>> + Send + 'static,
    B: AsRef<[u8]> + Send + 'static,
    E: Send + 'static,
{
    let deadline = tokio::time::Instant::now() + limits.timeout;
    stream::unfold(
        (
            source.boxed(),
            Decoder {
                limits,
                ..Decoder::default()
            },
            None::<B>,
            0,
        ),
        move |(mut source, mut decoder, mut current, mut offset)| async move {
            if decoder.terminal {
                return None;
            }
            loop {
                if let Some(bytes) = current.as_ref() {
                    while offset < bytes.as_ref().len() {
                        if offset % 1024 == 0 && tokio::time::Instant::now() >= deadline {
                            let event = decoder.fail("provider stream timed out");
                            return Some((event, (source, decoder, None, 0)));
                        }
                        let byte = bytes.as_ref()[offset];
                        offset += 1;
                        if let Some(event) = decoder.byte(byte) {
                            return Some((event, (source, decoder, current, offset)));
                        }
                    }
                }
                match tokio::time::timeout_at(deadline, source.next()).await {
                    Ok(Some(Ok(bytes))) => {
                        if bytes.as_ref().len()
                            > decoder.limits.wire_bytes.saturating_sub(decoder.wire_bytes)
                        {
                            let event = decoder.fail("provider stream exceeded the byte limit");
                            return Some((event, (source, decoder, None, 0)));
                        }
                        decoder.wire_bytes += bytes.as_ref().len();
                        current = Some(bytes);
                        offset = 0;
                    }
                    Ok(Some(Err(_))) => {
                        let event = decoder.fail("provider stream interrupted");
                        return Some((event, (source, decoder, None, 0)));
                    }
                    Err(_) => {
                        let event = decoder.fail("provider stream timed out");
                        return Some((event, (source, decoder, None, 0)));
                    }
                    Ok(None) => {
                        let event = decoder.finish();
                        return Some((event, (source, decoder, None, 0)));
                    }
                }
            }
        },
    )
    .boxed()
}
