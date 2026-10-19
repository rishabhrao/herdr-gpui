// The page around the app's canvas: the pairing screen, the keys bar for
// touch screens, pasting, and notes the app sends (links, copies, toasts).
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
      case "keys":
        prefix = note.key
          ? { key: note.key, control: note.control === "1", alt: note.alt === "1", shift: note.shift === "1", platform: note.platform === "1" }
          : null;
        break;
      case "device":
        // The app is still handling the message that asked: act after it.
        setTimeout(() => deviceAction(note.action), 0);
        break;
      case "disconnected":
        checkReach(Number(note.attempts || 0));
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

// --- Touch screens: the keys bar -------------------------------------------
//
// Two rows that fit any phone, as Termux and Termius lay theirs out: the
// keys a phone keyboard lacks, and the symbols a shell needs most. Swipe up
// on a key with a small mark for its second key. Arrows repeat while held.
// Ctrl, Alt, Shift, and Command hold for the next key, from here or the
// phone keyboard.

const sticky = { control: false, alt: false, shift: false, platform: false };
// Herdr's prefix as the app has it configured, from the app.
let prefix = null;
const SWIPE = 18;
const REPEAT_DELAY_MS = 380;
const REPEAT_MS = 55;

function stickyChanged(bar) {
  for (const button of bar.querySelectorAll("[data-sticky]")) {
    button.setAttribute("aria-pressed", String(sticky[button.dataset.sticky]));
  }
  app.set_sticky(sticky.control, sticky.alt, sticky.shift, sticky.platform);
}

function anySticky() {
  return sticky.control || sticky.alt || sticky.shift || sticky.platform;
}

function key(name) {
  app.send_key(name, sticky.control, sticky.alt, sticky.shift, sticky.platform);
}

// A key that also holds Control, as `ctrl` then the key would.
function chord(name) {
  app.send_key(name, true, false, false, false);
}

// A symbol types as text, unless a held modifier makes it a shortcut.
function symbol(text) {
  if (anySticky()) key(text);
  else app.send_text(text);
}

function sendPrefix() {
  if (prefix) app.send_key(prefix.key, prefix.control, prefix.alt, prefix.shift, prefix.platform);
}

async function paste() {
  try {
    // An image on the clipboard pastes as one, as on the Mac.
    if (navigator.clipboard.read) {
      for (const item of await navigator.clipboard.read()) {
        const type = item.types.find((kind) => kind.startsWith("image/"));
        if (type) {
          const blob = await item.getType(type);
          await pasteImage(new File([blob], "image", { type }));
          return;
        }
      }
    }
    const text = await navigator.clipboard.readText();
    if (text) app.paste(text);
  } catch {
    toast("This browser did not allow reading the clipboard.");
  }
}

// Chooses images from the phone's photos or files and pastes them, in the
// order chosen. The app prepares one at a time and holds the rest.
function pickImage() {
  const input = element("input", { type: "file", accept: "image/*", multiple: "" });
  input.addEventListener("change", async () => {
    for (const file of input.files || []) await pasteImage(file);
  });
  input.click();
}

// Whether the phone keyboard is up: it shrinks the visual viewport.
function keyboardUp() {
  const viewport = window.visualViewport;
  const focused = document.activeElement === $("herdr-input");
  return Boolean(focused && viewport && viewport.height < window.innerHeight * 0.8);
}

// Line-art icons for keys whose symbol many phone fonts lack. Static markup.
const ICONS = {
  keyboard:
    '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><rect x="2.5" y="6" width="19" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M6 14h.01M18 14h.01M9 14h6"/></svg>',
  paste:
    '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"><rect x="6" y="4" width="12" height="17" rx="2"/><path d="M9 4h6v3H9z"/></svg>',
};

