// Decides what the page shows: the app for a paired device, or the screen
// that asks for access. Everything after that happens in Rust and chrome.js.
import * as chrome from "./chrome.js";

if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("/sw.js").catch(() => {});
}

async function main() {
  let status;
  try {
    const response = await fetch("/auth/status", { cache: "no-store", credentials: "same-origin" });
    status = await response.json();
  } catch (error) {
    chrome.showStatus(`Herdr UI is not reachable: ${error}`);
    return;
  }
  if (!status.paired) {
    chrome.showPairing(status);
    return;
  }
  chrome.listen();
  const wasm = await import("./herdr_web_client.js");
  await wasm.default();
  chrome.start(wasm, status);
}

main().catch((error) => chrome.showStatus(`Herdr UI could not start: ${error}`));
