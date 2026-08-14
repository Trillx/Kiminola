# LLM provider architecture and the OAuth call

Type: grilling
Status: resolved
Blocked by: 03

## Question

Lock the provider-pluggable architecture for note enhancement, and make the deferred subscription-OAuth call, with the user (one question at a time), grounded in ticket 03's findings:

- Provider interface seam: what operations must a provider support (summarize, merge notepad+transcript, streaming?)
- Which providers ship in MVP (OpenRouter? direct OpenAI/Anthropic keys?)
- Whether ChatGPT/Claude subscription OAuth is in, out, or experimental — based on the researched ToS/risk picture
- Key storage approach on Windows/macOS/Linux

Feeds the spec's provider section.

## Answer

Decided with the user (one question at a time), grounded in ticket 03's research:

- **Provider seam**: one narrow `ChatProvider` interface (auth, list models, complete) with **streaming (SSE) completions from day one** — used by the enhancement UI for token-by-token rendering.
- **MVP providers**: a single **OpenAI-compatible implementation** (baseURL + key) covering OpenRouter, OpenAI direct, Ollama, LM Studio, and any compatible custom endpoint — plus a **ChatGPT subscription OAuth plugin, shipped and labeled experimental** (OpenAI currently tolerates this per ticket 03; no contractual guarantee, so it must never be the default or the only path). **No native Anthropic adapter** (Claude reached via OpenRouter). **No Claude-subscription OAuth, ever** — Anthropic explicitly forbids and enforces it.
- **Key/token storage**: OS keychain via the Rust `keyring` crate (Windows Credential Manager/DPAPI, macOS Keychain, Linux Secret Service). No plaintext secrets, no home-rolled vault.
- Post-meeting enhancement needs no tool-calling/agent loop; the seam stays narrow on purpose.
