export const SETTINGS_SECTIONS = ["general", "models", "ai", "shortcut", "templates", "about"] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

export type ProviderFormConfig = {
  kind: string;
  base_url: string;
  model: string;
};

export function resolveSettingsSection(value: string | null): SettingsSection {
  return SETTINGS_SECTIONS.includes(value as SettingsSection) ? (value as SettingsSection) : "general";
}

export function settingsSectionHref(section: SettingsSection): string {
  return section === "general" ? "/settings" : `/settings?section=${section}`;
}

export function nextSettingsSection(
  current: SettingsSection,
  key: string,
): SettingsSection | null {
  if (key === "Home") return SETTINGS_SECTIONS[0];
  if (key === "End") return SETTINGS_SECTIONS[SETTINGS_SECTIONS.length - 1];
  if (key !== "ArrowRight" && key !== "ArrowDown" && key !== "ArrowLeft" && key !== "ArrowUp") {
    return null;
  }
  const direction = key === "ArrowRight" || key === "ArrowDown" ? 1 : -1;
  const currentIndex = SETTINGS_SECTIONS.indexOf(current);
  const nextIndex = (currentIndex + direction + SETTINGS_SECTIONS.length) % SETTINGS_SECTIONS.length;
  return SETTINGS_SECTIONS[nextIndex];
}

export function shouldUseFocusedSettingsShell(pathname: string): boolean {
  return pathname.replace(/\/+$/, "") === "/settings";
}

export function isProviderConfigDirty(
  saved: ProviderFormConfig | null,
  current: ProviderFormConfig,
  apiKeyTouched: boolean,
): boolean {
  if (apiKeyTouched || !saved) return true;
  return (
    saved.kind !== current.kind ||
    saved.base_url !== current.base_url ||
    saved.model !== current.model
  );
}

export function templateNeedsDeleteConfirmation(template: {
  id: number;
  is_builtin: number;
}): boolean {
  return template.id !== -1 && template.is_builtin === 0;
}
