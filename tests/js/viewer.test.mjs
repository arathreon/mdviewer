// Tests for assets/viewer.js. Run: node --test tests/js/viewer.test.mjs
// (`cargo test` runs them too, see tests/page_script.rs).
//
// Node has no DOM, so the script runs in a `vm` context with a small fake `document`
// that has only the parts the script uses. No npm packages are necessary.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const SOURCE = readFileSync(new URL("../../assets/viewer.js", import.meta.url), "utf8");
const PAGE_ORIGIN = "https://mdviewer.localhost";

/// Runs the script once and gives the tools to control and observe it.
function load({
  origin = PAGE_ORIGIN,
  embeds = [],
  themeSelect = null,
  imagesSelect = null,
  dialectSelect = null,
  openButton = null,
  backButton = null,
  forwardButton = null,
  panelButton = null,
  panel = null,
  panelState = "hide",
  scroll = "0",
  panelWidth = "260px",
  resizer = null,
  innerWidth = 1100,
} = {}) {
  const listeners = new Map();
  const windowListeners = new Map();
  const sent = [];
  const queries = [];
  const timers = [];
  const bodyChildren = [];
  const document = {
    body: {
      append(child) {
        bodyChildren.push(child);
        child.remove = () => bodyChildren.splice(bodyChildren.indexOf(child), 1);
      },
    },
    querySelector(selector) {
      return selector === ".mdviewer-notice"
        ? (bodyChildren.find((child) => child.className === "mdviewer-notice") ?? null)
        : null;
    },
    documentElement: {
      dataset: { theme: "auto", panel: panelState, scroll },
      // The inline style of <html>: only the CSS variables.
      style: {
        properties: { "--panel-width": panelWidth },
        getPropertyValue(name) {
          return this.properties[name] ?? "";
        },
        setProperty(name, value) {
          this.properties[name] = value;
        },
      },
    },
    addEventListener(type, listener) {
      listeners.set(type, [...(listeners.get(type) ?? []), listener]);
    },
    querySelectorAll(selector) {
      queries.push(selector);
      return embeds;
    },
    getElementById(id) {
      const elements = {
        "mdviewer-theme": themeSelect,
        "mdviewer-images": imagesSelect,
        "mdviewer-dialect": dialectSelect,
        "mdviewer-open": openButton,
        "mdviewer-back": backButton,
        "mdviewer-forward": forwardButton,
        "mdviewer-panel-button": panelButton,
        "mdviewer-history": panel,
        "mdviewer-resizer": resizer,
      };
      return elements[id] ?? null;
    },
    createElement(tag) {
      const attributes = {};
      return {
        tagName: tag,
        className: "",
        textContent: "",
        attributes,
        setAttribute: (name, value) => (attributes[name] = value),
      };
    },
  };
  // The page scrolls at most to `maxScroll` (the page height), as in a browser.
  const window = {
    ipc: { postMessage: (message) => sent.push(message) },
    scrollY: 0,
    innerWidth,
    maxScroll: Infinity,
    scrolls: [],
    scrollTo(x, y) {
      window.scrolls.push(y);
      window.scrollY = Math.min(y, window.maxScroll);
    },
    addEventListener(type, listener, options) {
      windowListeners.set(type, [...(windowListeners.get(type) ?? []), { listener, options }]);
    },
  };
  const context = {
    document,
    window,
    location: { origin },
    requestAnimationFrame: (callback) => callback(),
    // A timer id is its place in `timers` plus one (0 is not a valid id).
    setTimeout: (callback, delay) => timers.push({ callback, delay, cleared: false }),
    clearTimeout: (id) => {
      if (id) {
        timers[id - 1].cleared = true;
      }
    },
  };
  // As in WebView2, the script starts before the page exists: no <html> element yet.
  const root = document.documentElement;
  document.documentElement = null;
  vm.runInNewContext(SOURCE, context);
  document.documentElement = root;
  const fire = (type, event = {}) => {
    for (const listener of listeners.get(type) ?? []) {
      listener(event);
    }
  };
  const fireWindow = (type, event = {}) => {
    for (const { listener } of windowListeners.get(type) ?? []) {
      listener(event);
    }
  };
  // Runs the timers that are not cleared and did not run yet, as the browser would.
  const runTimers = () => {
    for (const timer of timers) {
      if (!timer.cleared && !timer.done) {
        timer.done = true;
        timer.callback();
      }
    }
  };
  return {
    fire,
    fireWindow,
    runTimers,
    sent,
    queries,
    listeners,
    windowListeners,
    document,
    window,
    timers,
    bodyChildren,
  };
}

