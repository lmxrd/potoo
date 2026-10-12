const names = new Intl.DisplayNames(["en"], { type: "region" });

/**
 * A country's English name from its code (stored lowercase, `no`), via the
 * platform's CLDR data: no map to maintain, no dependency to add. Undefined
 * for a code that is not one.
 */
export function countryName(code: string): string | undefined {
  try {
    return names.of(code.toUpperCase());
  } catch {
    return undefined;
  }
}
