export const SETTINGS_SECTIONS = [
  { id: "general", label: "General" },
  { id: "models", label: "Speech model" },
  { id: "ai", label: "AI provider" },
  { id: "shortcut", label: "Shortcut" },
  { id: "templates", label: "Templates" },
  { id: "about", label: "About" },
] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number]["id"];

export type ProviderFormConfig = {
  kind: string;
  base_url: string;
  model: string;
  has_api_key?: boolean;
};

export function resolveSettingsSection(value: string | null): SettingsSection {
  return SETTINGS_SECTIONS.some((section) => section.id === value)
    ? (value as SettingsSection)
    : "general";
}

export function settingsSectionHref(section: SettingsSection): string {
  return section === "general" ? "/settings" : `/settings?section=${section}`;
}

export function nextSettingsSection(
  current: SettingsSection,
  key: string,
): SettingsSection | null {
  if (key === "Home") return SETTINGS_SECTIONS[0].id;
  if (key === "End") return SETTINGS_SECTIONS[SETTINGS_SECTIONS.length - 1].id;
  if (key !== "ArrowRight" && key !== "ArrowDown" && key !== "ArrowLeft" && key !== "ArrowUp") {
    return null;
  }
  const direction = key === "ArrowRight" || key === "ArrowDown" ? 1 : -1;
  const currentIndex = SETTINGS_SECTIONS.findIndex((section) => section.id === current);
  const nextIndex = (currentIndex + direction + SETTINGS_SECTIONS.length) % SETTINGS_SECTIONS.length;
  return SETTINGS_SECTIONS[nextIndex].id;
}

export function shouldUseFocusedSettingsShell(pathname: string): boolean {
  return pathname.replace(/\/+$/, "") === "/settings";
}

export function isProviderConfigDirty(
  saved: ProviderFormConfig | null,
  current: ProviderFormConfig,
  replacementApiKey: string,
): boolean {
  if (replacementApiKey.trim() !== "" || !saved) return true;
  return (
    saved.kind !== current.kind ||
    saved.base_url !== current.base_url ||
    saved.model !== current.model
  );
}

export function providerIsConfigured(config: ProviderFormConfig): boolean {
  if (config.base_url.trim() === "" || config.model.trim() === "") return false;
  if (config.kind === "ollama" || config.kind === "lm_studio") return true;
  return config.has_api_key === true;
}

export function templateNeedsDeleteConfirmation(template: {
  id: number;
  is_builtin: number;
}): boolean {
  return template.id !== -1 && template.is_builtin === 0;
}
