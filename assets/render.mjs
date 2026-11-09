// mdviewer content rendering: math (KaTeX), diagrams (Mermaid) and code colors
// (highlight.js). Each page loads this small module; it loads a library only when the
// page needs it:
//   [data-math-style]                 -> KaTeX
//   pre > code.language-mermaid       -> Mermaid (ESM: only the chunks a diagram needs)
//   pre > code.language-<other>       -> highlight.js
// The CSP allows scripts from /_mdviewer/ only, so documents cannot add their own.
// Tests: tests/js/render.test.mjs (run by `cargo test`).

export const BASE = "/_mdviewer/";

const LANGUAGE_PREFIX = "language-";

/// The elements that need work, below the document content `root`.
export function findWork(root) {
  const blocks = [...root.querySelectorAll(`pre > code[class*="${LANGUAGE_PREFIX}"]`)];
  const is = (code, language) => code.classList.contains(LANGUAGE_PREFIX + language);
  return {
    math: [...root.querySelectorAll("[data-math-style]")],
    diagrams: blocks.filter((code) => is(code, "mermaid")),
    code: blocks.filter((code) => !is(code, "mermaid") && !is(code, "math")),
  };
}

/// Replaces each math element (and its <pre> for ```math blocks) with KaTeX output.
export function renderMath(katex, elements, doc) {
  for (const element of elements) {
    const display = element.dataset.mathStyle === "display";
    const parent = element.parentElement;
    const target = parent && parent.tagName === "PRE" ? parent : element;
    const holder = doc.createElement("span");
    holder.className = "mdviewer-math";
    try {
      // trust: false (the default) blocks \href{javascript:...} and \html... commands.
      katex.render(element.textContent, holder, {
        displayMode: display,
        throwOnError: false,
        trust: false,
      });
    } catch (error) {
      holder.className = "mdviewer-math mdviewer-error";
      holder.textContent = element.textContent;
    }
    target.replaceWith(holder);
  }
}

/// Colors code blocks with a known language. Unknown languages stay plain text.
export function highlightCode(hljs, blocks) {
  hljs.configure({ ignoreUnescapedHTML: true });
  for (const code of blocks) {
    const language = [...code.classList]
      .find((name) => name.startsWith(LANGUAGE_PREFIX))
      ?.slice(LANGUAGE_PREFIX.length);
    if (language && hljs.getLanguage(language)) {
      hljs.highlightElement(code);
    }
  }
}

/// Gives true if the page shows the dark theme (data-theme, or Windows for "auto").
export function isDark(doc, matchMedia) {
  const theme = doc.documentElement.dataset.theme;
  return theme === "dark" || (theme !== "light" && matchMedia("(prefers-color-scheme: dark)").matches);
}

/// Replaces each ```mermaid block with a holder that keeps the source. Until Mermaid
/// draws the diagram, the holder shows the source text.
export function prepareDiagrams(blocks, doc) {
  return blocks.map((code) => {
    const holder = doc.createElement("div");
    holder.className = "mdviewer-diagram mdviewer-pending";
    holder.dataset.source = code.textContent;
    holder.textContent = code.textContent;
    code.parentElement.replaceWith(holder);
    return holder;
  });
}

let diagramCount = 0;

/// Draws (or draws again, for example after a theme change) all diagrams.
/// Gives the error texts of diagrams that cannot be drawn.
export async function drawDiagrams(mermaid, holders, dark) {
  // "strict": Mermaid removes HTML and click handlers from the diagram text.
  mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: dark ? "dark" : "default" });
  const errors = [];
  for (const holder of holders) {
    const source = holder.dataset.source;
    try {
      // parse() first: render() can leave an error element in the page on bad input.
      await mermaid.parse(source);
      diagramCount += 1;
      const { svg } = await mermaid.render(`mdviewer-diagram-${diagramCount}`, source);
      holder.className = "mdviewer-diagram";
      holder.innerHTML = svg;
    } catch (error) {
      const message = `Diagram error: ${error?.message ?? error}`;
      holder.className = "mdviewer-diagram mdviewer-error";
      holder.textContent = `${message}\n\n${source}`;
      errors.push(message);
    }
  }
  return errors;
}

/// Loads a classic (non-module) script, for example KaTeX.
export function loadScript(doc, src) {
  return new Promise((resolve, reject) => {
    const script = doc.createElement("script");
    script.src = src;
    script.onload = () => resolve();
    script.onerror = () => reject(new Error(`cannot load ${src}`));
    doc.head.append(script);
  });
}

function addStylesheet(doc, href) {
  const link = doc.createElement("link");
  link.rel = "stylesheet";
  link.href = href;
  doc.head.append(link);
}

/// Renders the page. `importModule` is `import()`; tests give a fake.
export async function main(doc, win, importModule) {
  const root = doc.querySelector(".markdown-body");
  if (!root) {
    return;
  }
  const report = (error) => win.ipc.postMessage(`error:${error?.message ?? error}`);
  const work = findWork(root);
  const jobs = [];

  if (work.math.length > 0) {
    addStylesheet(doc, `${BASE}katex/katex.min.css`);
    jobs.push(loadScript(doc, `${BASE}katex/katex.min.js`).then(() => renderMath(win.katex, work.math, doc)));
  }
  if (work.code.length > 0) {
    jobs.push(importModule(`${BASE}highlight/highlight.min.js`).then((module) => highlightCode(module.default, work.code)));
  }
  if (work.diagrams.length > 0) {
    const holders = prepareDiagrams(work.diagrams, doc);
    jobs.push(
      importModule(`${BASE}mermaid/mermaid.esm.min.mjs`).then(async (module) => {
        const mermaid = module.default;
        const draw = async () => {
          for (const error of await drawDiagrams(mermaid, holders, isDark(doc, win.matchMedia))) {
            report(error);
          }
        };
        await draw();
        // Mermaid colors are fixed in the SVG, so a theme change needs a new drawing.
        win.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", draw);
        new win.MutationObserver(draw).observe(doc.documentElement, {
          attributes: true,
          attributeFilter: ["data-theme"],
        });
      }),
    );
  }

  for (const result of await Promise.allSettled(jobs)) {
    if (result.status === "rejected") {
      report(result.reason);
    }
  }
}

if (typeof document !== "undefined") {
  main(document, window, (url) => import(url));
}