// One key: `run` on a tap, `swipe` on a swipe up, and repeating while held
// when `repeat` is set.
function barKey({ label, title, run, swipe, swipeLabel, repeat, icon, stickyName, wide }) {
  const node = element("button", { title, "aria-label": title });
  if (icon) {
    node.innerHTML = ICONS[icon];
    node.classList.add("herdr-icon");
  } else {
    node.textContent = label;
  }
  if (swipeLabel) node.dataset.alt = swipeLabel;
  if (stickyName) node.dataset.sticky = stickyName;
  if (wide) node.classList.add("herdr-wide");
  let start = null;
  let timer = null;
  const stop = () => {
    clearTimeout(timer);
    clearInterval(timer);
    timer = null;
  };
  node.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    start = event.clientY;
    if (navigator.vibrate) navigator.vibrate(6);
    if (repeat) {
      run();
      timer = setTimeout(() => {
        timer = setInterval(run, REPEAT_MS);
      }, REPEAT_DELAY_MS);
    }
  });
  node.addEventListener("pointerup", (event) => {
    stop();
    if (start === null) return;
    const swiped = start - event.clientY > SWIPE;
    start = null;
    if (repeat) return;
    if (swiped && swipe) swipe();
    else run();
  });
  for (const name of ["pointercancel", "pointerleave"]) {
    node.addEventListener(name, () => {
      stop();
      start = null;
    });
  }
  return node;
}

