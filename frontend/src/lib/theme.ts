/**
 * A theme is two knobs: the base palette (light or dark) and one accent
 * color everything highlighted derives from. The presets of the settings
 * page are named pairs of the two; "custom" is the same pair with the
 * user's own accent.
 */
export type Theme = { base: "light" | "dark"; accent: string };

export const DEFAULT_THEME: Theme = { base: "light", accent: "#0f172a" };

export const THEMES: { label: string; theme: Theme }[] = [
  { label: "Daylight", theme: { base: "light", accent: "#0f172a" } },
  { label: "Nordic", theme: { base: "light", accent: "#1d4ed8" } },
  { label: "Forest", theme: { base: "light", accent: "#15803d" } },
  { label: "Rosewater", theme: { base: "light", accent: "#be123c" } },
  { label: "Midnight", theme: { base: "dark", accent: "#38bdf8" } },
  { label: "Ember", theme: { base: "dark", accent: "#f59e0b" } },
];

/** The stored theme, or the default when it does not parse. */
export function parseTheme(raw: string | undefined | null): Theme {
  try {
    const value = JSON.parse(raw ?? "") as Partial<Theme>;
    if ((value.base === "light" || value.base === "dark") && typeof value.accent === "string") {
      return { base: value.base, accent: value.accent };
    }
  } catch {
    // fall through to the default
  }
  return DEFAULT_THEME;
}

/** A theme as the settings endpoint stores it. */
export function themeJson(theme: Theme): string {
  return JSON.stringify(theme);
}

/**
 * The color to put on the accent: whichever of black and white contrasts
 * more with it, so a light accent (amber, sky) gets dark text.
 */
function accentInk(accent: string): string {
  const hex = accent.replace("#", "");
  if (hex.length !== 6) return "#ffffff";
  const channel = (i: number) => {
    const c = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const luminance = 0.2126 * channel(0) + 0.7152 * channel(1) + 0.0722 * channel(2);
  return luminance > 0.18 ? "#0f172a" : "#ffffff";
}

/** Apply a theme to the page: the base as data-theme, the accent inline. */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  root.dataset.theme = theme.base;
  root.style.setProperty("--accent", theme.accent);
  root.style.setProperty("--accent-ink", accentInk(theme.accent));
}
