# Third-party browser libraries

These files are embedded in `mdviewer.exe` (see `build.rs`) and served at
`https://mdviewer.localhost/_mdviewer/<path>`. `assets/render.mjs` loads each library
only when the page needs it. The icons are CSS masks in `assets/style.css`; a test
checks that every icon that the style sheet names is embedded.

| Folder | Package | Version | License | Files taken |
|---|---|---|---|---|
| `katex/` | `katex` | 0.19.1 | MIT | `dist/katex.min.js`, `dist/katex.min.css`, `dist/fonts/*.woff2` |
| `mermaid/` | `mermaid` | 12.2.0 | MIT | `dist/mermaid.esm.min.mjs`, `dist/chunks/mermaid.esm.min/*.mjs` |
| `highlight/` | `@highlightjs/cdn-assets` | 11.12.0 | BSD-3-Clause | `es/highlight.min.js` (ESM, common languages) |
| `octicons/` | `@primer/octicons` | 19.42.0 | MIT | `build/svg/{info,light-bulb,report,alert,stop}-16.svg` (icons of GitHub alerts) |
| `lucide/` | `lucide-static` | 1.61.0 | ISC | `icons/{pencil,clipboard-list,info,circle-check,flame,check,circle-help,triangle-alert,x,zap,bug,list,quote,arrow-left,arrow-right,history}.svg` (icons of Obsidian callouts and of the viewer buttons) |

Each folder also has the `LICENSE` file of its package. Mermaid bundles other open
source libraries (for example d3, dagre, DOMPurify); their notices are in the package.

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
4. Update this file, then run `cargo test` (a test checks that every file is served).