function keysBar() {
  const bar = $("herdr-bar");
  const toggle = (name) => () => {
    sticky[name] = !sticky[name];
    stickyChanged(bar);
  };
  const prefixKey = barKey({ label: "pfx", title: "Herdr prefix", run: sendPrefix });
  prefixKey.hidden = !prefix;
  const rows = [
    [
      prefixKey,
      barKey({ label: "esc", title: "Escape (swipe up: Control-C)", run: () => key("escape"), swipe: () => chord("c"), swipeLabel: "^C" }),
      barKey({ label: "/", title: "Slash (swipe up: backslash)", run: () => symbol("/"), swipe: () => symbol("\\"), swipeLabel: "\\" }),
      barKey({ label: "-", title: "Dash (swipe up: underscore)", run: () => symbol("-"), swipe: () => symbol("_"), swipeLabel: "_" }),
      barKey({ label: "|", title: "Pipe (swipe up: ampersand)", run: () => symbol("|"), swipe: () => symbol("&"), swipeLabel: "&" }),
      barKey({ label: "home", title: "Home (swipe up: tilde)", run: () => key("home"), swipe: () => symbol("~"), swipeLabel: "~" }),
      barKey({ label: "↑", title: "Up", run: () => key("up"), repeat: true }),
      barKey({ label: "end", title: "End (swipe up: dollar)", run: () => key("end"), swipe: () => symbol("$"), swipeLabel: "$" }),
      barKey({ label: "pgup", title: "Page up", run: () => key("pageup"), repeat: true }),
      barKey({ icon: "keyboard", title: "Keyboard", run: () => app.keyboard(!keyboardUp()) }),
    ],
    [
      barKey({ label: "tab", title: "Tab (swipe up: Control-D)", run: () => key("tab"), swipe: () => chord("d"), swipeLabel: "^D" }),
      barKey({ label: "ctrl", title: "Control (next key)", run: toggle("control"), stickyName: "control" }),
      barKey({ label: "alt", title: "Option (next key)", run: toggle("alt"), stickyName: "alt" }),
      barKey({ label: "shift", title: "Shift (next key)", run: toggle("shift"), stickyName: "shift" }),
      barKey({ label: "⌘", title: "Command (next key)", run: toggle("platform"), stickyName: "platform" }),
      barKey({ label: "←", title: "Left", run: () => key("left"), repeat: true }),
      barKey({ label: "↓", title: "Down", run: () => key("down"), repeat: true }),
      barKey({ label: "→", title: "Right", run: () => key("right"), repeat: true }),
      barKey({ label: "pgdn", title: "Page down", run: () => key("pagedown"), repeat: true }),
      barKey({ icon: "paste", title: "Paste (swipe up: choose an image)", run: paste, swipe: pickImage, swipeLabel: "img" }),
    ],
  ];
  bar.replaceChildren(...rows.map((keys) => element("div", { class: "herdr-keys" }, keys)));
  // A tap on the bar must not take the keyboard away from the app.
  bar.addEventListener("pointerdown", (event) => event.preventDefault());
  window.addEventListener("herdr", (event) => {
    const note = event.detail || {};
    if (note.type === "sticky-used") {
      sticky.control = sticky.alt = sticky.shift = sticky.platform = false;
      stickyChanged(bar);
    }
    if (note.type === "keys") prefixKey.hidden = !prefix;
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

// Why the connection keeps failing, asked after a few tries. A sign-in to
// Cloudflare Access that ended sends every request to its login page, which
// only a reload shows; a device that was removed shows the pairing screen.
let checking = false;

async function checkReach(attempts) {
  if (checking || attempts < 2) return;
  checking = true;
  try {
    const response = await fetch("/auth/status", { cache: "no-store", credentials: "same-origin", redirect: "manual" });
    if (response.type === "opaqueredirect" || response.status === 401 || response.status === 403) {
      location.reload();
      return;
    }
    const kind = response.headers.get("content-type") || "";
    if (response.ok && !kind.includes("json")) {
      location.reload();
      return;
    }
    const status = response.ok ? await response.json() : null;
    if (status && !status.paired) location.reload();
  } catch {
    showStatus("Herdr UI is not reachable. Retrying\u2026");
  } finally {
    checking = false;
  }
}

// What Settings > Web access asks of this browser.
function deviceAction(action) {
  const zoom = (factor) => {
    const next = Math.min(2, Math.max(0.5, Math.round(app.zoom() * factor * 20) / 20));
    app.set_zoom(next);
    toast(`Text size ${Math.round(next * 100)}%`);
  };
  switch (action) {
    case "text-smaller":
      zoom(1 / 1.1);
      break;
    case "text-larger":
      zoom(1.1);
      break;
    case "text-reset":
      app.set_zoom(1);
      toast("Text size 100%");
      break;
    case "allow-notifications":
      if (!("Notification" in window)) {
        toast("This browser has no notifications.");
      } else if (Notification.permission === "granted") {
        toast("Notifications are on.");
      } else {
        // Browsers ask only after a tap on the page itself.
        toast("Show agent notifications on this device?", {
          label: "Allow",
          run: () => Notification.requestPermission().then((answer) => toast(`Notifications: ${answer}`)),
        });
      }
      break;
    case "hide-keyboard":
      app.keyboard(false);
      break;
    case "sign-out":
      if (appStatus && appStatus.local) {
        toast("A browser on this Mac needs no sign-in.");
      } else {
        post("/auth/sign-out", {}).finally(() => location.reload());
      }
      break;
  }
}

/// Image pastes: from the clipboard, a drop, or a phone keyboard's image.
const MAX_IMAGE = 15 * 1024 * 1024;

async function pasteImage(file) {
  if (file.size > MAX_IMAGE) {
    toast("That image is too large to paste.");
    return;
  }
  app.paste_image(file.type || "image/png", new Uint8Array(await file.arrayBuffer()));
}

function imageIn(data) {
  if (!data) return null;
  const item = [...(data.items || [])].find((entry) => entry.kind === "file" && entry.type.startsWith("image/"));
  return item ? item.getAsFile() : [...(data.files || [])].find((file) => file.type.startsWith("image/")) || null;
}

function listenForPastes() {
  const field = $("herdr-input");
  field.addEventListener("paste", (event) => {
    event.preventDefault();
    const image = imageIn(event.clipboardData);
    if (image) {
      pasteImage(image);
      return;
    }
    const text = event.clipboardData && event.clipboardData.getData("text/plain");
    if (text) app.paste(text);
  });
  field.addEventListener("beforeinput", (event) => {
    const image = imageIn(event.dataTransfer);
    if (image) {
      event.preventDefault();
      pasteImage(image);
    }
  });
  // Some keyboards insert an image element into rich text instead.
  new MutationObserver(() => {
    for (const image of field.querySelectorAll("img")) {
      const source = image.src;
      image.remove();
      fetch(source)
        .then((response) => response.blob())
        .then((blob) => pasteImage(new File([blob], "image", { type: blob.type || "image/png" })))
        .catch(() => toast("That image could not be pasted."));
    }
  }).observe(field, { childList: true, subtree: true });
}

export function start(wasm, started) {
  app = wasm;
  appStatus = started;
  listenForPastes();
  followViewport();
  // `?touch` shows the touch controls on any screen, such as a touch
  // laptop's, or a desktop browser trying the phone layout.
  const touch = new URLSearchParams(location.search).has("touch");
  if (touch || window.matchMedia("(pointer: coarse)").matches) keysBar();
  if (started && started.label) document.title = "Herdr UI";
}