/// A fake element for `closest`, `parentElement`, `classList` and `dataset`.
function element({ classes = [], dataset = {}, parent = null, attributes = {} } = {}) {
  const self = {
    parentElement: parent,
    dataset,
    classList: { contains: (name) => classes.includes(name) },
    getAttribute: (name) => attributes[name] ?? null,
    replacedWith: null,
    replaceWith(other) {
      self.replacedWith = other;
    },
    setAttribute: (name, value) => (attributes[name] = String(value)),
    blurred: false,
    blur() {
      self.blurred = true;
    },
  };
  return self;
}

/// A click target inside `title` (or outside any title when `title` is null).
const clickInside = (title) => ({
  target: { closest: (selector) => (selector === ".callout-title" ? title : null) },
});

const escape = (modifiers = {}) => ({
  key: "Escape",
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...modifiers,
});

test("does nothing on other origins", () => {
  const page = load({ origin: "https://example.com" });
  assert.equal(page.listeners.size, 0);
  assert.equal(page.window.mdviewerNotice, undefined);
});

/// A click (or other mouse event) on a fake link.
function linkEvent(href, { absolute = `https://mdviewer.localhost/docs/${href}`, wiki = false, button = 0 } = {}) {
  const link = {
    getAttribute: (name) => (name === "href" ? href : null),
    href: absolute,
    dataset: wiki ? { wikilink: "true" } : {},
  };
  const event = {
    button,
    prevented: false,
    target: { closest: (selector) => (selector === "a[href]" ? link : null) },
  };
  event.preventDefault = () => {
    event.prevented = true;
  };
  return event;
}

test("a link click goes to Rust with the absolute address", () => {
  const page = load();
  const event = linkEvent("other.md#part");
  page.fire("click", event);
  assert.deepEqual(page.sent, ["link:https://mdviewer.localhost/docs/other.md#part"]);
  assert.equal(event.prevented, true);
});

test("links to a place in the same page stay in the page", () => {
  const page = load();
  const event = linkEvent("#heading");
  page.fire("click", event);
  assert.deepEqual(page.sent, []);
  assert.equal(event.prevented, false);
});

test("an unresolved wiki link sends its decoded target", () => {
  const page = load();
  page.fire("click", linkEvent("Other%20note#My%20Part", { wiki: true }));
  page.fire("click", linkEvent("100%", { wiki: true }));
  assert.deepEqual(page.sent, ["wikilink:Other note#My Part", "wikilink:100%"]);
});

test("a middle click on a link is a link click, other buttons are not", () => {
  const page = load();
  page.fire("auxclick", linkEvent("a.md", { button: 1 }));
  page.fire("auxclick", linkEvent("b.md", { button: 2 }));
  assert.deepEqual(page.sent, ["link:https://mdviewer.localhost/docs/a.md"]);
});

test("Alt+Left and the mouse back button go back", () => {
  const page = load();
  const altLeft = key("ArrowLeft", { altKey: true });
  page.fire("keydown", altLeft);
  page.fire("keydown", key("ArrowLeft"));
  page.fire("keydown", key("ArrowLeft", { altKey: true, ctrlKey: true }));
  const mouse = { button: 3, preventDefault() {} };
  page.fire("mouseup", mouse);
  page.fire("mouseup", { button: 0, preventDefault() {} });
  assert.deepEqual(page.sent, ["back", "back"]);
  assert.equal(altLeft.prevented, true);
});

test("a notice shows text, replaces the old notice and disappears", () => {
  const page = load();
  page.window.mdviewerNotice("Note not found: <b>x</b>");
  assert.equal(page.bodyChildren.length, 1);
  const [notice] = page.bodyChildren;
  assert.equal(notice.className, "mdviewer-notice");
  assert.equal(notice.textContent, "Note not found: <b>x</b>");
  assert.equal("innerHTML" in notice, false);
  assert.equal(notice.attributes.role, "status");

  page.window.mdviewerNotice("second");
  assert.equal(page.bodyChildren.length, 1);
  assert.equal(page.bodyChildren[0].textContent, "second");

  page.timers.at(-1).callback();
  assert.equal(page.bodyChildren.length, 0);
  assert.equal(page.timers.at(-1).delay, 4000);
});

