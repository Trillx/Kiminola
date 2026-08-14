# Research 03: LLM provider landscape — BYOK, OpenRouter, and subscription OAuth

Date: 2026-08-12
Ticket: [issues/03-llm-provider-landscape-byok-openrouter-oauth.md](../issues/03-llm-provider-landscape-byok-openrouter-oauth.md)

## TL;DR

- **BYOK is the only safe foundation.** Every mature OSS client (OpenCode, Aider, Cline) is built around user-supplied API keys plus a small number of *vendor-sanctioned* subscription OAuth flows.
- **OpenRouter is the ideal MVP aggregator**: one OpenAI-compatible endpoint, one `sk-or-` key, hundreds of models, pass-through pricing. Integrating it first gives Kiminola near-universal model coverage with a single provider plugin.
- **Subscription OAuth is asymmetric as of 2026**: OpenAI explicitly tolerates/blesses third-party clients using "Sign in with ChatGPT" (Codex) OAuth; Anthropic explicitly **forbids** Claude Pro/Max OAuth tokens outside Claude Code/claude.ai and actively enforces with server-side blocks (since 2026-01-09) and account bans.
- **Recommendation**: ship API-key BYOK (OpenRouter + direct OpenAI/Anthropic/OpenAI-compatible) in MVP; treat ChatGPT-subscription OAuth as an optional, clearly-labeled provider plugin; do **not** build a Claude-subscription OAuth provider at all.

---

## 1. BYOK provider-plugin patterns in mature OSS apps

### OpenCode (TS, TUI) — the current reference architecture

