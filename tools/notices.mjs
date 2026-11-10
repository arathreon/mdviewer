// Makes third-party notice files: the names, versions, licenses and license texts of the
// packages whose code is in mdviewer, and checks that each license is acceptable.
//
//   node tools/notices.mjs cargo [--check | --out <file>]
//       The Rust crates in mdviewer.exe (from `cargo metadata`, so from Cargo.lock).
//       The release puts the output into the zip file (licenses/rust-crates.txt).
//   node tools/notices.mjs npm <project folder> <package> [--check | --out <file>]
//       The npm packages inside <package> (its runtime dependencies, without the
//       package itself), from <project folder>/node_modules. Used for Mermaid
//       (assets/vendor/mermaid/THIRD-PARTY-NOTICES.txt; see assets/vendor/README.md).
//
// The text goes to the standard output (UTF-8, LF line ends), or with --out <file> into
// that file (safer from PowerShell, which can change the characters of the standard
// output). With --check, the tool only checks the licenses. If a license is not acceptable, or a package has no license text, the
// tool writes the problems to the standard error and exits with code 1.
//
// The output contains no folder paths (the crates are below the user's home folder).
// The tool only reads files.

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** The folder of this file. */
const TOOLS_DIR = dirname(fileURLToPath(import.meta.url));

/**
 * The licenses that mdviewer can contain without a separate decision (SPDX ids). All are
 * permissive: they ask only for the notice. For "A OR B", one of them is enough. A
 * license that is not here (for example GPL, LGPL, or MPL-2.0 alone) stops the release,
 * so that a person decides.
 */
export const ACCEPTED_LICENSES = new Set([
  '0BSD',
  'Apache-2.0',
  'BSD-2-Clause',
  'BSD-3-Clause',
  'BSL-1.0',
  'CC0-1.0',
  'ISC',
  'MIT',
  'MIT-0',
  'Unicode-3.0',
  'Unicode-DFS-2016',
  'Unlicense',
  'Zlib',
]);

/** The license exceptions ("X WITH Y") that change nothing for mdviewer. */
export const ACCEPTED_EXCEPTIONS = new Set(['LLVM-exception']);

/**
 * Packages that a person checked by hand, by "name@version". The version is part of the
 * key, so that a new version stops the check again and gets a new review.
 *
 * - `license`: the license when the package metadata has none (from its license file).
 * - `accept`: a license that is not in ACCEPTED_LICENSES but is acceptable for this
 *   package, because mdviewer does what `note` says.
 * - `note`: the reason; it is printed with the package in the notices.
 */
export const REVIEWED_PACKAGES = {
  'khroma@2.1.0': {
    license: 'MIT',
    note: 'package.json has no "license" field; the license file of the package is MIT.',
  },
  'elkjs@0.9.3': {
    accept: 'EPL-2.0',
    note:
      'Eclipse Public License 2.0. mdviewer contains elkjs unchanged, inside Mermaid ' +
      '(chunks/mermaid.esm.min/elk-*.mjs). The source code of elkjs 0.9.3 is available ' +
      'at https://github.com/kieler/elkjs (tag 0.9.3) and on npm (elkjs@0.9.3).',
  },
};

/** File names of license texts in a package folder (only the top folder is read). */
const LICENSE_FILE = /^(licen[cs]e|copying|copyright|notice)/i;

/** Source files with a name like a license file are not license texts. */
const SOURCE_FILE = /\.(rs|js|mjs|cjs|ts|json)$/i;

/**
 * License texts for packages that have none in their folder, by package name. Each value
 * is a file in tools/license-texts/, copied from the repository of the package.
 */
export const EXTRA_LICENSE_TEXTS = {
  'webview2-com': 'webview2-rs.txt',
  'webview2-com-macros': 'webview2-rs.txt',
  'webview2-com-sys': 'webview2-rs.txt',
};

/**
 * Checks an SPDX license expression, for example "(MIT OR Apache-2.0) AND Unicode-3.0".
 * "A/B" (the old Cargo form) means "A OR B". Letter case of AND, OR and WITH is ignored.
 *
 * @param {string | undefined} expression The expression from package metadata.
 * @returns {boolean} `true` if the licenses that apply can all be accepted licenses.
 */