test("sends ready after the content is loaded", () => {
  const page = load();
  assert.deepEqual(page.sent, []);
  page.fire("DOMContentLoaded");
  assert.deepEqual(page.sent, ["ready"]);
});

test("Escape without modifier keys sends close", () => {
  const page = load();
  page.fire("keydown", escape());
  assert.deepEqual(page.sent, ["close"]);
});

test("Escape with a modifier key and other keys do nothing", () => {
  const page = load();
  for (const modifier of ["ctrlKey", "altKey", "shiftKey", "metaKey"]) {
    page.fire("keydown", escape({ [modifier]: true }));
  }
  page.fire("keydown", { ...escape(), key: "Enter" });
  page.fire("keydown", { ...escape(), key: "PageDown" });
  assert.deepEqual(page.sent, []);
});

/// A keydown event that records whether the script called preventDefault.
const key = (name, modifiers = {}) => {
  const event = { ...escape(modifiers), key: name, prevented: false };
  event.preventDefault = () => {
    event.prevented = true;
  };
  return event;
};

test("Ctrl+O sends open and stops the browser default", () => {
  const page = load();
  const lower = key("o", { ctrlKey: true });
  const upper = key("O", { ctrlKey: true }); // Caps Lock
  page.fire("keydown", lower);
  page.fire("keydown", upper);
  assert.deepEqual(page.sent, ["open", "open"]);
  assert.equal(lower.prevented, true);
  assert.equal(upper.prevented, true);
});

test("o without Ctrl, or with other modifier keys, does not open", () => {
  const page = load();
  const plain = key("o");
  page.fire("keydown", plain);
  page.fire("keydown", key("o", { ctrlKey: true, shiftKey: true }));
  page.fire("keydown", key("o", { ctrlKey: true, altKey: true }));
  page.fire("keydown", key("Escape", { ctrlKey: true }));
  assert.deepEqual(page.sent, []);
  assert.equal(plain.prevented, false);
});

test("a click on the open button sends open", () => {
  const button = element();
  const page = load({ openButton: button });
  page.fire("click", { target: button });
  assert.deepEqual(page.sent, ["open"]);
  assert.equal(button.blurred, true);
});

test("a click elsewhere does not send open", () => {
  const button = element();
  const page = load({ openButton: button });
  page.fire("click", clickInside(null));
  assert.deepEqual(page.sent, []);
});

test("a click on a folded callout title opens and closes it", () => {
  const page = load();
  const callout = element({ classes: ["callout"], dataset: { calloutFold: "collapsed" } });
  const title = element({ parent: callout });
  page.fire("click", clickInside(title));
  assert.equal(callout.dataset.calloutFold, "expanded");
  page.fire("click", clickInside(title));
  assert.equal(callout.dataset.calloutFold, "collapsed");
});

test("callouts without folding do not change", () => {
  const page = load();
  const callout = element({ classes: ["callout"], dataset: { calloutFold: "none" } });
  page.fire("click", clickInside(element({ parent: callout })));
  assert.equal(callout.dataset.calloutFold, "none");
});

test("clicks outside a callout title do nothing", () => {
  const page = load();
  page.fire("click", clickInside(null));
  // A title-like element whose parent is not a callout (for example in document HTML).
  const other = element({ classes: ["note"], dataset: { calloutFold: "collapsed" } });
  page.fire("click", clickInside(element({ parent: other })));
  assert.equal(other.dataset.calloutFold, "collapsed");
  page.fire("click", clickInside(element({ parent: null })));
});

test("a theme change sets the page theme, sends it and gives the keyboard back", () => {
  const select = element();
  select.value = "dark";
  const page = load({ themeSelect: select });
  page.fire("change", { target: select });
  assert.equal(page.document.documentElement.dataset.theme, "dark");
  assert.deepEqual(page.sent, ["theme:dark"]);
  assert.equal(select.blurred, true);
});

test("changes of other elements do not change the theme", () => {
  const select = element();
  select.value = "dark";
  const page = load({ themeSelect: select });
  const fake = element();
  fake.value = "light";
  page.fire("change", { target: fake });
  assert.equal(page.document.documentElement.dataset.theme, "auto");
  assert.deepEqual(page.sent, []);
});

