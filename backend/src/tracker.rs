/// Tracker served at /tracker.js. Cookie-less; sends only host, path,
/// referrer and (for clicks) a label. Options are data attributes on the
/// tag:
///   data-outbound   track clicks on links to other sites
///   data-clicks     track clicks on elements with a data-potoo-click="label" attribute
///   data-host       override the reported hostname
pub const TRACKER_JS: &str = r#"(function () {
  var s = document.currentScript;
  if (!s) return;
  var endpoint = new URL("api/collect", s.src).href;
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
"#;

/// The transparent 1x1 GIF the no-JS fallback serves.
pub const GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x01, 0x44, 0x00, 0x3b,
];