- Uses the **Vercel AI SDK** as the provider abstraction plus **Models.dev** as the model-metadata registry (context limits, pricing, capabilities) — claims 75+ providers ([OpenCode providers docs, fetched 2026-08-12](https://opencode.ai/docs/providers/)).
- Credentials are collected via a `/connect` command and stored in a local `auth.json` (`~/.local/share/opencode/auth.json`); per-provider config (baseURL, whitelist/blacklist, custom headers) lives in `opencode.json`.
- Any **OpenAI-compatible endpoint** can be added as a custom provider with just `{ npm: "@ai-sdk/openai-compatible", options: { baseURL }, models: {...} }` — this is the universal escape hatch and covers local servers (Ollama, LM Studio, llama.cpp) too.
- OAuth-based provider logins (ChatGPT Plus/Pro, GitHub Copilot device code, GitLab Duo, Snowflake, xAI SuperGrok) are implemented as per-provider auth plugins; tokens are refreshed automatically and stored alongside API keys.

### Aider (Python, CLI)

- Multi-provider via a thin layer over LiteLLM-style model metadata; keys supplied via CLI flags, environment variables, or `.env` files ([Aider API keys docs](https://aider.chat/docs/config/api-keys.html)). No keychain integration — env/config files only. Simple but leaves secrets on disk in plaintext if the user chooses `.env`.

### Cline / Roo Code (VS Code extensions)

- Store API keys in **VS Code's `ExtensionContext.secrets` (SecretStorage API)** — OS-backed encrypted storage (on Windows, protected via Electron safeStorage → DPAPI) rather than plaintext settings ([cline issue #5492](https://github.com/cline/cline/issues/5492), 2025-08). This is the desktop-app best practice.

### Key storage best practice for Kiminola (Windows-first desktop)

- **Windows**: DPAPI (`CryptProtectData`, per-user scope) is the baseline — protects against other users on the machine, not against other processes in the same user session. Windows Credential Manager is the alternative store.
- **Electron**: `safeStorage.encryptString()` gives exactly this — Keychain on macOS, DPAPI on Windows, Secret Service/libsecret on Linux — and is what mature apps migrated to after `keytar` was deprecated ([Electron safeStorage docs](https://electronjs.org/fr/docs/latest/api/safe-storage); [safeStorage migration example](https://freek.dev/2103-replacing-keytar-with-electrons-safestorage-in-ray)). Store only the encrypted blob in the config file.
- **Tauri**: `tauri-plugin-stronghold` or the `keyring` crate (Windows Credential Manager / macOS Keychain / Secret Service).
- Keep secrets out of the renderer process and out of logs; decrypt only in the main process at request time.
- Codex CLI's own pattern is a useful precedent: `cli_auth_credentials_store = "file | keyring | auto"` — OS keyring preferred, plaintext file as explicit fallback ([Codex auth docs, fetched 2026-08-12](https://developers.openai.com/codex/auth/)).

## 2. OpenRouter

Primary sources: [OpenRouter quickstart](https://openrouter.ai/docs/quickstart) and [OpenRouter FAQ](https://openrouter.ai/docs/faq), both fetched 2026-08-12.

- **API shape**: OpenAI-compatible REST at `https://openrouter.ai/api/v1` — `/chat/completions`, `/completions`, plus `/api/v1/models` to enumerate the catalog programmatically. Drop-in for the OpenAI SDK by changing `baseURL` + key (`sk-or-...`). SSE streaming via `stream: true`.
- **Coverage**: hundreds of models — frontier (OpenAI, Anthropic, Google, xAI) plus open weights (Llama, DeepSeek, Kimi, Qwen, Mistral), and as of 2026 all modalities (text, image, audio, embeddings, transcription) through one endpoint ([OpenRouter blog, 2026-07-16](https://openrouter.ai/blog/insights/every-modality-one-api/)).
- **Pricing mechanics**: prepaid USD credits; **no markup on inference** — pass-through of provider pricing. OpenRouter earns a fee at credit-purchase time (card ~5%, crypto ~5% per FAQ; exact numbers vary). BYOK (bring your own upstream provider keys through OpenRouter) exists with a plan-dependent free allowance and a 5% fee above it.
- **Routing/fallbacks**: per-request provider routing (`provider.order`, `allow_fallbacks`), automatic fallback on provider error, and model-slug variants (`:free`, `:nitro`, `:floor`, `:exacto`, `:thinking`) — nice-to-haves, not needed for MVP.
- **Free tier**: `:free` model variants with low daily rate limits — useful as a zero-config "try it" path.
- **Privacy**: prompts/completions not logged by default; providers that train on data are excluded from routing unless the user opts in. Worth surfacing in Kiminola's UI since meeting content is sensitive.
- Optional app-attribution headers (`HTTP-Referer`, `X-OpenRouter-Title`) get the app on OpenRouter's public leaderboard — free discovery for an OSS app.
- Note: OpenRouter also covers **audio/transcription models** — relevant if Kiminola ever wants a cloud-transcription fallback to complement local Whisper.

## 3. Subscription OAuth status (the risky question)

### Anthropic — explicitly forbidden, actively enforced

- **Official terms** ([Claude Code legal & compliance doc, fetched 2026-08-12](https://code.claude.com/docs/en/legal-and-compliance)): OAuth tokens are "intended exclusively" for subscription purchasers' use of "Claude Code and other native Anthropic applications"; "Anthropic does not permit third-party developers to offer Claude.ai login or to route requests through Free, Pro, or Max plan credentials on behalf of their users." Developers must use API keys. "Anthropic reserves the right to take measures to enforce these restrictions... without prior notice."
- **Enforcement timeline**:
  - **2026-01-09 ~02:20 UTC**: server-side block activated. OpenCode, Cline, Roo Code, Zed, OpenClaw et al. began receiving `This credential is only authorized for use with Claude Code and cannot be used for other API requests` ([Bito, 2026-06-10](https://bito.ai/ai-tools/opencode-vs-claude-code-a-2026-comparison-after-the-oauth-split/); [RDVCC timeline, 2026-07-29](https://rdvcc.com/en/blog/post/chatgpt-claude-account-ban-reasons-2026)). Early OpenCode versions had been spoofing the `claude-code-20250219` beta header, which is what triggered the fingerprinting crackdown.
  - **2026-02-19/20**: restriction formalized in the legal docs, explicitly naming even Anthropic's own **Agent SDK** as forbidden with subscription tokens ([Awesome Agents, 2026-02-26](https://awesomeagents.ai/news/claude-code-oauth-policy-third-party-crackdown/); [nanoclaw issue #312](https://github.com/qwibitai/nanoclaw/issues/312)).
  - **2026-02**: OpenCode removed all Claude OAuth code "citing Anthropic legal requests" ([indianprompt, 2026-06-24](https://www.indianprompt.com/opencode-vs-claude-code-2026/)); current OpenCode docs state plainly "Anthropic explicitly prohibits this" and recommend an API key.
  - **2026-04-04**: extended to Team accounts — third-party harnesses on Team plans now bill from "extra usage" credits, not subscription seats (Anthropic admin emails, via [OpenCode weekly W15](https://blog.csdn.net/evilstar2015/article/details/160078945)).
  - **2026-04+**: anti-spoofing hardened (client fingerprinting, header checks); spoofing attempts now risk **account suspension** ([QCode, 2026-07-05](https://qcode.cc/en/claude-code-account-ban-guide)).
- **Sanctioned Claude paths for third parties**: Anthropic API key (Console), Bedrock, Vertex. One gray-zone pattern exists — shells that spawn the *actual* `claude` CLI binary as a subprocess (e.g. T3 Code) delegate auth entirely to Anthropic's own client and have not been blocked ([gsd-2 issue #3772](https://github.com/gsd-build/gsd-2/issues/3772)) — but that's delegating to Claude Code, not using Claude as an LLM provider, and is wrong-shaped for note enhancement.

### OpenAI — explicitly tolerated, de-facto sanctioned

- **Official docs** ([Codex auth docs, fetched 2026-08-12](https://developers.openai.com/codex/auth/)): "Sign in with ChatGPT for subscription access" is a documented, first-class auth method with browser PKCE and a device-code flow for headless use; tokens auto-refresh; keyring storage supported.
- **Third-party stance**: In the official repo, an OpenAI engineer confirmed forks/clients using standard "Sign in with ChatGPT" are fine and pointed to OpenCode as precedent: "our terms of use and code license are quite permissive, and OSS projects like OpenCode are doing things similar to what you're describing" ([openai/codex discussion #8338, 2025-12](https://github.com/openai/codex/discussions/8338)).
- **The drama**: when Anthropic cut off third parties on 2026-01-09, OpenAI counter-positioned — OpenCode shipped Codex OAuth within ~24h (v1.1.11, 2026-01-10) and OpenAI publicly welcomed it ([Dev Genius, 2026-01-10](https://blog.devgenius.io/jump-ship-in-minutes-codex-oauth-now-works-in-opencode-d2708c32f571); [byteiota, 2026-05-19](https://byteiota.com/opencode-open-source-ai-coding-agent/)). Sam Altman publicly endorsed OpenClaw's use of the same pattern (2026-05-01, per codex discussion #8338).
- **Caveats**: there is **no documented public contract** guaranteeing third-party access to the ChatGPT-backed Responses endpoint — it's "CLI-internal by design, tolerated in practice" ([openai/codex issue #36886, 2026-08-04](https://github.com/openai/codex/issues/36886), unanswered by OpenAI at time of writing). Model availability via OAuth is an allowlist (e.g. some `-codex` model IDs are rejected on ChatGPT-account auth). Terms could change; OpenAI could follow Anthropic's playbook at any time.
- Other sanctioned subscription-OAuth precedents: **GitHub Copilot** (device flow, used by OpenCode/Cline), GitLab Duo, xAI SuperGrok.

## 4. Risk assessment

| Approach | Revocation likelihood | Blast radius if revoked | Notes |
|---|---|---|---|
| BYOK API keys (OpenAI, Anthropic, OpenRouter, OpenAI-compatible) | Very low | None structural — it's the vendor's business model | Safe foundation. |
| OpenRouter as aggregator | Low | Medium — single point of failure; mitigate by keeping direct-provider plugins | Pass-through pricing; outage risk only. |
| ChatGPT Plus/Pro OAuth ("Sign in with ChatGPT") | Low-medium | Medium — feature loss, not account bans; no reported bans for client use | Tolerated today, no contractual guarantee; OpenAI could tighten without notice. |
| Claude Pro/Max OAuth in a third-party client | **Certainty — already revoked** | High — server-side block + **user account suspension risk**; header spoofing aggravates | Do not ship. Would also expose Kiminola users' paid accounts to bans. |

## 5. Recommended provider architecture for Kiminola

Note enhancement is a simple workload: transcript text in → structured notes/summary out. No tool calling or agent loop required, so the seam can be narrow.

```
┌────────────────────────────────────────────────────────┐
│ NoteEnhancer (core)                                    │
│   enhance(transcript, style, providerRef) → notes      │
└──────────────────────┬─────────────────────────────────┘
                       │  ChatProvider interface (the one seam)
        ┌──────────────┼───────────────┬──────────────────┐
   OpenAI-compat    OpenRouter      Anthropic         (optional)
   provider         provider        provider          ChatGPT-OAuth
   (baseURL+key:    (sk-or- key,    (api key,         provider
    OpenAI direct,  model slug)     Messages API)     (experimental)
    local Ollama/LM Studio…)
                       │
        ┌──────────────┴──────────────┐
   CredentialStore (per-OS keychain)  ModelRegistry (id →
   Windows: DPAPI/CredMan via         context limit, pricing,
   Electron safeStorage / keyring     capabilities — Models.dev
   crate; macOS Keychain; linux       JSON or OpenRouter /models)
   Secret Service
```

- **One interface**: `ChatProvider { id, displayName, auth: ApiKey|OAuth|None, listModels(), complete(messages, opts) → text (streaming optional) }`. Everything else (prompt templates, note styles, retries) is provider-agnostic core code.
- **Default to an OpenAI-compatible core implementation** parameterized by `baseURL` + key — that one class covers OpenAI direct, OpenRouter, and local servers. Anthropic gets a small native adapter (Messages API) or is reached via OpenRouter with zero extra code.
- **MVP provider set**: OpenRouter (one key → all models), OpenAI direct, Anthropic direct, custom OpenAI-compatible (base URL field — instantly covers Ollama/LM Studio/llama.cpp for fully-offline-after-transcription users).
- **ChatGPT-subscription OAuth**: optional post-MVP plugin, behind an "experimental / terms may change" label, using the documented browser PKCE or device-code flow with OS-keyring token storage. Never spoof client identity.
- **Claude-subscription OAuth**: do not implement. Detecting-and-reusing `~/.claude/.credentials.json` tokens is a ToS violation that endangers users' accounts.
- **What's pluggable**: provider list, model registry, credential store, and (later) auth-flow plugins. What's core: prompt assembly, transcript chunking, note rendering, error/retry handling.
- **Privacy surface**: meeting transcripts are sensitive — show per-provider data policy hints (OpenRouter's no-training routing, OpenAI/Anthropic API non-training defaults) in provider setup UI.

## Sources

- [OpenCode providers docs](https://opencode.ai/docs/providers/) — fetched 2026-08-12
- [Codex authentication docs](https://developers.openai.com/codex/auth/) — fetched 2026-08-12
- [openai/codex discussion #8338](https://github.com/openai/codex/discussions/8338) — 2025-12-19
- [openai/codex issue #36886 (no documented third-party OAuth contract)](https://github.com/openai/codex/issues/36886) — 2026-08-04
- [Claude Code legal and compliance](https://code.claude.com/docs/en/legal-and-compliance) — fetched 2026-08-12
- [Awesome Agents: Anthropic Locks Down Claude Code](https://awesomeagents.ai/news/claude-code-oauth-policy-third-party-crackdown/) — 2026-02-26
- [Bito: OpenCode vs Claude Code after the OAuth split](https://bito.ai/ai-tools/opencode-vs-claude-code-a-2026-comparison-after-the-oauth-split/) — 2026-06-10
- [RDVCC: 2025-2026 enforcement timeline](https://rdvcc.com/en/blog/post/chatgpt-claude-account-ban-reasons-2026) — 2026-07-29
- [QCode: Claude account ban guide](https://qcode.cc/en/claude-code-account-ban-guide) — 2026-07-05
- [Dev Genius: Codex OAuth in OpenCode](https://blog.devgenius.io/jump-ship-in-minutes-codex-oauth-now-works-in-opencode-d2708c32f571) — 2026-01-10
- [byteiota: OpenCode guide](https://byteiota.com/opencode-open-source-ai-coding-agent/) — 2026-05-19
- [OpenRouter quickstart](https://openrouter.ai/docs/quickstart) and [OpenRouter FAQ](https://openrouter.ai/docs/faq) — fetched 2026-08-12
- [OpenRouter: every modality through one API](https://openrouter.ai/blog/insights/every-modality-one-api/) — 2026-07-16
- [Electron safeStorage docs](https://electronjs.org/fr/docs/latest/api/safe-storage)
- [Aider API keys docs](https://aider.chat/docs/config/api-keys.html)
- [cline issue #5492 (SecretStorage)](https://github.com/cline/cline/issues/5492) — 2025-08-11
- [gsd-2 issue #3772 (T3 Code claude-CLI delegation pattern)](https://github.com/gsd-build/gsd-2/issues/3772) — 2026-04-08