test("without selectors, changes do nothing", () => {
  const page = load();
  page.fire("change", { target: element() });
  page.fire("change", { target: null });
  assert.deepEqual(page.sent, []);
});

test("an images change sends the choice but does not change the theme", () => {
  const images = element();
  images.value = "block";
  const page = load({ themeSelect: element(), imagesSelect: images });
  page.fire("change", { target: images });
  assert.deepEqual(page.sent, ["images:block"]);
  assert.equal(page.document.documentElement.dataset.theme, "auto");
  assert.equal(images.blurred, true);
});

test("blocked embeds are replaced with a note that shows the address as text", () => {
  const frame = element({ attributes: { src: "https://example.com/<b>x</b>" } });
  const object = element({ attributes: { data: "movie.swf" } });
  const empty = element();
  const page = load({ embeds: [frame, object, empty] });
  page.fire("DOMContentLoaded");

  assert.deepEqual(page.queries, [".markdown-body :is(iframe, object, embed)"]);
  assert.equal(frame.replacedWith.className, "mdviewer-blocked");
  assert.equal(
    frame.replacedWith.textContent,
    "Embedded content is blocked for security: https://example.com/<b>x</b>",
  );
  assert.equal("innerHTML" in frame.replacedWith, false);
  assert.equal(
    object.replacedWith.textContent,
    "Embedded content is blocked for security: movie.swf",
  );
  assert.equal(empty.replacedWith.textContent, "Embedded content is blocked for security");
  assert.deepEqual(page.sent, ["ready"]);
});

// ---- Status bar (link preview at the bottom left) ----

/// The status bar element, or undefined before its first use.
const statusBar = (page) => page.bodyChildren.find((child) => child.className === "mdviewer-status");

/// A mouse or focus event on nothing that is a link.
const awayFromLinks = () => ({ target: { closest: () => null } });

test("the mouse on a local link shows its decoded path below the root", () => {
  const page = load();
  page.fire("mouseover", linkEvent("x", { absolute: "https://mdviewer.localhost/docs/My%20Note.md#Part%201" }));
  const status = statusBar(page);
  assert.equal(status.textContent, "docs/My Note.md#Part 1");
  assert.equal(status.hidden, false);
  assert.equal(status.attributes["aria-hidden"], "true");
  assert.equal("innerHTML" in status, false);
  // The preview needs no messages to Rust.
  assert.deepEqual(page.sent, []);
});

test("a web link shows the address exactly as the click sends it", () => {
  const page = load();
  const address = "https://xn--e1afmkfd.example/a%20b?q=%E2%80%AE#x";
  page.fire("mouseover", linkEvent("x", { absolute: address }));
  assert.equal(statusBar(page).textContent, address);
  page.fire("mouseover", linkEvent("x", { absolute: "mailto:someone@example.com" }));
  assert.equal(statusBar(page).textContent, "mailto:someone@example.com");
});

test("a wiki link that was not found shows the note that a click searches for", () => {
  const page = load();
  page.fire("mouseover", linkEvent("Other%20note#Part", { wiki: true }));
  assert.equal(statusBar(page).textContent, "Search the vault: Other note#Part");
});

test("the preview has no characters that change the text direction", () => {
  const page = load();
  // U+202E (right-to-left override), percent-encoded in a local path.
  page.fire("mouseover", linkEvent("x", { absolute: "https://mdviewer.localhost/a%E2%80%AEdm.exe" }));
  assert.equal(statusBar(page).textContent, "adm.exe");
  // The same character and U+2066, not encoded, in a wiki link.
  const override = String.fromCharCode(0x202e);
  const isolate = String.fromCharCode(0x2066);
  page.fire("mouseover", linkEvent(`Note${override}${isolate}`, { wiki: true }));
  assert.equal(statusBar(page).textContent, "Search the vault: Note");
});

test("a bad encoding or a very long address does not break the preview", () => {
  const page = load();
  page.fire("mouseover", linkEvent("x", { absolute: "https://mdviewer.localhost/100%25%zz.md" }));
  assert.equal(statusBar(page).textContent, "100%25%zz.md");
  page.fire("mouseover", linkEvent("x", { absolute: `https://example.com/${"a".repeat(2000)}` }));
  assert.equal(statusBar(page).textContent.length, 500);
});