export function isAccepted(expression) {
  if (typeof expression !== 'string' || expression.trim() === '') return false;
  const tokens = expression.replaceAll('/', ' OR ').match(/\(|\)|[^\s()]+/g) ?? [];
  let at = 0;
  const peek = () => tokens[at]?.toUpperCase();
  // or := and ("OR" and)* ; and := atom ("AND" atom)* ;
  // atom := "(" or ")" | id ("WITH" id)?
  const or = () => {
    let ok = and();
    while (peek() === 'OR') {
      at += 1;
      ok = and() || ok;
    }
    return ok;
  };
  const and = () => {
    let ok = atom();
    while (peek() === 'AND') {
      at += 1;
      ok = atom() && ok;
    }
    return ok;
  };
  const atom = () => {
    const token = tokens[at++];
    if (token === '(') {
      const ok = or();
      if (tokens[at++] !== ')') throw new SyntaxError('missing )');
      return ok;
    }
    if (token === undefined || [')', 'AND', 'OR', 'WITH'].includes(token.toUpperCase())) {
      throw new SyntaxError(`unexpected ${token}`);
    }
    if (peek() === 'WITH') {
      at += 1;
      const exception = tokens[at++];
      return ACCEPTED_LICENSES.has(token) && ACCEPTED_EXCEPTIONS.has(exception);
    }
    return ACCEPTED_LICENSES.has(token);
  };
  try {
    const ok = or();
    return ok && at === tokens.length;
  } catch {
    return false;
  }
}

/**
 * The npm "license" field as an SPDX expression. Old packages use `{ type }` or an array
 * "licenses" of `{ type }`; an array means that the user can choose.
 *
 * @param {object} manifest The content of package.json.
 * @returns {string | undefined} The expression, or `undefined` if the package has none.
 */
export function npmLicense(manifest) {
  const { license, licenses } = manifest;
  if (typeof license === 'string') return license;
  if (license && typeof license.type === 'string') return license.type;
  if (Array.isArray(licenses) && licenses.length > 0) {
    return licenses.map((entry) => entry.type ?? entry).join(' OR ');
  }
  return undefined;
}

/**
 * The crates whose code is in the program: the root package and all crates that it
 * reaches through normal dependencies. Build and dev dependencies are not in the
 * program. Proc-macro crates run only in the compiler, so they and the crates below
 * them (for example syn) are not in the program either.
 *
 * @param {object} metadata The output of `cargo metadata --format-version 1`.
 * @returns {Array<{name: string, version: string, license?: string,
 *   repository?: string, dir: string}>} The crates without the root, sorted by name.
 */
export function cargoPackages(metadata) {
  const packages = new Map(metadata.packages.map((p) => [p.id, p]));
  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
  const isProcMacro = (p) => p.targets.some((t) => t.kind.includes('proc-macro'));
  const root = metadata.resolve.root;
  const seen = new Set([root]);
  const todo = [root];
  while (todo.length > 0) {
    for (const dep of nodes.get(todo.pop()).deps) {
      const normal = dep.dep_kinds.some((kind) => kind.kind === null);
      if (normal && !seen.has(dep.pkg) && !isProcMacro(packages.get(dep.pkg))) {
        seen.add(dep.pkg);
        todo.push(dep.pkg);
      }
    }
  }
  seen.delete(root);
  return sortPackages(
    [...seen].map((id) => {
      const p = packages.get(id);
      return {
        name: p.name,
        version: p.version,
        license: p.license ?? undefined,
        repository: p.repository ?? undefined,
        dir: dirname(p.manifest_path),
      };
    }),
  );
}

/**
 * The npm packages inside `rootName`: its runtime dependencies ("dependencies" and the
 * installed "optionalDependencies"), and theirs, found as Node.js finds them (the
 * nearest node_modules folder first). Type packages (@types/...) contain no code and
 * are left out.
 *
 * @param {string} projectDir The folder with the node_modules folder.
 * @param {string} rootName The package, for example "mermaid".
 * @returns {Array<{name: string, version: string, license?: string,
 *   repository?: string, dir: string}>} The packages without the root, sorted by name.
 * @throws {Error} If a dependency is not installed.
 */
