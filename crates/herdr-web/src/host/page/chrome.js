// The page around the app's canvas: the pairing screen, the keys bar and
// menu for touch screens, and notes the app sends (links, copies, toasts).
// Text from the app or the network only ever goes in through textContent.

const $ = (id) => document.getElementById(id);
const PAIR_POLL_MS = 2000;

function element(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === "text") node.textContent = value;
    else if (key === "class") node.className = value;
    else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value);
  }
  for (const child of children) node.append(child);
  return node;
}

export function showStatus(text) {
  const status = $("herdr-status");
  status.textContent = text;
  status.hidden = !text;
}

function overlay(children) {
  const box = $("herdr-overlay");
  box.replaceChildren(element("div", { class: "herdr-card" }, children));
  box.hidden = false;
  showStatus("");
}

function closeOverlay() {
  const box = $("herdr-overlay");
  box.hidden = true;
  box.replaceChildren();
}

function toast(text, action) {
  const note = element("div", { class: "herdr-toast" }, [element("span", { text })]);
  if (action) {
    note.append(
      element("button", {
        text: action.label,
        onclick: () => {
          action.run();
          note.remove();
        },
      }),
    );
  }
  note.append(element("button", { class: "herdr-close", text: "✕", "aria-label": "Dismiss", onclick: () => note.remove() }));
  $("herdr-toasts").append(note);
  setTimeout(() => note.remove(), action ? 15000 : 5000);
}

// --- Pairing -----------------------------------------------------------

function guessLabel() {
  const agent = navigator.userAgent;
  const device = /iPhone/.test(agent)
    ? "iPhone"
    : /iPad/.test(agent)
      ? "iPad"
      : /Android/.test(agent)
        ? "Android"
        : /Mac/.test(agent)
          ? "Mac"
          : /Windows/.test(agent)
            ? "Windows PC"
            : /Linux/.test(agent)
              ? "Linux"
              : "Device";
  const browser = /Edg\//.test(agent)
    ? "Edge"
    : /Firefox\//.test(agent)
      ? "Firefox"
      : /Chrome\//.test(agent) || /CriOS\//.test(agent)
        ? "Chrome"
        : /Safari\//.test(agent)
          ? "Safari"
          : "browser";
  return `${device} ${browser}`;
}