test("the status bar is hidden away from links and outside the window", () => {
  const page = load();
  // Away from links before the first link: no status bar is made.
  page.fire("mouseover", awayFromLinks());
  assert.equal(statusBar(page), undefined);

  page.fire("mouseover", linkEvent("a.md"));
  page.fire("mouseover", awayFromLinks());
  assert.equal(statusBar(page).hidden, true);

  page.fire("mouseover", linkEvent("a.md"));
  page.fire("mouseout", { relatedTarget: {} });
  assert.equal(statusBar(page).hidden, false);
  page.fire("mouseout", { relatedTarget: null });
  assert.equal(statusBar(page).hidden, true);
  // Only one status bar.
  assert.equal(page.bodyChildren.length, 1);
});

test("the keyboard focus on a link shows the preview too", () => {
  const page = load();
  page.fire("focusin", linkEvent("x", { absolute: "https://mdviewer.localhost/b.md" }));
  assert.equal(statusBar(page).textContent, "b.md");
  page.fire("focusout", {});
  assert.equal(statusBar(page).hidden, true);
});

// ---- Back, forward and the history panel ----

test("Alt+Right and the mouse forward button go forward", () => {
  const page = load();
  const altRight = key("ArrowRight", { altKey: true });
  page.fire("keydown", altRight);
  page.fire("keydown", key("ArrowRight"));
  page.fire("keydown", key("ArrowRight", { altKey: true, shiftKey: true }));
  page.fire("mouseup", { button: 4, preventDefault() {} });
  assert.deepEqual(page.sent, ["forward", "forward"]);
  assert.equal(altRight.prevented, true);
});

test("the back and forward buttons send their messages and give the keyboard back", () => {
  const back = element();
  const forward = element();
  const page = load({ backButton: back, forwardButton: forward });
  page.fire("click", { target: back });
  page.fire("click", { target: forward });
  assert.deepEqual(page.sent, ["back", "forward"]);
  assert.equal(back.blurred, true);
  assert.equal(forward.blurred, true);
});

test("Ctrl+H and the panel button open and close the history panel", () => {
  const button = element();
  const page = load({ panelButton: button });
  const ctrlH = key("h", { ctrlKey: true });
  page.fire("keydown", ctrlH);
  assert.equal(ctrlH.prevented, true);
  assert.equal(page.document.documentElement.dataset.panel, "show");
  assert.equal(button.getAttribute("aria-pressed"), "true");

  page.fire("click", { target: button });
  assert.equal(page.document.documentElement.dataset.panel, "hide");
  assert.equal(button.getAttribute("aria-pressed"), "false");
  assert.equal(button.blurred, true);

  page.fire("keydown", key("h"));
  page.fire("keydown", key("H", { ctrlKey: true, shiftKey: true }));
  assert.deepEqual(page.sent, ["panel:show", "panel:hide"]);
});

test("a click on a panel entry sends its index, entries elsewhere do nothing", () => {
  const entry = { dataset: { history: "2" } };
  const fake = { dataset: { history: "0" } };
  const panel = { contains: (node) => node === entry };
  const page = load({ panel });
  const on = (node) => ({
    target: { closest: (selector) => (selector === "[data-history]" ? node : null) },
  });
  page.fire("click", on(entry));
  // A document element with the same attribute, outside the real panel.
  page.fire("click", on(fake));
  assert.deepEqual(page.sent, ["history:2"]);
});

// ---- Scroll position ----

test("the scroll position goes to Rust when the scrolling stops", () => {
  const page = load();
  page.window.scrollY = 120.4;
  page.fire("scroll");
  page.window.scrollY = 300.6;
  page.fire("scroll");
  assert.deepEqual(page.sent, []);
  assert.equal(page.timers.at(-1).delay, 200);
  page.runTimers();
  assert.deepEqual(page.sent, ["scroll:301"]);
  // The same position again: no new message.
  page.fire("scroll");
  page.runTimers();
  assert.deepEqual(page.sent, ["scroll:301"]);
});

test("messages that show another page send the scroll position first", () => {
  const page = load();
  page.window.scrollY = 450;
  page.fire("scroll");
  page.fire("click", linkEvent("other.md"));
  assert.deepEqual(page.sent, ["scroll:450", "link:https://mdviewer.localhost/docs/other.md"]);
  // The waiting timer does not send it again.
  page.runTimers();
  page.fire("keydown", key("ArrowLeft", { altKey: true }));
  assert.deepEqual(page.sent.slice(2), ["back"]);
});

