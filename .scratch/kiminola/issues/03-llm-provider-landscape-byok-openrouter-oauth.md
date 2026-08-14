# LLM provider landscape: BYOK, OpenRouter, and subscription OAuth status

Type: research
Status: resolved

## Question

What is the current landscape for LLM providers in a third-party desktop client, to inform Kiminola's provider-pluggable architecture?

Cover:

- **BYOK patterns**: how mature OSS apps (e.g. OpenCode, Aider, Cline, Obsidian plugins) structure provider plugins; key storage best practice on Windows (DPAPI, OS keychain abstractions).
- **OpenRouter**: API shape (OpenAI-compatible?), model coverage, pricing mechanics, anything a desktop client needs to know.
- **Subscription OAuth status**: current state of using ChatGPT (Plus/Pro) and Claude (Pro/Max) consumer subscriptions via OAuth in third-party clients. What do OpenAI and Anthropic officially allow as of 2026? What happened to tools that rode on subscription OAuth (crackdowns, ToS changes)? Is there any sanctioned path (e.g. Anthropic's Claude Code subscription usage terms, OpenAI's sign-in-with-ChatGPT)?
- **Risk assessment**: likelihood and blast radius of access being revoked for OAuth-based approaches.

Output: facts + a recommended provider architecture sketch (interface seams, what's pluggable), with dated sources.

## Answer

Full findings: [research/03-llm-provider-landscape-byok-openrouter-oauth.md](../research/03-llm-provider-landscape-byok-openrouter-oauth.md) (2026-08-12).

- **OAuth verdict**: Claude Pro/Max subscription OAuth in third-party clients is dead — Anthropic's legal docs explicitly forbid it (even via their own Agent SDK) and have enforced server-side since 2026-01-09, with account-suspension risk. Do not build it. ChatGPT Plus/Pro OAuth ("Sign in with ChatGPT", Codex flow) is the opposite: documented by OpenAI, tolerated/blessed for third parties (OpenCode ships it; OpenAI engineer confirmed in codex discussion #8338) — viable as an optional experimental plugin, but with no contractual guarantee.
- **BYOK patterns**: OpenCode = Vercel AI SDK + Models.dev registry + `/connect` credential store; Cline = OS-backed SecretStorage; universal escape hatch is a parameterized OpenAI-compatible provider (baseURL + key). Store keys via OS keychain: DPAPI/Windows Credential Manager (Electron `safeStorage` or `keyring` crate), never plaintext.
- **OpenRouter**: one OpenAI-compatible endpoint (`/api/v1/chat/completions`), one `sk-or-` key, hundreds of models incl. all frontier labs, pass-through pricing (fee only at credit purchase), automatic fallbacks, free `:free` variants — the ideal MVP aggregator.
- **Architecture**: single `ChatProvider` seam (auth, listModels, complete); one parameterized OpenAI-compatible implementation covers OpenAI/OpenRouter/local servers; Anthropic native adapter optional; MVP = OpenRouter + direct OpenAI/Anthropic + custom base URL; ChatGPT OAuth as labeled-experimental plugin post-MVP.
