// mdviewer page script. The web view adds it to the page before the page loads,
// so the CSP of the page does not block it. Documents cannot run their own scripts.
// Tests: tests/js/viewer.test.mjs (run by `cargo test`).
(() => {
  "use strict";
  if (location.origin !== "https://mdviewer.localhost") {
    return;
  }

  const send = (message) => window.ipc.postMessage(message);

  // The viewer elements come before the document content, so getElementById finds
  // them first, even if the document has elements with the same ids.
  const byId = (id) => document.getElementById(id);

  // The CSP blocks frames and plugins. Without this, WebView2 shows its own message,
  // which tells the user to "contact the site owner". This note says what is missing.
  const replaceBlockedEmbeds = () => {
    for (const element of document.querySelectorAll(".markdown-body :is(iframe, object, embed)")) {
      const address = element.getAttribute("src") || element.getAttribute("data") || "";
      const note = document.createElement("div");
      note.className = "mdviewer-blocked";
      // textContent, not innerHTML: the address comes from the document.
      note.textContent = address
        ? `Embedded content is blocked for security: ${address}`
        : "Embedded content is blocked for security";
      element.replaceWith(note);
    }
  };

  // Scroll position. Rust keeps it in the history entry of the page, so that "Back"
  // goes to the same place. The page sends it 200 ms after the scrolling stops, and
  // again before each message that can show another page (see `leave`).
  //
  // This script starts before the page exists (document.documentElement is null), so
  // the position of the history entry (<html data-scroll>) is read at DOMContentLoaded.
  const SCROLL_DELAY = 200;
  let startScroll = 0;
  let sentScroll = 0;
  let scrollTimer;
  const sendScroll = () => {
    clearTimeout(scrollTimer);
    const scroll = Math.max(0, Math.round(window.scrollY) || 0);
    if (scroll !== sentScroll) {
      sentScroll = scroll;
      send(`scroll:${scroll}`);
    }
  };
  document.addEventListener("scroll", () => {
    clearTimeout(scrollTimer);
    scrollTimer = setTimeout(sendScroll, SCROLL_DELAY);
  });
  const leave = (message) => {
    sendScroll();
    send(message);
  };

  // "Back" and "Forward" give the page the scroll position of the entry
  // (<html data-scroll>). Images and diagrams can make the page longer after
  // DOMContentLoaded, so the page goes there again after the load, if the user did not
  // scroll in the meantime.
  const restoreScroll = () => {
    startScroll = Number(document.documentElement?.dataset.scroll) || 0;
    // Rust already has this position.
    sentScroll = startScroll;
    if (startScroll <= 0) {
      return;
    }
    window.scrollTo(0, startScroll);
    const first = window.scrollY;
    window.addEventListener(
      "load",
      () => {
        if (window.scrollY === first) {
          window.scrollTo(0, startScroll);
        }
      },
      { once: true },
    );
  };

  // The history panel can be longer than the window: scroll it so that the current
  // node is in the middle. (A hidden panel has no layout; it is done again on open.)
  const showCurrentNode = () => {
    const panel = byId("mdviewer-history");
    const node = panel?.querySelector?.('[aria-current="page"]');
    if (node) {
      panel.scrollTop = Math.max(0, node.offsetTop - panel.clientHeight / 2);
    }
  };

  document.addEventListener("DOMContentLoaded", () => {
    replaceBlockedEmbeds();
    restoreScroll();
    showCurrentNode();
    requestAnimationFrame(() => send("ready"));
  });

  // A short message at the bottom of the page. Rust calls it, for example for
  // "Note not found". textContent, not innerHTML: the text can contain document text.
  window.mdviewerNotice = (text) => {
    document.querySelector(".mdviewer-notice")?.remove();
    const notice = document.createElement("div");
    notice.className = "mdviewer-notice";
    notice.setAttribute("role", "status");
    notice.textContent = text;
    document.body.append(notice);
    setTimeout(() => notice.remove(), 4000);
  };

  const decode = (text) => {
    try {
      return decodeURIComponent(text);
    } catch {
      return text;
    }
  };

  // Characters that change the direction of text (Unicode "bidi" controls). In a
  // decoded address, they could make "moc.live" show as "evil.com". They are built
  // from their numbers, so that no tool changes them in this file.
  const DIRECTION_CONTROLS = new RegExp(
    `[${[0x061c, 0x200e, 0x200f, 0x202a, 0x202b, 0x202c, 0x202d, 0x202e, 0x2066, 0x2067, 0x2068, 0x2069]
      .map((code) => String.fromCharCode(code))
      .join("")}]`,
    "g",
  );
  const MAX_PREVIEW = 500;

  // The text of the status bar for a link (as a browser shows it at the bottom left):
  // - a web or mail link: the address exactly as the click sends it. It is not
  //   decoded: the browser encodes all characters that are not ASCII, so the text
  //   cannot hide its real host;
  // - a local link: the decoded path below the root folder (vault or repository);
  // - a wiki link that was not found: the note that a click searches for.
  const linkPreview = (link) => {
    let text;
    if (link.dataset.wikilink !== undefined) {
      text = `Search the vault: ${decode(link.getAttribute("href"))}`;
    } else {
      const address = String(link.href);
      const local = `${location.origin}/`;
      if (address.startsWith(local)) {
        let path = address.slice(local.length);
        try {
          path = decodeURI(path);
        } catch {
          // Bad percent encoding: show the path as it is.
        }
        text = path || "/";
      } else {
        text = address;
      }
    }
    return text.replace(DIRECTION_CONTROLS, "").slice(0, MAX_PREVIEW);
  };

  // The status bar. It is made on the first use and only hidden after that.
  let statusBar = null;
  const showStatus = (text) => {
    if (!statusBar) {
      statusBar = document.createElement("div");
      statusBar.className = "mdviewer-status";
      // Screen readers already read the link; the status bar would repeat it.
      statusBar.setAttribute("aria-hidden", "true");
      document.body.append(statusBar);
    }
    statusBar.textContent = text;
    statusBar.hidden = false;
  };
  const hideStatus = () => {
    if (statusBar) {
      statusBar.hidden = true;
    }
  };
  const linkAt = (target) => target?.closest?.("a[href]") ?? null;
  // The mouse on a link, or the keyboard focus on it (Tab).
  const previewLink = (event) => {
    const link = linkAt(event.target);
    if (link) {
      showStatus(linkPreview(link));
    } else {
      hideStatus();
    }
  };
  document.addEventListener("mouseover", previewLink);
  document.addEventListener("focusin", previewLink);
  document.addEventListener("focusout", hideStatus);
  // The mouse leaves the window.
  document.addEventListener("mouseout", (event) => {
    if (!event.relatedTarget) {
      hideStatus();
    }
  });

  // Links: Rust decides what a click does (viewer, browser, File Explorer or blocked).
  // Links to a place in the same page (#...) stay in the page: the browser scrolls.
  // Gives true if the event was a link click.
  const followLink = (event) => {
    const link = linkAt(event.target);
    if (!link) {
      return false;
    }
    const href = link.getAttribute("href");
    if (href.startsWith("#")) {
      return false;
    }
    event.preventDefault();
    if (link.dataset.wikilink !== undefined) {
      // A wiki link that was not found when the page opened: Rust searches for it now.
      leave(`wikilink:${decode(href)}`);
    } else {
      // link.href is absolute: the browser resolves it against the page URL.
      leave(`link:${link.href}`);
    }
    return true;
  };

  // The history panel opens and closes at once (CSS: :root[data-panel]); Rust saves
  // the state.
  const togglePanel = () => {
    const root = document.documentElement;
    const panel = root.dataset.panel === "show" ? "hide" : "show";
    root.dataset.panel = panel;
    byId("mdviewer-panel-button")?.setAttribute("aria-pressed", String(panel === "show"));
    if (panel === "show") {
      showCurrentNode();
    }
    send(`panel:${panel}`);
  };

  // The width of the history panel. The user drags the line at its right side
  // (#mdviewer-resizer), or uses the arrow keys when the line has the keyboard focus.
  // A double-click gives the default width. The page changes at once (CSS variable
  // --panel-width); Rust saves the width only when the drag stops. The limits are the
  // same as in settings.rs (a Rust test checks them); the page keeps at least 600 px
  // (page.rs: MIN_PAGE_WIDTH), so that the lines of the document stay readable.
  const PANEL_MIN = 160;
  const PANEL_MAX = 800;
  const PANEL_DEFAULT = 260;
  const PANEL_STEP = 16;
  const PAGE_MIN = 600;
  let resizing = false;
  let panelWidth = null;

  const clampWidth = (width) => {
    const room = (Number(window.innerWidth) || Infinity) - PAGE_MIN;
    const max = Math.max(PANEL_MIN, Math.min(PANEL_MAX, room));
    return Math.round(Math.min(Math.max(width, PANEL_MIN), max));
  };
  // The width now: the last one that was set, or the one from Rust (<html style>).
  const currentWidth = () =>
    panelWidth ??
    (parseInt(document.documentElement.style.getPropertyValue("--panel-width"), 10) || PANEL_DEFAULT);
  const setPanelWidth = (width) => {
    panelWidth = clampWidth(width);
    document.documentElement.style.setProperty("--panel-width", `${panelWidth}px`);
    byId("mdviewer-resizer")?.setAttribute("aria-valuenow", String(panelWidth));
  };
  const savePanelWidth = () => {
    if (panelWidth !== null) {
      send(`panelwidth:${panelWidth}`);
    }
  };
  const isResizer = (target) => {
    const resizer = byId("mdviewer-resizer");
    return Boolean(resizer) && target === resizer;
  };

  document.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 || !isResizer(event.target)) {
      return;
    }
    event.preventDefault();
    resizing = true;
    // The moves still come when the mouse leaves the line or the window.
    event.target.setPointerCapture?.(event.pointerId);
    document.documentElement.dataset.resizing = "true";
  });
  document.addEventListener("pointermove", (event) => {
    if (resizing) {
      // The panel starts at the left side of the window.
      setPanelWidth(event.clientX);
    }
  });
  const stopResizing = () => {
    if (!resizing) {
      return;
    }
    resizing = false;
    delete document.documentElement.dataset.resizing;
    savePanelWidth();
  };
  document.addEventListener("pointerup", stopResizing);
  document.addEventListener("pointercancel", stopResizing);
  document.addEventListener("dblclick", (event) => {
    if (isResizer(event.target)) {
      setPanelWidth(PANEL_DEFAULT);
      savePanelWidth();
    }
  });

  document.addEventListener("keydown", (event) => {
    const others = event.altKey || event.shiftKey || event.metaKey;
    const arrow = event.key === "ArrowLeft" || event.key === "ArrowRight";
    if (arrow && isResizer(event.target) && !others && !event.ctrlKey) {
      event.preventDefault();
      setPanelWidth(currentWidth() + (event.key === "ArrowLeft" ? -PANEL_STEP : PANEL_STEP));
      savePanelWidth();
      return;
    }
    const altOnly = event.altKey && !event.ctrlKey && !event.shiftKey && !event.metaKey;
    const key = event.key.toLowerCase();
    if (event.key === "Escape" && !others && !event.ctrlKey) {
      send("close");
    } else if (key === "o" && event.ctrlKey && !others) {
      event.preventDefault();
      leave("open");
    } else if (key === "h" && event.ctrlKey && !others) {
      event.preventDefault();
      togglePanel();
    } else if (event.key === "ArrowLeft" && altOnly) {
      event.preventDefault();
      leave("back");
    } else if (event.key === "ArrowRight" && altOnly) {
      event.preventDefault();
      leave("forward");
    }
  });

  // The mouse "back" (3) and "forward" (4) buttons.
  document.addEventListener("mouseup", (event) => {
    if (event.button === 3 || event.button === 4) {
      event.preventDefault();
      leave(event.button === 3 ? "back" : "forward");
    }
  });

  // Middle click on a link: the same as a click (not a new window).
  document.addEventListener("auxclick", (event) => {
    if (event.button === 1) {
      followLink(event);
    }
  });

  // The viewer buttons and the messages that they send. The icons are CSS, so the
  // click target is the button itself.
  const BUTTONS = [
    ["mdviewer-open", "open"],
    ["mdviewer-back", "back"],
    ["mdviewer-forward", "forward"],
  ];

  document.addEventListener("click", (event) => {
    const target = event.target;
    for (const [id, message] of BUTTONS) {
      const button = byId(id);
      if (button && target === button) {
        leave(message);
        // Give the keyboard back to the page, so that Page Down scrolls again.
        button.blur();
        return;
      }
    }
    const panelButton = byId("mdviewer-panel-button");
    if (panelButton && target === panelButton) {
      togglePanel();
      panelButton.blur();
      return;
    }

    // An entry of the history panel. Only entries inside the real panel count: the
    // document can also have elements with a data-history attribute.
    const entry = target?.closest?.("[data-history]");
    const panel = byId("mdviewer-history");
    if (entry && panel && panel.contains(entry)) {
      leave(`history:${entry.dataset.history}`);
      return;
    }

    if (followLink(event)) {
      return;
    }

    // Folded Obsidian callouts: a click on the title opens or closes the callout.
    const title = target?.closest?.(".callout-title");
    const callout = title && title.parentElement;
    if (!callout || !callout.classList.contains("callout")) {
      return;
    }
    const fold = callout.dataset.calloutFold;
    if (fold === "collapsed") {
      callout.dataset.calloutFold = "expanded";
    } else if (fold === "expanded") {
      callout.dataset.calloutFold = "collapsed";
    }
  });

  // The toolbar selectors.
  // - Theme: the page changes at once (CSS: :root[data-theme]); Rust saves the choice
  //   and changes the title bar.
  // - Remote images: Rust saves the choice and reloads the page with a new CSP. The
  //   reloaded page goes back to the same scroll position.
  // - Dialect: Rust saves the choice for the folder of the file and reloads the page
  //   (rendered with the chosen dialect), at the same scroll position.
  document.addEventListener("change", (event) => {
    const theme = byId("mdviewer-theme");
    const images = byId("mdviewer-images");
    const dialect = byId("mdviewer-dialect");
    if (event.target === theme && theme) {
      document.documentElement.dataset.theme = theme.value;
      send(`theme:${theme.value}`);
    } else if (event.target === images && images) {
      leave(`images:${images.value}`);
    } else if (event.target === dialect && dialect) {
      leave(`dialect:${dialect.value}`);
    } else {
      return;
    }
    // Give the keyboard back to the page, so that Page Down scrolls again.
    event.target.blur();
  });
})();
