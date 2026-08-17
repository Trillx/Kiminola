# Kimi Nola UI components

These are **shadcn-svelte** components ("nova" style, Tailwind CSS v4). They live in the repo so we can customize them and keep the Oatwave brand locked in.

## Adding a component

```bash
npx shadcn-svelte@latest add -y <component-name>
```

## Theming

`src/app.css` is the source of truth. The shadcn CSS variables (`--background`, `--primary`, `--ring`, etc.) are mapped to the Oatwave tokens (`--canvas`, `--ink`, `--brand`, etc.) in both `:root` and `[data-theme="dark"]`. Components should continue to look on-brand in light and dark mode without extra work.

## Brand rules

- One gold element per view. Gold (`--brand` / `--brand-deep`) is emphasis, not the default.
- Prefer `variant="default"` for primary actions (ink fill) and `variant="outline"` for secondary actions.
- Keep hairlines at `--border` / `--hairline`; never add heavier borders.
- No gradients, no off-palette hues.

## Existing custom components

`$lib/components/` still contains older bespoke components (Sidebar, Topbar, Select, etc.). Migrate them to shadcn primitives when you touch them; don't rewrite everything at once.
