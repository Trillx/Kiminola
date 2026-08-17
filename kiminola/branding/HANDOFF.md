# Kimi Nola — Handoff Spec (Rev A)

**To the implementing agent.** This spec + the identity sheet (`app/index.html`, the tie-breaker — when in doubt, match it exactly) are sufficient to build the Kimi Nola brand surface in any stack without follow-up questions. All assets live in `branding/`.

## 1. Concept

**Oatwave.** An oat grain — the *granola* namesake — whose natural striations are a live audio waveform. One mark carries both halves of the product: meeting notes (the grain) and voice dictation (the wave). The husk path is the clip mask for the bars, so longer bars end flush at the shell like real grain texture. Warm earth palette, editorial serif voice, mono for metadata. The identity is light-first (paper cream) with a charcoal dark mode; the app tile is always charcoal.

## 2. Design tokens

| Token | Value | Usage |
|---|---|---|
| `--paper` | `#F2EFE9` | Default light background; wordmark ink on dark panels |
| `--beige` | `#DFD7C8` | Cards, sidebar surfaces, inactive states on light |
| `--gold` | `#CCC0A8` | Muted gold — **dark surfaces only**: recording state, the gold bar, emphasis |
| `--deepgold` | `#A88B4F` | Gold on **light surfaces only**: links, accents, the "nola" in the wordmark |
| `--ink` | `#242424` | Text, app tile, dark-mode window chrome |
| `--ink2` | `#2E2E2E` | Derived surface: title bar on dark chrome |
| `--grey` | `#474747` | Secondary text, metadata, captions |
| `--hair-light` | `rgba(36,36,36,.14)` | Hairlines on light |
| `--hair-dark` | `rgba(242,239,233,.09–.10)` | Hairlines on dark |

**Hard rules**

- One gold element per view. Gold is emphasis — if everything is gold, nothing is.
- No gradients, no new hues. Semantic states derive from the ladder above (same temperature, darker/lighter steps), never fresh colors.
- Hairlines never heavier than the token values. No borders + radius + accent-strip combos.
- The mark is never recolored outside the shipped colorways (light-bg, dark-bg, mono-black, mono-white, tile).
- "nola" always carries the gold; "kimi" always ink/cream. Never swap, never gold on gold.

## 3. Fonts

| Family | Weights | Role |
|---|---|---|
| Gentium Book Plus | 400, 700, italic 400 | Display, wordmark (shipped as outlines), note titles, headlines |
| Archivo | 400, 500, 600 | UI body, summaries, lists, buttons |
| IBM Plex Mono | 400, 500 | Metadata, timestamps, chips, labels — uppercase, tracked |

Type scale: display `clamp(28px, 4vw, 44px)` line-height 1.15–1.2; note titles 27px/1.2; body 13.5–15px/1.65; mono 9–11px, letter-spacing `.10–.16em`, always uppercase. Rules: display type never all-caps; italic reserved for the one phrase that carries the point (e.g. *takes notes*). All three are SIL OFL — safe for MIT distribution; the wordmark SVGs ship as outlined paths, so they render with zero font dependencies.

## 4. Surface inventory

The identity applies to these surfaces, in priority order:

1. **App icon / tile** — charcoal rounded square (`rx = 22%` of size), cream grain, one gold bar (bar index 4, the tallest). Source: `svg/kimi-nola-icon.svg`.
2. **Title bar** — `ink2` chrome, 16px icon + "Kimi Nola" in Archivo 400 12px at 75% cream; Windows caption glyphs right.
3. **System tray** — mono variants only (`mark-mono-white` on dark tray, `mark-mono-black` on light). Never the gold bar in the tray.
4. **Sidebar** — meeting list: Gentium 14.5px titles, mono 9px metadata (`TODAY · 09:30 · REC`), active row = `rgba(204,192,168,.14)` wash + 2px gold left edge.
5. **Note pane** — Gentium 27px title; mono meta line with recording dot; live waveform; Archivo summary; action-items table (hairline rows, mono owner column right-aligned).
6. **Empty states / about / README** — striation motif (§6) and the stamp variant.
7. **Docs site** — light-first; favicon system per §9.

## 5. Motion spec