async function post(path, body) {
  const response = await fetch(path, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const reply = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(reply.error || `HTTP ${response.status}`);
  return reply;
}

export function showPairing(status) {
  const name = element("input", { type: "text", value: guessLabel(), maxlength: "48", "aria-label": "Device name" });
  const ask = element("button", { class: "herdr-primary", text: "Ask for access" });
  const local = status.local
    ? [element("p", { class: "herdr-hint", text: "On this Mac, opening the address `herdr-gpui serve` printed (with ?token=) also works." })]
    : [];
  overlay([
    element("h1", { text: "Use Herdr UI on this device" }),
    element("p", { text: "Herdr UI runs on your Mac. Someone already using it approves each new device." }),
    element("label", { class: "herdr-field" }, [element("span", { text: "Device name" }), name]),
    ask,
    ...local,
  ]);
  ask.addEventListener("click", async () => {
    ask.disabled = true;
    try {
      const reply = await post("/pair/request", { label: name.value });
      waitForApproval(reply);
    } catch (error) {
      ask.disabled = false;
      toast(String(error.message || error));
    }
  });
}

function waitForApproval(reply) {
  let stopped = false;
  overlay([
    element("h1", { text: "Approve this device" }),
    element("p", { text: "Your code:" }),
    element("div", { class: "herdr-code", text: reply.code }),
    element("p", {
      text: "Open Herdr UI where you already use it, such as the Mac's browser at http://localhost:1234. It asks to allow this device. Check that the codes match, then choose Allow.",
    }),
    element("p", { class: "herdr-hint", text: "This page continues by itself. The code works for 10 minutes." }),
    element("button", {
      text: "Cancel",
      onclick: () => {
        stopped = true;
        location.reload();
      },
    }),
  ]);
  const poll = async () => {
    if (stopped) return;
    let state = "waiting";
    try {
      const response = await fetch(`/pair/wait?id=${encodeURIComponent(reply.id)}`, { cache: "no-store", credentials: "same-origin" });
      state = (await response.json()).state;
    } catch {
      // A dropped request is tried again.
    }
    if (state === "approved") {
      location.reload();
      return;
    }
    if (state === "denied" || state === "gone") {
      overlay([
        element("h1", { text: state === "denied" ? "Access was denied" : "The request ran out of time" }),
        element("button", { class: "herdr-primary", text: "Ask again", onclick: () => location.reload() }),
      ]);
      return;
    }
    setTimeout(poll, PAIR_POLL_MS);
  };
  setTimeout(poll, PAIR_POLL_MS);
}

// --- The app's notes ----------------------------------------------------

let app;
// What `/auth/status` said when the app started: whether this browser is
// on this machine.
let appStatus;
// The app's open questions, oldest first; the page shows one at a time.
let prompts = [];
let shownPrompt = null;

function showPrompt() {
  if (shownPrompt !== null || prompts.length === 0) return;
  const prompt = prompts[0];
  shownPrompt = prompt.id;
  const answers = prompt.answers ? prompt.answers.split("\u001f") : ["OK"];
  const answer = (index) => {
    if (app) app.prompt_answer(Number(prompt.id), index);
    prompts = prompts.filter((open) => open.id !== prompt.id);
    shownPrompt = null;
    closeOverlay();
    showPrompt();
  };
  overlay([
    element("h1", { text: prompt.message }),
    ...(prompt.detail ? [element("p", { class: "herdr-prompt-detail", text: prompt.detail })] : []),
    ...answers.map((label, index) =>
      element("button", { class: index === 0 ? "herdr-primary" : "", text: label, onclick: () => answer(index) }),
    ),
  ]);
}

export function listen() {
  window.addEventListener("herdr", (event) => {
    const note = event.detail || {};
    switch (note.type) {
      case "toast":
        toast(note.text);
        break;
      case "open-url":
        // A page may only open a window from a tap, so the link waits for one.
        toast(`Open ${note.url}`, { label: "Open", run: () => window.open(note.url, "_blank", "noopener") });
        break;
      case "clipboard":
        if (navigator.clipboard && navigator.clipboard.writeText) {
          navigator.clipboard.writeText(note.text).catch(() =>
            toast("Herdr UI copied text.", { label: "Copy", run: () => navigator.clipboard.writeText(note.text) }),
          );
        }
        break;
      case "notify":
        if ("Notification" in window && Notification.permission === "granted" && document.hidden) {
          new Notification(note.title, { body: note.body, icon: "/icon-192.png" });
        } else {
          toast(note.body ? `${note.title}: ${note.body}` : note.title);
        }
        break;
      case "protocol":
        overlay([
          element("h1", { text: "Herdr UI was updated" }),
          element("button", { class: "herdr-primary", text: "Reload", onclick: () => location.reload() }),
        ]);
        break;
      case "signed-out":
        location.reload();
        break;
      case "disconnected":
        fetch("/auth/status", { cache: "no-store" })
          .then((response) => (response.ok ? response.json() : null))
          .then((status) => {
            if (status && !status.paired) location.reload();
          })
          .catch(() => {});
        break;
      case "prompt":
        prompts.push(note);
        showPrompt();
        break;
      case "prompt-closed":
        prompts = prompts.filter((prompt) => prompt.id !== note.id);
        if (shownPrompt === note.id) {
          shownPrompt = null;
          closeOverlay();
          showPrompt();
        }
        break;
      default:
        break;
    }
  });
}

// --- Touch screens: the keys bar and menu ---------------------------------

const sticky = { control: false, alt: false, platform: false };

function stickyChanged(bar) {
  for (const button of bar.querySelectorAll("[data-sticky]")) {
    button.setAttribute("aria-pressed", String(sticky[button.dataset.sticky]));
  }
  app.set_sticky(sticky.control, sticky.alt, sticky.platform);
}

function key(name) {
  app.send_key(name, sticky.control, sticky.alt, false, sticky.platform);
}

async function paste() {
  try {
    const text = await navigator.clipboard.readText();
    app.paste(text);
  } catch {
    toast("This browser did not allow reading the clipboard.");
  }
}

async function devices() {
  let list = [];
  try {
    const response = await fetch("/auth/devices", { cache: "no-store", credentials: "same-origin" });
    list = (await response.json()).devices || [];
  } catch (error) {
    toast(String(error));
    return;
  }
  const seen = (seconds) => {
    const minutes = Math.floor((Date.now() / 1000 - seconds) / 60);
    if (minutes < 1) return "seen just now";
    if (minutes < 60) return `seen ${minutes} min ago`;
    if (minutes < 1440) return `seen ${Math.floor(minutes / 60)} h ago`;
    return `seen ${Math.floor(minutes / 1440)} days ago`;
  };
  const rows = list.map((device) => {
    // A second tap forgets: one stray tap must not lock a device out.
    let armed = false;
    const forget = element("button", {
      text: "Forget",
      onclick: async () => {
        if (!armed) {
          armed = true;
          forget.textContent = "Tap to forget";
          forget.classList.add("herdr-danger");
          return;
        }
        await post("/auth/forget", { id: device.id }).catch((error) => toast(String(error.message || error)));
        if (device.current) location.reload();
        else devices();
      },
    });
    return element("li", {}, [
      element("div", { class: "herdr-device" }, [
        element("span", { text: device.current ? `${device.label} (this device)` : device.label }),
        element("span", {
          class: "herdr-hint",
          text: `${device.remote ? "Through the tunnel" : "On this Mac"} \u00b7 ${seen(device.lastSeen)}`,
        }),
      ]),
      forget,
    ]);
  });
  if (rows.length === 0) rows.push(element("li", { class: "herdr-hint", text: "No device is paired." }));
  overlay([
    element("h1", { text: "Paired devices" }),
    element("ul", { class: "herdr-devices" }, rows),
    element("button", { text: "Close", onclick: closeOverlay }),
  ]);
}

function menu() {
  const zoom = (factor) => {
    const next = Math.min(2, Math.max(0.5, Math.round(app.zoom() * factor * 20) / 20));
    app.set_zoom(next);
    toast(`Text size ${Math.round(next * 100)}%`);
  };
  const remote = !(appStatus && appStatus.local);
  overlay([
    element("h1", { text: "Herdr UI" }),
    element("p", { class: "herdr-hint", text: "Text size on this device" }),
    element("div", { class: "herdr-row" }, [
      element("button", { text: "A−", "aria-label": "Smaller", onclick: () => zoom(1 / 1.1) }),
      element("button", { text: "100%", "aria-label": "Reset", onclick: () => { app.set_zoom(1); toast("Text size 100%"); } }),
      element("button", { text: "A+", "aria-label": "Larger", onclick: () => zoom(1.1) }),
    ]),
    element("button", {
      text: "Allow notifications",
      onclick: () => "Notification" in window && Notification.requestPermission().then((answer) => toast(`Notifications: ${answer}`)),
    }),
    element("button", { text: "Paired devices", onclick: devices }),
    ...(remote
      ? [element("button", { text: "Sign out this device", onclick: () => post("/auth/sign-out", {}).finally(() => location.reload()) })]
      : []),
    element("button", { class: "herdr-primary", text: "Close", onclick: closeOverlay }),
  ]);
}

// A key that also holds Control, as `ctrl` then the key would.
function chord(name) {
  app.send_key(name, true, false, false, false);
}

// Line-art icons for keys whose symbol many phone fonts lack. Static markup.
const ICONS = {
  keyboard:
    '<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><rect x="2.5" y="6" width="19" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M6 14h.01M18 14h.01M9 14h6"/></svg>',
  more:
    '<svg viewBox="0 0 24 24" width="22" height="22" fill="currentColor"><circle cx="5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="19" cy="12" r="1.8"/></svg>',
};

function keysBar() {
  const bar = $("herdr-bar");
  const button = (label, title, run, extra = {}) =>
    element("button", { text: label, title, "aria-label": title, onclick: run, ...extra });
  const icon = (name, title, run) => {
    const node = button("", title, run);
    node.innerHTML = ICONS[name];
    node.classList.add("herdr-icon");
    return node;
  };
  let keyboard = false;
  // The keys scroll; the keyboard and the menu stay in reach at the end.
  const keys = element("div", { class: "herdr-keys" }, [
    button("esc", "Escape", () => key("escape")),
    button("tab", "Tab", () => key("tab")),
    button("ctrl", "Control (next key)", () => { sticky.control = !sticky.control; stickyChanged(bar); }, { "data-sticky": "control" }),
    button("^C", "Interrupt (Control-C)", () => chord("c")),
    button("←", "Left", () => key("left")),
    button("↑", "Up", () => key("up")),
    button("↓", "Down", () => key("down")),
    button("→", "Right", () => key("right")),
    button("⏎", "Return", () => key("enter")),
    button("alt", "Option (next key)", () => { sticky.alt = !sticky.alt; stickyChanged(bar); }, { "data-sticky": "alt" }),
    button("⌘", "Command (next key)", () => { sticky.platform = !sticky.platform; stickyChanged(bar); }, { "data-sticky": "platform" }),
    button("^D", "End of input (Control-D)", () => chord("d")),
    button("paste", "Paste", paste),
  ]);
  const pinned = element("div", { class: "herdr-pinned" }, [
    icon("keyboard", "Keyboard", () => {
      keyboard = !keyboard;
      app.keyboard(keyboard);
    }),
    icon("more", "Menu", menu),
  ]);
  bar.replaceChildren(keys, pinned);
  // A tap on the bar must not take the keyboard away from the app.
  bar.addEventListener("pointerdown", (event) => event.preventDefault());
  window.addEventListener("herdr", (event) => {
    if (event.detail && event.detail.type === "sticky-used") {
      sticky.control = sticky.alt = sticky.platform = false;
      stickyChanged(bar);
    }
  });
  stickyChanged(bar);
  bar.hidden = false;
}

function followViewport() {
  const viewport = window.visualViewport;
  if (!viewport) return;
  const apply = () => {
    document.documentElement.style.setProperty("--app-height", `${Math.round(viewport.height)}px`);
    window.scrollTo(0, 0);
  };
  viewport.addEventListener("resize", apply);
  viewport.addEventListener("scroll", apply);
  apply();
}

export function start(wasm, started) {
  app = wasm;
  appStatus = started;
  followViewport();
  // `?touch` shows the touch controls on any screen, such as a touch
  // laptop's, or a desktop browser trying the phone layout.
  const touch = new URLSearchParams(location.search).has("touch");
  if (touch || window.matchMedia("(pointer: coarse)").matches) keysBar();
  if (started && started.label) document.title = "Herdr UI";
}
