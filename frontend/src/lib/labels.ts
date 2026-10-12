import { countryName } from "./countries";

/**
 * The display names of the facet values the backend stores as lowercase
 * keys. The database holds `macos` and `chrome`; what a person reads is
 * decided here, so a rename never touches the data.
 *
 * A value with no label is shown as it is, and reported once in the
 * console: the backend stores a client or a platform it does not know under
 * the name it announces (`palemoon`, `cros`), and a missing entry here is
 * how someone notices and adds a label.
 */
const LABELS: Record<string, Record<string, string>> = {
  device: { desktop: "Desktop", mobile: "Mobile", tablet: "Tablet" },
  browser: {
    chrome: "Chrome",
    safari: "Safari",
    firefox: "Firefox",
    edge: "Edge",
    opera: "Opera",
    other: "Other",
  },
  os: {
    windows: "Windows",
    macos: "macOS",
    ios: "iOS",
    ipados: "iPadOS",
    android: "Android",
    linux: "Linux",
    other: "Other",
  },
};

/** The facets whose values the backend stores lowercase. */
const LOWERCASE = new Set(["country", "device", "browser", "os", "referrer"]);

/** A filter value as it is stored: lowercase for the lowercase facets. */
export function storedValue(facet: string, value: string): string {
  return LOWERCASE.has(facet) ? value.toLowerCase() : value;
}

const reported = new Set<string>();

/** What to show for a stored value of a facet; the value itself if unknown. */
export function facetLabel(facet: string, value: string): string {
  // Paths, referrers and click labels are shown as they are, and so is the
  // "??" of hits whose country could not be resolved.
  if (facet === "country" && value !== "??") return label(facet, value, countryName(value));
  const labels = LABELS[facet];
  return labels ? label(facet, value, labels[value]) : value;
}

function label(facet: string, value: string, found: string | undefined): string {
  if (found) return found;
  const key = `${facet}:${value}`;
  if (!reported.has(key)) {
    reported.add(key);
    console.warn(`No label for the ${facet} "${value}": showing it as it is.`);
  }
  return value;
}
