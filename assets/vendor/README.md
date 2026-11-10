# Third-party browser libraries

These files are embedded in `mdviewer.exe` (see `build.rs`) and served at
`https://mdviewer.localhost/_mdviewer/<path>`. `assets/render.mjs` loads each library
only when the page needs it. The icons are CSS masks in `assets/style.css`; a test
checks that every icon that the style sheet names is embedded.

| Folder | Package | Version | License | Files taken |
|---|---|---|---|---|
| `katex/` | `katex` | 0.19.1 | MIT (code); fonts: OFL-1.1 | `dist/katex.min.js`, `dist/katex.min.css`, `dist/fonts/*.woff2` |
| `mermaid/` | `mermaid` | 12.2.0 | MIT; bundled libraries: see below | `dist/mermaid.esm.min.mjs`, `dist/chunks/mermaid.esm.min/*.mjs` |
| `highlight/` | `@highlightjs/cdn-assets` | 11.12.0 | BSD-3-Clause | `es/highlight.min.js` (ESM, common languages) |
| `octicons/` | `@primer/octicons` | 19.42.0 | MIT | `build/svg/{info,light-bulb,report,alert,stop}-16.svg` (icons of GitHub alerts) |
| `lucide/` | `lucide-static` | 1.61.0 | ISC; MIT for the icons from Feather | `icons/{pencil,clipboard-list,info,circle-check,flame,check,circle-help,triangle-alert,x,zap,bug,list,quote,arrow-left,arrow-right,history}.svg` (icons of Obsidian callouts and of the viewer buttons) |

## Licenses

The licenses require that their notices go with each copy of mdviewer. The release zip
file has all of them (`.github/scripts/package-release.ps1`), and `tests/licenses.rs`
checks that they are here.

- Each folder has the `LICENSE` file of its package. The Lucide file also has the MIT
  license of the icons from Feather.
- `katex/OFL.txt`: the KaTeX **fonts** use the SIL Open Font License 1.1, not the MIT
  license of KaTeX's code. The fonts name it themselves (their `name` table: copyright
  and license). The npm package has no OFL text, so the file has the copyright lines of
  the fonts and the OFL text from https://openfontlicense.org/documents/OFL.txt.
  mdviewer does not change the fonts.
- `mermaid/THIRD-PARTY-NOTICES.txt`: the notices of the libraries inside the Mermaid
  files (83 npm packages: MIT, ISC, BSD-3-Clause, Apache-2.0, Unlicense, and):
  - **DOMPurify**: MPL-2.0 OR Apache-2.0; mdviewer uses it under Apache-2.0.
  - **elkjs** (`chunks/mermaid.esm.min/elk-*.mjs`, 1.6 MB, loaded only for
    `layout: elk`): Eclipse Public License 2.0. EPL-2.0 does not change the license
    of mdviewer, but users must be told where the source code is; the notices file
    does this. elkjs is the Java code of ELK, compiled to JavaScript, so the file
    names both: https://github.com/kieler/elkjs (tag 0.9.3) and
    https://github.com/eclipse-elk/elk (v0.9.1 with change #955, from the elkjs 0.9.3
    release notes). For a new elkjs version, find the ELK version on its release page
    and update the note in `REVIEWED_PACKAGES`.
- The Rust crates are not here: the release makes their notices from `Cargo.lock`
  (`node tools/notices.mjs cargo`), and CI checks their licenses on each push.

`tools/notices.mjs` accepts only permissive licenses (`ACCEPTED_LICENSES`). Each other
case needs a review by a person, recorded in `REVIEWED_PACKAGES` with the exact version
(today: elkjs, EPL-2.0; khroma, which has no license field but an MIT license file).

## Integrity (npm `sha512`, checked by `npm pack`)

- katex@0.19.1: `sha512-jG8uKntjSEcm2EpQOh0XturpPuuC9r3Sq5HLHdsFdWZT27E672SF19QEi+of0/YeOy15lDgRhKvr5614uRrEfA==`
- mermaid@12.2.0: `sha512-w7nr/SFrLO7BaVTs3dQTvYSV6nYSBj0Yy9DjBhsIJtAcHqRzXSq4Pg32ujHZ/AdWsAMNEvvhbm5dt9E2tOiHzw==`
- @highlightjs/cdn-assets@11.12.0: `sha512-KvOKXODaiFmId9xaq3xc5xCL66wVLUuOngDbO9B/kewbFTqdGbn2nJxNhN3H5R1cgDTVj6R8vH0zgiNDEGjpDw==`
- @primer/octicons@19.42.0: `sha512-MEcHxVxNHu1I2Ndk5Mcwp7tfBiDk/EpytDJgKOfE2ldqIczWucVrZNeNU4wtEN5mnNudZ0YL5G0spax3bSMT4w==`
- lucide-static@1.61.0: `sha512-yLAiQB3MWUJEstsEDAwenSA3m7vr1tdLvgnC16Q/XMxRRma0DWwMs8M2bWlvgShrIs1m2mWMJ+0AiahqQO4GrA==`

## Update

1. Use a version that was published at least two weeks ago.
2. `npm pack <package>@<version> --pack-destination <new empty folder>`, then
   `tar -xzf` the `.tgz` in that folder.
3. Copy only the files in the table above (no source maps), and the `LICENSE`.
4. For a new KaTeX version: check that the fonts still name the same license and
   copyright (`katex/OFL.txt`).
5. For a new Mermaid version, make the notices again. Use a new empty folder outside
   the repository, and the date when that Mermaid version was published (`npm view
   mermaid time`), plus one minute, so that npm takes the versions of that time:

   ```sh
   echo '{"private":true}' > package.json
   npm install mermaid@<version> --before=<date> --omit=dev --ignore-scripts
   node <repository>/tools/notices.mjs npm . mermaid --out <repository>/assets/vendor/mermaid/THIRD-PARTY-NOTICES.txt
   ```

   `--ignore-scripts`: no package script runs. If the tool reports a license problem,
   decide with the license text, and record the decision in `REVIEWED_PACKAGES`.
6. Update this file, then run `cargo test` (tests check that every file is served and
   that the license files are complete).
