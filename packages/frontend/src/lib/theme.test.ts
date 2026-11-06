import { describe, expect, it } from "vitest";
import { DEFAULT_THEME, THEMES, accentInk, parseTheme, sameTheme, themeJson } from "./theme";

describe("parseTheme", () => {
  it("reads what themeJson writes", () => {
    for (const { theme } of THEMES) expect(parseTheme(themeJson(theme))).toEqual(theme);
  });

  it("falls back to the default for anything else", () => {
    expect(parseTheme(undefined)).toEqual(DEFAULT_THEME);
    expect(parseTheme("")).toEqual(DEFAULT_THEME);
    expect(parseTheme("not json")).toEqual(DEFAULT_THEME);
    expect(parseTheme('{"base":"sepia","accent":"#000000"}')).toEqual(DEFAULT_THEME);
    expect(parseTheme('{"base":"dark"}')).toEqual(DEFAULT_THEME);
  });
});

describe("accentInk", () => {
  it("puts white on a dark accent and dark text on a light one", () => {
    expect(accentInk("#0f172a")).toBe("#ffffff");
    expect(accentInk("#be123c")).toBe("#ffffff");
    expect(accentInk("#f59e0b")).toBe("#0f172a");
    expect(accentInk("#38bdf8")).toBe("#0f172a");
  });

  it("uses white for what is not a six-digit color", () => {
    expect(accentInk("#fff")).toBe("#ffffff");
  });
});

describe("sameTheme", () => {
  it("ignores the case of the accent, not the base", () => {
    expect(
      sameTheme({ base: "light", accent: "#1D4ED8" }, { base: "light", accent: "#1d4ed8" }),
    ).toBe(true);
    expect(
      sameTheme({ base: "dark", accent: "#1d4ed8" }, { base: "light", accent: "#1d4ed8" }),
    ).toBe(false);
  });
});

describe("THEMES", () => {
  it("has distinct names and themes", () => {
    expect(new Set(THEMES.map((t) => t.label)).size).toBe(THEMES.length);
    expect(new Set(THEMES.map((t) => themeJson(t.theme))).size).toBe(THEMES.length);
  });
});