export function npmPackages(projectDir, rootName) {
  const top = resolve(projectDir);
  const find = (fromDir, name) => {
    for (let dir = fromDir; ; dir = dirname(dir)) {
      const candidate = join(dir, 'node_modules', name);
      if (existsSync(join(candidate, 'package.json'))) return candidate;
      if (dir === top || dirname(dir) === dir) return undefined;
    }
  };
  const rootDir = find(top, rootName);
  if (!rootDir) throw new Error(`${rootName} is not installed in ${projectDir}`);
  const found = new Map();
  const todo = [rootDir];
  while (todo.length > 0) {
    const dir = todo.pop();
    const manifest = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
    const required = Object.keys(manifest.dependencies ?? {});
    const optional = Object.keys(manifest.optionalDependencies ?? {});
    for (const name of [...required, ...optional]) {
      if (name.startsWith('@types/')) continue;
      const depDir = find(dir, name);
      if (!depDir) {
        if (required.includes(name)) throw new Error(`${name} (for ${manifest.name}) is not installed`);
        continue;
      }
      if (found.has(depDir) || depDir === rootDir) continue;
      const depManifest = JSON.parse(readFileSync(join(depDir, 'package.json'), 'utf8'));
      const repository = depManifest.repository?.url ?? depManifest.repository;
      found.set(depDir, {
        name: depManifest.name,
        version: depManifest.version,
        license: npmLicense(depManifest),
        repository: typeof repository === 'string' ? repository : undefined,
        dir: depDir,
      });
      todo.push(depDir);
    }
  }
  return sortPackages([...found.values()]);
}

/**
 * Adds the reviews of REVIEWED_PACKAGES: a missing license is filled in, and the
 * accepted license and the note are added.
 *
 * @param {object[]} packages The packages from cargoPackages or npmPackages.
 * @param {object} [reviews] The reviews (for tests).
 * @returns {object[]} New package objects, in the same order.
 */
export function withReviews(packages, reviews = REVIEWED_PACKAGES) {
  return packages.map((pkg) => {
    const review = reviews[`${pkg.name}@${pkg.version}`];
    if (!review) return pkg;
    return { ...pkg, license: pkg.license ?? review.license, accept: review.accept, note: review.note };
  });
}

/** Sorts packages by name, then by version, so that the output is always the same. */
function sortPackages(list) {
  return list.sort((a, b) => a.name.localeCompare(b.name, 'en') || a.version.localeCompare(b.version, 'en'));
}

/**
 * Reads the license texts of a package: the license files in its folder, else the text
 * from tools/license-texts/ (EXTRA_LICENSE_TEXTS).
 *
 * @param {{name: string, dir: string}} pkg The package.
 * @param {string} [extraDir] The folder of the extra texts (for tests).
 * @returns {Array<{file: string, text: string}>} The texts, sorted by file name.
 */
export function licenseTexts(pkg, extraDir = join(TOOLS_DIR, 'license-texts')) {
  const files = readdirSync(pkg.dir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && LICENSE_FILE.test(entry.name) && !SOURCE_FILE.test(entry.name))
    .map((entry) => ({ file: entry.name, text: readFileSync(join(pkg.dir, entry.name), 'utf8') }));
  const extra = EXTRA_LICENSE_TEXTS[pkg.name];
  if (files.length === 0 && extra) {
    files.push({ file: `${extra} (from the repository of the package)`, text: readFileSync(join(extraDir, extra), 'utf8') });
  }
  return files
    .map(({ file, text }) => ({ file, text: normalize(text) }))
    .sort((a, b) => a.file.localeCompare(b.file, 'en'));
}

/** LF line ends, no spaces at line ends, no empty lines at the start and the end. */
function normalize(text) {
  return text
    .replace(/^﻿/, '')
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((line) => line.trimEnd())
    .join('\n')
    .trim();
}

/**
 * Finds the problems: a license that is not accepted, or a package without a license
 * text.
 *
 * @param {Array<{name: string, version: string, license?: string, accept?: string}>}
 *   packages The packages (after withReviews).
 * @param {(pkg: object) => Array<{file: string, text: string}>} textsOf Gives the texts.
 * @returns {string[]} One line for each problem; empty if there is none.
 */
export function problems(packages, textsOf) {
  const out = [];
  for (const pkg of packages) {
    const reviewed = pkg.accept !== undefined && pkg.accept === pkg.license;
    if (!reviewed && !isAccepted(pkg.license)) {
      out.push(`${pkg.name} ${pkg.version}: license "${pkg.license ?? '(none)'}" is not accepted`);
    }
    if (textsOf(pkg).length === 0) {
      out.push(`${pkg.name} ${pkg.version}: no license text (add one to EXTRA_LICENSE_TEXTS)`);
    }
  }
  return out;
}

