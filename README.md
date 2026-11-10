# mdviewer

A fast, read-only Markdown viewer for Windows. Open a `.md` file and read it, as GitHub
or Obsidian would show it. mdviewer is not an editor.

mdviewer detects the Markdown **dialect** of each file automatically:

| Dialect | Detected when | Shows |
|---|---|---|
| **Obsidian** | the file is in an Obsidian vault (an `.obsidian` folder above it, or the vault is in Obsidian's own vault list), or the text uses Obsidian syntax | wiki links, embeds, callouts, `==highlight==`, `%%comments%%`, hard line breaks |
| **GitHub** | the file is in a Git repository, or has a typical name (`README.md`, …), or nothing else matches | alerts, emoji `:rocket:`, GitHub math, task lists |

If the detection is wrong, select the dialect in the **Dialect** list of the toolbar.
mdviewer remembers the choice for the folder of the file and its subfolders (the
nearest folder with a choice wins). **Auto** goes back to the detection; it also
shows which dialect the detection found, for example "Auto (Obsidian)".

Both dialects show tables, footnotes, task lists, math (KaTeX), diagrams (Mermaid) and
colored code (highlight.js). Alerts and callouts have monochrome icons, as on GitHub
(Octicons) and in Obsidian (Lucide).

## Requirements

- Windows 10 or 11 with the **Microsoft Edge WebView2 Runtime** (Windows 11 has it).
- To build: Rust (edition 2024) with the MSVC toolchain.
- To run the tests: **Node.js** (the JavaScript tests use Node's built-in test runner)
  and **PowerShell 7** (`pwsh`; the tests of the release scripts).

## Build

```powershell
cargo build --release
```

The program is `target\release\mdviewer.exe` (about 8.4 MB, because it contains KaTeX,
Mermaid and highlight.js).

## Use

```powershell
mdviewer.exe "D:\Notes\Daily\today.md"
cargo run -- README.md          # during development
```

Without a file, mdviewer shows a page with instructions. You can also **drag a `.md`
file** onto the window.

| Key or action | What it does |
|---|---|
| **Ctrl+O** or **Open…** | Shows the Windows "Open" dialog. It starts in the folder of the current file. |
| **Escape** | Closes the window. |
| **Alt+Left**, **←** or the mouse "back" button | Opens the file that you came from, at the place where you left it. |
| **Alt+Right**, **→** or the mouse "forward" button | Opens the file that you went to last from here. |
| **Ctrl+H** or the history button | Opens or closes the history panel: the tree of the files of this session. A click opens that file. Drag the right edge of the panel to change its width (double-click: default width). |
| Mouse on a link (or Tab to it) | Shows the address at the bottom left, as a browser does. |
| Click a web or mail link | Opens it in your default program (`http`, `https`, `mailto` only). |
| Click a link to a `.md` file or a wiki link | Opens the note in mdviewer, at the heading if the link has one. |
| Click a link to another local file | Shows the file in File Explorer. mdviewer never runs it. |
| Click a faded wiki link | Searches the vault for the note (see "Privacy"). |

The buttons at the top left are **←** (back), **→** (forward) and the history panel.
The toolbar (top right) has **Open…**, **Dialect** (Auto, GitHub, Obsidian; only when
a file is open), **Theme** (Auto, Light, Dark) and **Remote images** (Show, Block).

The history has only the local Markdown files that mdviewer showed. Web links and
files that open in File Explorer are not in it. The history is in memory only: when
you close the window, it is lost.

The history is a **tree**: each file shows below the file whose link you clicked.

```
a.md
├─ b.md
│  └─ Deep note.md   ← current
└─ Deep note.md      (faded: the same file, opened from another note)
```

- A file that you opened from two notes shows two times, once below each note. The
  second line is faded.
- A link back to a note that is already above you in the branch (for example, A links
  to B, and B links back to A) goes up to that note. It adds nothing to the tree, so
  notes that link to each other cannot make the tree grow.
- A link to a file that is already below the current note goes to that line too.
- "Open…" and drag and drop clear the history: it starts again with only the new file.
- A note that you go back to opens at the place where you left it. If the link names
  a heading, the note opens at the heading.
- Nothing is lost when you go back and then open a different link: the old branch
  stays in the tree. The tree keeps the last 200 visits.

## Files

| What | Where |
|---|---|
| Settings | `%APPDATA%\mdviewer\settings.ini` |
| Dialect choices | `%APPDATA%\mdviewer\dialects.ini`: one line for each folder, for example `D:\Notes = obsidian` (`auto`, `github` or `obsidian`). At most 1000 folders; the oldest choice goes first. |
| Log | `%LOCALAPPDATA%\mdviewer\logs\mdviewer.log` (moved to `mdviewer.old.log` when it is larger than 1 MB) |
| WebView2 data | `%LOCALAPPDATA%\mdviewer\WebView2` |

`settings.ini` is a text file that you can edit:

```ini
# theme: auto, light or dark
theme = auto
# remote_images: show or block (block: no requests to the internet)
remote_images = show
# history_panel: show or hide (the tree of the files of this session)
history_panel = hide
# history_panel_width: the width of the history panel in pixels (160 to 800)
history_panel_width = 260
# window: normal or maximized (saved when the window closes)
window = normal
```

Only the state and the width of the history panel are saved, never the history itself.
The panel leaves at least 600 px for the document, so that its lines stay readable; in
a smaller window the panel is narrower than its saved width. mdviewer also remembers
whether the window was maximized when you closed it.

The environment variable `MDVIEWER_LOG` sets the log level: `off`, `error`, `warn`,
`info` (default), `debug` or `trace`.

## Privacy and security

A Markdown file can contain HTML, so mdviewer treats every file as untrusted:

- Scripts, frames, plugins and forms in a document are blocked.
- The page loads local **images and media only**, and only from inside the vault,
  repository or document folder.
- Requests to your **local network** are always blocked. With "Remote images: Block",
  no request goes to the internet at all.
- mdviewer never sends your Windows login to an image server, and image servers cannot
  store cookies.
- When a page opens, mdviewer **does not list your folders**. It finds a vault by
  checking the parent folders of the file, and it checks wiki links only at a few exact
  places. Only a click on a faded link searches the vault.
- The link preview shows a web address exactly as mdviewer sends it, not decoded, so
  hidden characters cannot change how the address looks.
- The history panel shows file paths only as text, never in HTML attributes, because
  CSS in a document could read attribute values and send them to a server.

The details, with the reason for each rule, are in
[docs/architecture.md](docs/architecture.md).

## Development

```powershell
cargo test                                   # Rust and JavaScript tests (needs Node.js)
cargo clippy --all-targets -- -D warnings    # also fails for code without doc comments
cargo fmt
```

- GitHub Actions runs the same checks (format, lints, tests, docs) on Windows for each
  push to `main` and each pull request: [.github/workflows/ci.yml](.github/workflows/ci.yml).
- The architecture, the data flow and the design decisions are in
  [docs/architecture.md](docs/architecture.md).
- The third-party libraries (versions, hashes, licenses, update steps) are in
  [assets/vendor/README.md](assets/vendor/README.md).

### Release

1. Set the new version in `Cargo.toml` (for example `version = "0.2.0"`), and commit it.
2. On GitHub, open "Releases" → "Draft a new release". Make the tag `v0.2.0` (the version
   with a `v`), and click "Publish release".
3. [.github/workflows/release.yml](.github/workflows/release.yml) then checks that the
   tag is the version, runs all checks of `ci.yml`, makes the release build, and adds
   two files to the release:
   - `mdviewer-0.2.0-windows-x64.zip`: `mdviewer.exe`, this README, `LICENSE.txt`, and
     in `licenses/` the licenses of the embedded libraries and the notices of the Rust
     crates (made from `Cargo.lock` by `tools/notices.mjs`);
   - `mdviewer-0.2.0-windows-x64.zip.sha256`: the checksum, for `sha256sum -c` or
     `Get-FileHash`.

**Pre-releases** (test builds before a release) need no change of `Cargo.toml`: select
"Set as a pre-release", and add a suffix to the tag, for example `v0.2.0-rc.1`, then
`v0.2.0-rc.2`. The zip name has the suffix (`mdviewer-0.2.0-rc.1-windows-x64.zip`).
When the test is good, publish the release `v0.2.0` without the checkbox. Only a
pre-release can have a suffix. Use a dot (`rc.1`, not `rc1`), so that `rc.10` sorts
after `rc.9`.

If a step fails, the release has no files. Fix the cause, then use "Re-run jobs" in the
"Actions" tab. To make the zip file locally: `cargo build --release --locked`, then
`./.github/scripts/package-release.ps1 -Version 0.2.0` (output in `dist\`; needs
Node.js).

## Limits

- Embedded notes (`![[Note]]`) show as links, not as the note content.
- Obsidian plugins (for example Dataview, Excalidraw, Canvas) and GeoJSON or STL blocks
  are not supported.
- mdviewer is not yet registered as the program for `.md` files in File Explorer.
- Windows only. The system code is in `src/platform/`, so that a port to macOS or Linux
  is possible later; `docs/architecture.md` ("Platforms") lists what a port must do.

## License

mdviewer uses the [MIT license](LICENSE).

The program contains third-party code with its own licenses, all of them permissive:
the browser libraries (MIT, ISC, BSD-3-Clause; the KaTeX fonts use the SIL Open Font
License 1.1; inside Mermaid also Apache-2.0 and, for the ELK layout, the Eclipse Public
License 2.0) and the Rust crates (mostly MIT or Apache-2.0; also BSD-2-Clause, Zlib and
the Unicode license). The details are in
[assets/vendor/README.md](assets/vendor/README.md), and each release zip file has all
license texts in its `licenses/` folder.