| Element | Parameters |
|---|---|
| Recording dot | `1.6s ease-out infinite`; box-shadow ripple `0 0 0 0 → 0 0 0 9px`, gold at 50% → 0 opacity; keyframe stops at 70%/100% |
| Live waveform bars | `scaleY(.15) → scaleY(1)`, `ease-in-out infinite alternate`; per-bar duration `0.5 + (i mod 5) × 0.11s`, delay `i × 0.07s`; 12 bars, 5px wide, 3px radius, 5px gap |
| Buttons | hover invert (fill ↔ transparent), `0.3s` all |

**No other animation.** No scroll reveals, no hover scales, no marquees. The identity moves only when audio is live.

## 6. Logic to port

- **Striation rhythm** — the grain interior and the texture motif share one array of `(y, width)` pairs: `[(68,30),(84,56),(100,42),(116,80),(132,104),(148,62),(164,88),(180,46),(196,30)]` on a 256-unit grid, bar index 4 is gold. Port it as data, not magic numbers — empty states, README dividers, and any future mark variant reuse it. It is a feature (the brand's signature rhythm), not decoration.
- **Icon selection by context** — size < 24px → `favicon-16.png` hand-placed asset; 24–255px → nearest vector render; ≥ 256px or any print → SVG. Tray → mono variant by OS theme. This keeps the small sizes crisp by construction.

## 7. Placeholders & copy rules

| Placeholder in the sheet | Real content needed |
|---|---|
| Meeting titles/times in sidebar | Live data from the store |
| `REC 00:12:47 · 4 SPEAKERS` | Recording state; omit speakers when unknown, never show 0 |
| Summary / action items | Model output; action items always have an owner or "unassigned" |
| `OPEN SOURCE · MIT · 2026` (stamp) | Update year on major revs only |

Copy voice: specific numbers over adjectives ("4 speakers", not "several"); timestamps always absolute + relative ("today · 09:30"); failures get equal word count to wins; no exclamation marks in product copy.

## 8. Engineering notes

- **Tauri**: `branding/icons/tauri/` is a drop-in replacement for `src-tauri/icons/` (32x32, 128x128, 128x128@2x, icon.png, icon.ico). Do not run `tauri icon` afterward — it would regenerate rasters from the 512 and lose the hand-placed 16px inside the .ico. For MS Store `Square*Logo` assets, render from `icon.svg` at the required sizes with a `paper`-colored 10% margin.
- **Vector-only rule**: every logo surface starts from the SVGs; rasters are exports, never sources. The wordmark paths are font-independent outlines — do not re-type them.
- File budget: all SVGs < 5 KB; the identity sheet is one self-contained HTML file, CDN fonts only, no build step.
- Breakpoints: sidebar collapses to icons under 720px window width; note-pane max-width 520px holds at all sizes.
- Dark mode is a token swap (`paper ↔ ink`, `beige ↔ ink2`, muted gold stays, deep gold becomes muted gold). The tile never changes.

## 9. Favicon & icon system

Ships: `favicon.svg` (master, opaque charcoal tile — transparency reads broken on light browser chrome), `favicon-16.png` (**hand-placed pixel by pixel**, never downscaled — the striations dissolve), `favicon-32.png`, `apple-touch-icon-180.png` (opaque, square corners — iOS masks its own), `icon.ico` (entries 16 hand-placed + 24/32/48/256 vector renders), `icon-{32…1024}.png`. Safe swaps: 16px hand-placed ↔ nothing (it's the only approved sub-24px asset); all ≥24px sizes may be re-rendered from `icon.svg` if ever lost.

## 10. Definition of done

- [ ] App icon, tray icon, and taskbar all render the Oatwave mark; tray uses mono variant matching OS theme
- [ ] .ico contains the hand-placed 16px entry (verify by extracting, not by eye)
- [ ] Wordmark renders identically on a machine without Gentium installed (it is outlines)
- [ ] Exactly one gold element visible in any app view
- [ ] Recording state = pulsing dot + animated waveform, both per §5; nothing else animates
- [ ] No gradients and no off-palette hex values anywhere in the UI (grep the stylesheet)
- [ ] README header uses the stamp or primary lockup, linked from `branding/`, with clearspace per the motif tile
