// Tests for assets/render.mjs. Run: node --test tests/js/render.test.mjs
// (`cargo test` runs them too, see tests/page_script.rs).
//
// The module exports its functions, and each function gets the libraries and the
// document as parameters. Thus, these tests use small fakes; no npm packages.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  BASE,
  drawDiagrams,
  findWork,
  highlightCode,
  isDark,
  main,
  prepareDiagrams,
  renderMath,
} from "../../assets/render.mjs";

/// A fake element with the parts that render.mjs uses.
function element(tag, { classes = [], dataset = {}, text = "", parent = null } = {}) {
  const classList = [...classes];
  classList.contains = (name) => classList.includes(name);
  const self = {
    tagName: tag.toUpperCase(),
    classList,
    dataset,
    textContent: text,
    parentElement: parent,
    className: classes.join(" "),
    replacedWith: null,
    replaceWith(other) {
      self.replacedWith = other;
    },
  };
  return self;
}

/// A fake document. `head.append` "loads" scripts at once.
function fakeDocument(theme = "auto") {
  const appended = [];
  return {
    appended,
    documentElement: { dataset: { theme } },
    createElement: (tag) => element(tag),
    head: {
      append(child) {
        appended.push(child);
        if (child.tagName === "SCRIPT") {
          queueMicrotask(() => child.onload());
        }
      },
    },
  };
}

/// A code block: <pre><code class="language-X">text</code></pre>.
function codeBlock(language, text = "", dataset = {}) {
  const pre = element("pre");
  return element("code", { classes: [`language-${language}`], text, parent: pre, dataset });
}

/// A fake root for querySelectorAll.
function fakeRoot({ math = [], blocks = [] }) {
  return {
    querySelectorAll(selector) {
      if (selector === "[data-math-style]") return math;
      if (selector === 'pre > code[class*="language-"]') return blocks;
      throw new Error(`unexpected selector ${selector}`);
    },
  };
}

function fakeMermaid({ failOn = "bad" } = {}) {
  const calls = { initialize: [], render: [] };
  return {
    calls,
    initialize: (config) => calls.initialize.push(config),
    parse: async (source) => {
      if (source.includes(failOn)) throw new Error(`Parse error in "${source}"`);
    },
    render: async (id, source) => {
      calls.render.push(id);
      return { svg: `<svg id="${id}">${source}</svg>` };
    },
  };
}

test("findWork separates math, diagrams and other code", () => {
  const inlineMath = element("span", { dataset: { mathStyle: "inline" } });
  const mathBlock = codeBlock("math", "x", { mathStyle: "display" });
  const diagram = codeBlock("mermaid", "graph TD");
  const rust = codeBlock("rust", "fn main() {}");
  const work = findWork(fakeRoot({ math: [inlineMath, mathBlock], blocks: [mathBlock, diagram, rust] }));
  assert.deepEqual(work.math, [inlineMath, mathBlock]);
  assert.deepEqual(work.diagrams, [diagram]);
  assert.deepEqual(work.code, [rust]);
});

test("renderMath uses display mode, safe options, and replaces the <pre> of math blocks", () => {
  const calls = [];
  const katex = { render: (text, holder, options) => calls.push({ text, holder, options }) };
  const inline = element("span", { dataset: { mathStyle: "inline" }, text: "a^2", parent: element("p") });
  const block = codeBlock("math", "\\sum x", { mathStyle: "display" });
  renderMath(katex, [inline, block], fakeDocument());

  assert.equal(calls.length, 2);
  assert.deepEqual(calls[0].options, { displayMode: false, throwOnError: false, trust: false });
  assert.equal(calls[0].text, "a^2");
  assert.equal(calls[1].options.displayMode, true);
  assert.equal(inline.replacedWith, calls[0].holder);
  assert.equal(block.replacedWith, null, "the <code> stays in its <pre>");
  assert.equal(block.parentElement.replacedWith, calls[1].holder, "the <pre> is replaced");
  assert.equal(calls[1].holder.className, "mdviewer-math");
});

test("renderMath shows the source text when KaTeX throws", () => {
  const katex = {
    render() {
      throw new Error("too many expansions");
    },
  };
  const math = element("span", { dataset: { mathStyle: "inline" }, text: "\\loop", parent: element("p") });
  renderMath(katex, [math], fakeDocument());
  assert.equal(math.replacedWith.className, "mdviewer-math mdviewer-error");
  assert.equal(math.replacedWith.textContent, "\\loop");
});

test("highlightCode colors known languages only", () => {
  const highlighted = [];
  let configured = null;
  const hljs = {
    configure: (options) => (configured = options),
    getLanguage: (name) => (["rust", "python"].includes(name) ? {} : undefined),
    highlightElement: (code) => highlighted.push(code),
  };
  const rust = codeBlock("rust");
  const unknown = codeBlock("klingon");
  const python = codeBlock("python");
  highlightCode(hljs, [rust, unknown, python]);
  assert.deepEqual(highlighted, [rust, python]);
  assert.deepEqual(configured, { ignoreUnescapedHTML: true });
});

test("isDark follows data-theme, and Windows for auto", () => {
  const media = (dark) => () => ({ matches: dark });
  assert.equal(isDark(fakeDocument("dark"), media(false)), true);
  assert.equal(isDark(fakeDocument("light"), media(true)), false);
  assert.equal(isDark(fakeDocument("auto"), media(true)), true);
  assert.equal(isDark(fakeDocument("auto"), media(false)), false);
});

test("prepareDiagrams replaces each block with a holder that shows the source", () => {
  const diagram = codeBlock("mermaid", "graph TD; A-->B");
  const [holder] = prepareDiagrams([diagram], fakeDocument());
  assert.equal(diagram.parentElement.replacedWith, holder);
  assert.equal(holder.dataset.source, "graph TD; A-->B");
  assert.equal(holder.textContent, "graph TD; A-->B");
  assert.equal(holder.className, "mdviewer-diagram mdviewer-pending");
});

