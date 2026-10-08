/**
 * Tracker served at /api/tracker.js. Cookie-less; sends only host, path,
 * referrer and (for clicks) a label. Options are data attributes on the tag:
 *   data-outbound   track clicks on links to other sites
 *   data-clicks     track clicks on elements with a data-potoo-click="label" attribute
 *   data-host       override the reported hostname
 */
export const TRACKER_JS = `(function () {
  var s = document.currentScript;
  if (!s) return;
  var endpoint = new URL("collect", s.src.replace(/tracker\\.js.*$/, "")).href;
  var host = s.getAttribute("data-host") || location.host;
  function send(type, extra) {
    var p = { type: type, host: host, path: location.pathname };
    for (var k in extra) p[k] = extra[k];
    var body = JSON.stringify(p);
    if (!(navigator.sendBeacon && navigator.sendBeacon(endpoint, body))) {
      fetch(endpoint, { method: "POST", body: body, keepalive: true, headers: { "content-type": "text/plain" } });
    }
  }
  var last;
  function pageview() {
    if (last === location.pathname) return;
    var ref = last ? location.origin + last : document.referrer;
    last = location.pathname;
    send("pageview", { referrer: ref });
  }
  var push = history.pushState;
  history.pushState = function () { push.apply(this, arguments); pageview(); };
  addEventListener("popstate", pageview);
  pageview();
  var outbound = s.hasAttribute("data-outbound");
  var clicks = s.hasAttribute("data-clicks");
  if (outbound || clicks) {
    addEventListener("click", function (e) {
      var el = e.target && e.target.closest && e.target.closest("[data-potoo-click], a[href]");
      if (!el) return;
      var label = el.getAttribute("data-potoo-click");
      if (label && clicks) return send("click", { name: label });
      if (outbound && el.href && el.host && el.host !== location.host) {
        send("click", { name: "outbound:" + el.host + el.pathname });
      }
    }, true);
  }
})();
`;