test("the page goes to the scroll position of the history entry, and again after the load", () => {
  const page = load({ scroll: "800" });
  // The images are not loaded yet: the page is shorter.
  page.window.maxScroll = 500;
  page.fire("DOMContentLoaded");
  assert.deepEqual(page.window.scrolls, [800]);
  assert.equal(page.window.scrollY, 500);
  page.window.maxScroll = Infinity;
  page.fireWindow("load");
  assert.deepEqual(page.window.scrolls, [800, 800]);
  assert.equal(page.window.scrollY, 800);
  assert.equal(page.windowListeners.get("load")[0].options.once, true);
  // Rust already has this position: only "ready" is sent.
  page.fire("scroll");
  page.runTimers();
  assert.deepEqual(page.sent, ["ready"]);
});

test("after the load, the page does not move if the user scrolled", () => {
  const page = load({ scroll: "800" });
  page.window.maxScroll = 500;
  page.fire("DOMContentLoaded");
  page.window.scrollY = 100;
  page.fireWindow("load");
  assert.deepEqual(page.window.scrolls, [800]);
});

test("a new page starts at the top or at its heading (no scroll position)", () => {
  for (const scroll of ["0", null, "bad"]) {
    const page = load({ scroll });
    page.fire("DOMContentLoaded");
    assert.deepEqual(page.window.scrolls, [], String(scroll));
    assert.equal(page.windowListeners.get("load"), undefined);
  }
});

test("the images change sends the scroll position first (the page reloads)", () => {
  const images = element();
  images.value = "block";
  const page = load({ imagesSelect: images });
  page.window.scrollY = 75;
  page.fire("change", { target: images });
  assert.deepEqual(page.sent, ["scroll:75", "images:block"]);
});

test("a dialect change sends the scroll position first, then the choice", () => {
  const dialect = element();
  dialect.value = "obsidian";
  const page = load({ dialectSelect: dialect, imagesSelect: element(), themeSelect: element() });
  page.window.scrollY = 40;
  page.fire("change", { target: dialect });
  // Rust reloads the page with the other dialect; the scroll position must be known first.
  assert.deepEqual(page.sent, ["scroll:40", "dialect:obsidian"]);
  // The theme does not change, and the keyboard goes back to the page.
  assert.equal(page.document.documentElement.dataset.theme, "auto");
  assert.equal(dialect.blurred, true);
});

test("a change of a document element with the dialect id sends nothing", () => {
  const dialect = element();
  const fake = element();
  fake.value = "github";
  const page = load({ dialectSelect: dialect });
  page.fire("change", { target: fake });
  assert.deepEqual(page.sent, []);
});

// ---- History tree panel ----

/// A fake history panel with a current node at `offsetTop`.
function fakePanel(offsetTop) {
  const node = { offsetTop };
  return {
    scrollTop: 0,
    clientHeight: 400,
    queries: [],
    querySelector(selector) {
      this.queries.push(selector);
      return selector === '[aria-current="page"]' ? node : null;
    },
    contains: () => false,
  };
}

test("the panel scrolls so that the current node is in the middle", () => {
  const panel = fakePanel(1000);
  const page = load({ panel });
  page.fire("DOMContentLoaded");
  assert.equal(panel.scrollTop, 800);
  assert.deepEqual(panel.queries, ['[aria-current="page"]']);

  // A node near the top: the panel stays at the top.
  const top = fakePanel(50);
  load({ panel: top }).fire("DOMContentLoaded");
  assert.equal(top.scrollTop, 0);
});

test("opening the panel scrolls it to the current node, closing does not", () => {
  const panel = fakePanel(900);
  const page = load({ panel, panelButton: element() });
  page.fire("keydown", key("h", { ctrlKey: true }));
  assert.equal(panel.scrollTop, 700);
  panel.scrollTop = 5;
  page.fire("keydown", key("h", { ctrlKey: true }));
  assert.equal(panel.scrollTop, 5);
});

// ---- Width of the history panel ----

/// A fake resize line that records the pointer capture.
function fakeResizer() {
  const line = element();
  line.captured = null;
  line.setPointerCapture = (id) => (line.captured = id);
  return line;
}

