const names = new Intl.DisplayNames(["en"], { type: "region" });

/**
 * A country code's English name, via the platform's CLDR data: no map to
 * maintain, no dependency to add. Unknown codes, like the "??" of hits
 * without a resolved country, stay as they are.
 */
export function countryName(code: string): string {
  try {
    return names.of(code) ?? code;
  } catch {
    return code;
  }
}
