# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root.
- **`docs/adr/`** — read ADRs that touch the area you're about to work in, when that directory exists.

If any of these files don't exist, proceed silently. The `/domain-modeling` skill creates them lazily when terms or decisions actually get resolved.

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a domain test name, or a report), use the term as defined in `CONTEXT.md`. If the concept is not in the glossary, treat that as a signal to either reconsider the wording or resolve the vocabulary gap through `/domain-modeling`.

## Single-context layout

This repository has one root `CONTEXT.md`. Architectural decisions belong in `docs/adr/` and should be created lazily when a decision is hard to reverse, surprising without context, and the result of a real trade-off.

## Flag conflicts

If an output contradicts an existing ADR or the glossary, surface the conflict explicitly rather than silently overriding it.