/// A pointer event on `target` at the horizontal position `clientX`.
const pointer = (target, clientX, { button = 0 } = {}) => {
  const event = { target, clientX, button, pointerId: 7, prevented: false };
  event.preventDefault = () => (event.prevented = true);
  return event;
};

/// Puts `target` on the same event object (so that `prevented` stays visible).
const withTarget = (event, target) => Object.assign(event, { target });

/// The panel width that the page uses now (the CSS variable).
const cssWidth = (page) => page.document.documentElement.style.getPropertyValue("--panel-width");

test("a drag of the line changes the width at once and saves it when the drag stops", () => {
  const line = fakeResizer();
  const page = load({ resizer: line });
  const down = pointer(line, 260);
  page.fire("pointerdown", down);
  assert.equal(down.prevented, true);
  assert.equal(line.captured, 7);
  assert.equal(page.document.documentElement.dataset.resizing, "true");

  page.fire("pointermove", pointer(line, 300.4));
  page.fire("pointermove", pointer(line, 350));
  assert.equal(cssWidth(page), "350px");
  assert.equal(line.getAttribute("aria-valuenow"), "350");
  // Nothing goes to Rust during the drag.
  assert.deepEqual(page.sent, []);

  page.fire("pointerup", pointer(line, 350));
  assert.deepEqual(page.sent, ["panelwidth:350"]);
  assert.equal(page.document.documentElement.dataset.resizing, undefined);
  // Moves after the drag do nothing.
  page.fire("pointermove", pointer(line, 500));
  page.fire("pointerup", pointer(line, 500));
  assert.equal(cssWidth(page), "350px");
  assert.deepEqual(page.sent, ["panelwidth:350"]);
});

test("the width stays between the limits, and the page keeps 600 px", () => {
  const line = fakeResizer();
  const page = load({ resizer: line, innerWidth: 1000 });
  page.fire("pointerdown", pointer(line, 260));
  page.fire("pointermove", pointer(line, 20));
  assert.equal(cssWidth(page), "160px");
  page.fire("pointermove", pointer(line, 690));
  assert.equal(cssWidth(page), "400px");
  page.fire("pointercancel", {});
  assert.deepEqual(page.sent, ["panelwidth:400"]);

  // A wide window: at most 800 px.
  const wide = load({ resizer: fakeResizer(), innerWidth: 3000 });
  const wideLine = wide.document.getElementById("mdviewer-resizer");
  wide.fire("pointerdown", pointer(wideLine, 260));
  wide.fire("pointermove", pointer(wideLine, 2000));
  assert.equal(cssWidth(wide), "800px");
});

test("other buttons and other elements do not start a drag", () => {
  const line = fakeResizer();
  const page = load({ resizer: line });
  page.fire("pointerdown", pointer(line, 260, { button: 2 }));
  page.fire("pointerdown", pointer(element(), 260));
  page.fire("pointermove", pointer(line, 400));
  page.fire("pointerup", pointer(line, 400));
  assert.equal(cssWidth(page), "260px");
  assert.deepEqual(page.sent, []);
});

test("the arrow keys on the line change the width in steps", () => {
  const line = fakeResizer();
  const page = load({ resizer: line, panelWidth: "300px" });
  const right = withTarget(key("ArrowRight"), line);
  page.fire("keydown", right);
  assert.equal(right.prevented, true);
  page.fire("keydown", withTarget(key("ArrowRight"), line));
  page.fire("keydown", withTarget(key("ArrowLeft"), line));
  assert.equal(cssWidth(page), "316px");
  assert.deepEqual(page.sent, ["panelwidth:316", "panelwidth:332", "panelwidth:316"]);
  // Alt+Left on the line is still "Back", and arrows elsewhere do nothing here.
  page.fire("keydown", withTarget(key("ArrowLeft", { altKey: true }), line));
  page.fire("keydown", withTarget(key("ArrowLeft"), element()));
  assert.deepEqual(page.sent.slice(3), ["back"]);
});

test("a double-click on the line gives the default width", () => {
  const line = fakeResizer();
  const page = load({ resizer: line, panelWidth: "512px" });
  page.fire("dblclick", { target: element() });
  assert.equal(cssWidth(page), "512px");
  page.fire("dblclick", { target: line });
  assert.equal(cssWidth(page), "260px");
  assert.deepEqual(page.sent, ["panelwidth:260"]);
});
