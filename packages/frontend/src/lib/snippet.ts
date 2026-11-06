/** What the tracking snippet does: the options of its checkboxes. */
export type SnippetOptions = {
  /** Track clicks on elements with `data-potoo-click="label"`. */
  clicks: boolean;
  /** Track clicks on links to other sites. */
  outbound: boolean;
  /** Add the no-JavaScript pixel, in a `<noscript>`. */
  pixel: boolean;
};

/**
 * The HTML a site adds to its pages to be tracked by the Potoo at `origin`:
 * the script, with its options as attributes, and the pixel for browsers
 * without JavaScript.
 */
export function trackingSnippet(origin: string, hostname: string, options: SnippetOptions): string {
  const attributes = `${options.clicks ? " data-clicks" : ""}${options.outbound ? " data-outbound" : ""}`;
  const script = `<script async src="${origin}/tracker.js"${attributes}></script>`;
  const pixel = `<noscript><img src="${origin}/api/pixel?h=${hostname}" alt="" /></noscript>`;
  return options.pixel ? `${script}\n${pixel}` : script;
}
