// Product analytics on PostHog: who comes, whether the game loads for them, how far they get and whether they come
// back. Only a handful of events a game, never one a frame. PostHog's own script (about 100 KB) is only fetched
// once the game is on screen, or can't be, so it never slows the wasm down; what's tracked before then waits here.
// Nothing is sent from localhost or with the dev flags (?dev, ?demo, ?showcase), or while KEY is empty.

// the project's API key (phc_...), from PostHog's project settings: public, it can only send events
const KEY = "phc_qaCzPNqDeK3wNazof5as6qxqLCBxpweu9Gu2r7xjqdNh";
// where events go: PostHog's managed reverse proxy to its US cloud (where the project is), on our own subdomain so
// ad blockers leave it be (a CNAME at Cloudflare, DNS only)
const HOST = "https://t.readyplayerz.com";

const params = new URLSearchParams(location.search);
const on = KEY !== ""
  && !["localhost", "127.0.0.1"].includes(location.hostname)
  && !["dev", "demo", "showcase"].some(f => params.has(f));

// what's tracked before PostHog is loaded, each with when it happened
const waiting = [];
let started = false;

/** Tracks `event`, with `props`. */
export function track(event, props = {}) {
  if (!on) return;
  if (started) window.posthog.capture(event, props);
  else waiting.push([event, props, new Date()]);
}

/** Properties sent with every event from now on. */
export function remember(props) {
  if (!on) return;
  if (started) window.posthog.register(props);
  else waiting.push([null, props]);
}

/** Fetches PostHog and sends what's waiting. Only the first call does anything. */
export function startAnalytics() {
  if (!on || started) return;
  started = true;
  // PostHog's loader: a stand-in that queues calls until the script has come
  !function(t,e){var o,n,p,r;e.__SV||(window.posthog=e,e._i=[],e.init=function(i,s,a){function g(t,e){var o=e.split(".");2==o.length&&(t=t[o[0]],e=o[1]),t[e]=function(){t.push([e].concat(Array.prototype.slice.call(arguments,0)))}}(p=t.createElement("script")).type="text/javascript",p.crossOrigin="anonymous",p.async=!0,p.src=s.api_host.replace(".i.posthog.com","-assets.i.posthog.com")+"/static/array.js",(r=t.getElementsByTagName("script")[0]).parentNode.insertBefore(p,r);var u=e;for(void 0!==a?u=e[a]=[]:a="posthog",u.people=u.people||[],u.toString=function(t){var e="posthog";return"posthog"!==a&&(e+="."+a),t||(e+=" (stub)"),e},u.people.toString=function(){return u.toString(1)+".people (stub)"},o="capture register register_once unregister opt_out_capturing opt_in_capturing has_opted_out_capturing identify reset".split(" "),n=0;n<o.length;n++)g(u,o[n]);e._i.push([i,s,a])},e.__SV=1)}(document,window.posthog||[]);
  window.posthog.init(KEY, {
    api_host: HOST,
    // PostHog's own site, for links back to it when events go through a proxy
    ui_host: "https://us.posthog.com",
    defaults: "2026-05-30",
    // no accounts: everyone is an anonymous visitor, which retention and funnels count just the same
    person_profiles: "identified_only",
    // the visitor's id in localStorage, not a cookie
    persistence: "localStorage",
    // a click in play is a shot, so clicks aren't tracked by themselves: only the events below
    autocapture: false,
    capture_dead_clicks: false,
    rageclick: false,
    capture_heatmaps: false,
    // recording the page would watch the HUD, which changes every frame
    disable_session_recording: true,
    capture_performance: false,
    disable_surveys: true,
    advanced_disable_feature_flags: true,
    capture_pageview: true,
    capture_pageleave: true,
  });
  for (const [event, props, at] of waiting) {
    if (event) window.posthog.capture(event, props, { timestamp: at });
    else window.posthog.register(props);
  }
  waiting.length = 0;
}

// errors the page didn't catch (a wasm panic among them), the first few a visit
let errors = 0;
const error = (message, where) => {
  if (errors++ < 5) track("error", { message: String(message).slice(0, 500), where });
};
addEventListener("error", e => error(e.message, e.filename ? `${e.filename}:${e.lineno}` : ""));
addEventListener("unhandledrejection", e => error(e.reason?.message || e.reason, "promise"));