test("drawDiagrams uses strict security, the theme, and unique ids", async () => {
  const mermaid = fakeMermaid();
  const doc = fakeDocument();
  const holders = prepareDiagrams([codeBlock("mermaid", "graph A"), codeBlock("mermaid", "graph B")], doc);

  assert.deepEqual(await drawDiagrams(mermaid, holders, true), []);
  assert.deepEqual(mermaid.calls.initialize[0], {
    startOnLoad: false,
    securityLevel: "strict",
    theme: "dark",
  });
  assert.equal(holders[0].innerHTML.startsWith("<svg"), true);
  assert.equal(holders[0].className, "mdviewer-diagram");

  await drawDiagrams(mermaid, holders, false);
  assert.equal(mermaid.calls.initialize[1].theme, "default");
  assert.equal(new Set(mermaid.calls.render).size, 4, "each drawing has a new id");
});

test("drawDiagrams reports bad diagrams and keeps their source visible", async () => {
  const mermaid = fakeMermaid();
  const holders = prepareDiagrams([codeBlock("mermaid", "bad diagram"), codeBlock("mermaid", "graph OK")], fakeDocument());
  const errors = await drawDiagrams(mermaid, holders, false);
  assert.equal(errors.length, 1);
  assert.match(errors[0], /^Diagram error: Parse error/);
  assert.equal(holders[0].className, "mdviewer-diagram mdviewer-error");
  assert.match(holders[0].textContent, /bad diagram$/);
  assert.equal(holders[0].innerHTML, undefined, "no HTML from a failed diagram");
  assert.equal(holders[1].className, "mdviewer-diagram", "the next diagram is still drawn");
});

/// A fake window for main().
function fakeWindow({ katex = { render() {} }, dark = false } = {}) {
  const sent = [];
  const observers = [];
  const mediaListeners = [];
  return {
    sent,
    observers,
    mediaListeners,
    katex,
    ipc: { postMessage: (message) => sent.push(message) },
    matchMedia: () => ({ matches: dark, addEventListener: (_, listener) => mediaListeners.push(listener) }),
    MutationObserver: class {
      constructor(callback) {
        this.callback = callback;
        observers.push(this);
      }
      observe(target, options) {
        this.target = target;
        this.options = options;
      }
    },
  };
}

function docWithContent(content, theme = "auto") {
  const doc = fakeDocument(theme);
  doc.querySelector = (selector) => (selector === ".markdown-body" ? content : null);
  return doc;
}

test("main loads nothing for a page without math, diagrams or code", async () => {
  const imports = [];
  const doc = docWithContent(fakeRoot({}));
  await main(doc, fakeWindow(), async (url) => imports.push(url));
  assert.deepEqual(imports, []);
  assert.deepEqual(doc.appended, []);
});

test("main does nothing without the content element", async () => {
  const imports = [];
  await main(docWithContent(null), fakeWindow(), async (url) => imports.push(url));
  assert.deepEqual(imports, []);
});

test("main loads each library only when needed, from the viewer path", async () => {
  const katexCalls = [];
  const win = fakeWindow({ katex: { render: (text) => katexCalls.push(text) } });
  const mermaid = fakeMermaid();
  const highlighted = [];
  const hljs = {
    configure() {},
    getLanguage: () => ({}),
    highlightElement: (code) => highlighted.push(code),
  };
  const imports = [];
  const importModule = async (url) => {
    imports.push(url);
    return { default: url.includes("mermaid") ? mermaid : hljs };
  };
  const math = element("span", { dataset: { mathStyle: "inline" }, text: "x", parent: element("p") });
  const rust = codeBlock("rust");
  const doc = docWithContent(fakeRoot({ math: [math], blocks: [codeBlock("mermaid", "graph TD"), rust] }));

  await main(doc, win, importModule);

  assert.deepEqual(imports.sort(), [`${BASE}highlight/highlight.min.js`, `${BASE}mermaid/mermaid.esm.min.mjs`]);
  const loaded = doc.appended.map((child) => child.src ?? child.href);
  assert.deepEqual(loaded, [`${BASE}katex/katex.min.css`, `${BASE}katex/katex.min.js`]);
  assert.deepEqual(katexCalls, ["x"]);
  assert.deepEqual(highlighted, [rust]);
  assert.equal(mermaid.calls.render.length, 1);
  assert.deepEqual(win.sent, []);

  // A theme change draws the diagrams again.
  assert.equal(win.observers.length, 1);
  assert.deepEqual(win.observers[0].options, { attributes: true, attributeFilter: ["data-theme"] });
  doc.documentElement.dataset.theme = "dark";
  await win.observers[0].callback();
  assert.equal(mermaid.calls.render.length, 2);
  assert.equal(mermaid.calls.initialize.at(-1).theme, "dark");
  assert.equal(win.mediaListeners.length, 1);
});

test("main reports load errors and diagram errors to Rust", async () => {
  const win = fakeWindow();
  const importModule = async (url) => {
    if (url.includes("highlight")) throw new Error(`cannot load ${url}`);
    return { default: fakeMermaid() };
  };
  const doc = docWithContent(fakeRoot({ blocks: [codeBlock("rust"), codeBlock("mermaid", "bad")] }));
  await main(doc, win, importModule);
  assert.equal(win.sent.length, 2);
  assert.ok(win.sent.some((message) => message.startsWith("error:Diagram error:")), win.sent);
  assert.ok(win.sent.includes(`error:cannot load ${BASE}highlight/highlight.min.js`), win.sent);
});