/**
 * Makes the notice text: a title, an introduction, the list of packages, and the
 * license texts. Packages with the same text share one copy of it.
 *
 * @param {{title: string, intro: string, packages: object[],
 *   textsOf: (pkg: object) => Array<{file: string, text: string}>}} input The content.
 * @returns {string} The text, with LF line ends and a line end at the end.
 */
export function render({ title, intro, packages, textsOf }) {
  const lines = [title, '='.repeat(title.length), '', intro, '', 'Packages', '--------', ''];
  for (const pkg of packages) {
    lines.push(`${pkg.name} ${pkg.version}  ${pkg.license ?? '(no license)'}${pkg.repository ? `  ${pkg.repository}` : ''}`);
    if (pkg.note) lines.push(`    Note: ${pkg.note}`);
  }
  lines.push('', 'License texts', '-------------');
  // Text -> the packages (and their file names) that have this text.
  const groups = new Map();
  for (const pkg of packages) {
    for (const { file, text } of textsOf(pkg)) {
      if (!groups.has(text)) groups.set(text, []);
      groups.get(text).push(`${pkg.name} ${pkg.version} (${file})`);
    }
  }
  for (const [text, users] of groups) {
    lines.push('', `=== ${users.join(', ')}`, '', text);
  }
  return `${lines.join('\n')}\n`;
}

/** The host target of rustc, for example "x86_64-pc-windows-msvc". */
function hostTarget() {
  const info = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  return /^host: (.+)$/m.exec(info)[1].trim();
}

/** Runs the command line (see the top of this file). Gives the exit code. */
function main(args) {
  const check = args.includes('--check');
  const outAt = args.indexOf('--out');
  const outFile = outAt >= 0 ? args[outAt + 1] : undefined;
  if (outAt >= 0 && !outFile) {
    process.stderr.write('--out needs a file name\n');
    return 2;
  }
  const [mode, ...rest] = args.filter((arg, i) => arg !== '--check' && i !== outAt && i !== outAt + 1);
  let title;
  let intro;
  let packages;
  if (mode === 'cargo') {
    const json = execFileSync(
      'cargo',
      ['metadata', '--format-version', '1', '--locked', '--filter-platform', hostTarget()],
      { cwd: join(TOOLS_DIR, '..'), encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
    );
    packages = cargoPackages(JSON.parse(json));
    title = 'Third-party notices: Rust crates in mdviewer.exe';
    intro =
      'mdviewer.exe contains code of the Rust crates below. Each line gives the crate,\n' +
      'its version, its license (an SPDX expression; "A OR B" means that you can choose)\n' +
      'and its repository. The license texts follow; crates with the same text share one\n' +
      'copy. Made by tools/notices.mjs from Cargo.lock.';
  } else if (mode === 'npm' && rest.length === 2) {
    const [projectDir, rootName] = rest;
    packages = npmPackages(projectDir, rootName);
    title = `Third-party notices: libraries inside ${rootName}`;
    intro =
      `The files of ${rootName} in mdviewer contain code of the npm packages below (its\n` +
      'runtime dependencies). Each line gives the package, its version, its license (an\n' +
      'SPDX expression; "A OR B" means that you can choose) and its repository. The\n' +
      'license texts follow; packages with the same text share one copy. Made by\n' +
      'tools/notices.mjs (see assets/vendor/README.md).\n\n' +
      `The versions are those that npm installed for ${rootName}. The ${rootName} files were\n` +
      'built from their own lock file, so they can contain other patch versions (their\n' +
      'remaining license comments name them, for example DOMPurify). The licenses are the\n' +
      'same.';
  } else {
    process.stderr.write('usage: node tools/notices.mjs cargo [--check | --out <file>]\n' +
      '       node tools/notices.mjs npm <project folder> <package> [--check | --out <file>]\n');
    return 2;
  }
  packages = withReviews(packages);
  const textsOf = (pkg) => licenseTexts(pkg);
  const found = problems(packages, textsOf);
  if (found.length > 0) {
    process.stderr.write(`License problems:\n${found.map((line) => `  ${line}\n`).join('')}`);
    return 1;
  }
  if (check) {
    process.stderr.write(`${packages.length} packages, all licenses accepted.\n`);
  } else if (outFile) {
    writeFileSync(outFile, render({ title, intro, packages, textsOf }), 'utf8');
  } else {
    process.stdout.write(render({ title, intro, packages, textsOf }));
  }
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
