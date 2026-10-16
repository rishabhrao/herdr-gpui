// Keeps the page's own files for a quick start and an install prompt. The
// network answers first; a copy serves only when it cannot. Pairing, the
// socket, and tab pages always go to the network.
const CACHE = "herdr-shell-v2";
const SHELL = new Set([
  "/",
  "/boot.js",
  "/chrome.js",
  "/style.css",
  "/manifest.webmanifest",
  "/icon-192.png",
  "/icon-512.png",
  "/apple-touch-icon.png",
  "/herdr_web_client.js",
  "/herdr_web_client_bg.wasm",
]);

self.addEventListener("install", () => self.skipWaiting());

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const name of await caches.keys()) {
        if (name !== CACHE) await caches.delete(name);
      }
      await self.clients.claim();
    })(),
  );
});

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (event.request.method !== "GET" || url.origin !== self.location.origin) return;
  if (!SHELL.has(url.pathname) || url.search) return;
  event.respondWith(
    (async () => {
      const cache = await caches.open(CACHE);
      try {
        const response = await fetch(event.request);
        if (response.ok && response.type === "basic") {
          await cache.put(event.request, response.clone());
        }
        return response;
      } catch (error) {
        const kept = await cache.match(event.request);
        if (kept) return kept;
        throw error;
      }
    })(),
  );
});
