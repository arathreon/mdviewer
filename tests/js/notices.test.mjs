// Tests of tools/notices.mjs (third-party notices and license checks). The tests use
// fake metadata and fake node_modules folders, so they need no network and no npm.

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, test } from 'node:test';
import { fileURLToPath } from 'node:url';

import {
  USAGE,
  cargoPackages,
  isAccepted,
  licenseTexts,
  npmLicense,
  npmPackages,
  parseArgs,
  problems,
  render,
  withReviews,
} from '../../tools/notices.mjs';

/** The tool itself, for the tests of the command line. */
const TOOL = fileURLToPath(new URL('../../tools/notices.mjs', import.meta.url));

/** Temporary folders of the current test; removed after each test. */
const folders = [];
afterEach(() => {
  for (const dir of folders.splice(0)) rmSync(dir, { recursive: true, force: true });
});

/** Makes a temporary folder with difficult characters in its name (see test_util.rs). */
function tempDir() {
  const dir = mkdtempSync(join(tmpdir(), "mdviewer test 'é&' notices-"));
  folders.push(dir);
  return dir;
}

/** Writes the files of `tree` ({ "a/b.txt": "text" }) below `dir`. */
function writeTree(dir, tree) {
  for (const [path, content] of Object.entries(tree)) {
    const file = join(dir, path);
    mkdirSync(join(file, '..'), { recursive: true });
    writeFileSync(file, typeof content === 'string' ? content : JSON.stringify(content));
  }
}

test('accepted licenses and expressions', () => {
  for (const ok of [
    'MIT',
    'MIT OR Apache-2.0',
    '(MIT OR Apache-2.0) AND Unicode-3.0',
    'MPL-2.0 OR Apache-2.0', // one accepted choice is enough
    '(MPL-2.0 OR Apache-2.0)',
    'MIT/Apache-2.0', // the old Cargo form
    'Apache-2.0 WITH LLVM-exception',
    'Zlib or Apache-2.0 or MIT', // letter case of the operators
  ]) {
    assert.equal(isAccepted(ok), true, ok);
  }
});

test('rejected licenses and broken expressions', () => {
  for (const bad of [
    undefined,
    '',
    'GPL-3.0-only',
    'MPL-2.0',
    'MIT AND GPL-2.0-only', // both apply, one is not accepted
    'EPL-2.0',
    'SEE LICENSE IN LICENSE.txt',
    'MIT OR',
    '(MIT',
    'MIT)',
    'GPL-2.0-only WITH LLVM-exception',
    'MIT WITH Unknown-exception',
  ]) {
    assert.equal(isAccepted(bad), false, String(bad));
  }
});

test('npm license fields of all forms', () => {
  assert.equal(npmLicense({ license: 'ISC' }), 'ISC');
  assert.equal(npmLicense({ license: { type: 'MIT' } }), 'MIT');
  assert.equal(npmLicense({ licenses: [{ type: 'MIT' }, { type: 'Apache-2.0' }] }), 'MIT OR Apache-2.0');
  assert.equal(npmLicense({}), undefined);
});

/** A package of fake `cargo metadata` output. */
function crate(name, { kinds = ['lib'], dir = `/registry/${name}` } = {}) {
  return {
    id: `${name}-id`,
    name,
    version: '1.0.0',
    license: 'MIT',
    repository: `https://example.com/${name}`,
    manifest_path: `${dir}/Cargo.toml`,
    targets: [{ kind: kinds }],
  };
}

/** A dependency edge of fake `cargo metadata` output. */
function dep(name, kind = null) {
  return { pkg: `${name}-id`, dep_kinds: [{ kind }] };
}

test('cargo: only crates that are in the program', () => {
  const metadata = {
    packages: ['root', 'a', 'b', 'build', 'dev', 'syn', 'shared'].map((n) => crate(n))
      .concat(crate('macro', { kinds: ['proc-macro'] })),
    resolve: {
      root: 'root-id',
      nodes: [
        { id: 'root-id', deps: [dep('a'), dep('build', 'build'), dep('dev', 'dev'), dep('macro')] },
        { id: 'a-id', deps: [dep('b'), dep('shared')] },
        { id: 'b-id', deps: [dep('a')] }, // a circle must not loop
        { id: 'macro-id', deps: [dep('syn'), dep('shared')] },
        ...['build', 'dev', 'syn', 'shared'].map((n) => ({ id: `${n}-id`, deps: [] })),
      ],
    },
  };
  const names = cargoPackages(metadata).map((p) => p.name);
  // "shared" is also below the proc-macro, but "a" uses it in the program.
  assert.deepEqual(names, ['a', 'b', 'shared']);
  assert.equal(cargoPackages(metadata)[0].dir, '/registry/a');
});

test('npm: dependencies are found as Node.js finds them', () => {
  const dir = tempDir();
  writeTree(dir, {
    'node_modules/root/package.json': {
      name: 'root', version: '1.0.0', license: 'MIT',
      dependencies: { y: '^1', '@types/z': '^1' },
      optionalDependencies: { missing: '^1' },
    },
    'node_modules/y/package.json': {
      name: 'y', version: '1.0.0', license: 'ISC', dependencies: { w: '^2' },
      repository: { type: 'git', url: 'https://example.com/y.git' },
    },
    // y needs w 2 and has it in its own node_modules; the top w 1 is not used.
    'node_modules/y/node_modules/w/package.json': { name: 'w', version: '2.0.0', license: 'MIT' },
    'node_modules/w/package.json': { name: 'w', version: '1.0.0', license: 'MIT' },
    'node_modules/@types/z/package.json': { name: '@types/z', version: '1.0.0', license: 'MIT' },
  });
  const found = npmPackages(dir, 'root');
  assert.deepEqual(found.map((p) => `${p.name}@${p.version}`), ['w@2.0.0', 'y@1.0.0']);
  assert.equal(found[1].repository, 'https://example.com/y.git');
});

test('npm: a missing required dependency is an error', () => {
  const dir = tempDir();
  writeTree(dir, {
    'node_modules/root/package.json': { name: 'root', version: '1.0.0', dependencies: { gone: '^1' } },
  });
  assert.throws(() => npmPackages(dir, 'root'), /gone \(for root\) is not installed/);
  assert.throws(() => npmPackages(dir, 'other'), /other is not installed/);
});

test('license texts: license files only, normalized, else the extra text', () => {
  const dir = tempDir();
  writeTree(dir, {
    'pkg/LICENSE-MIT': '\uFEFFMIT text  \r\nline 2\r\n\r\n',
    'pkg/COPYING': 'copying',
    'pkg/license.rs': 'source code, not a license',
    'pkg/README.md': 'readme',
    'pkg/sub/LICENSE': 'not in the top folder',
    'empty/Cargo.toml': '',
    'extra/webview2-rs.txt': 'extra text\n',
  });
  assert.deepEqual(licenseTexts({ name: 'pkg', dir: join(dir, 'pkg') }, join(dir, 'extra')), [
    { file: 'COPYING', text: 'copying' },
    { file: 'LICENSE-MIT', text: 'MIT text\nline 2' },
  ]);
  assert.deepEqual(licenseTexts({ name: 'webview2-com', dir: join(dir, 'empty') }, join(dir, 'extra')), [
    { file: 'webview2-rs.txt (from the repository of the package)', text: 'extra text' },
  ]);
  assert.deepEqual(licenseTexts({ name: 'unknown', dir: join(dir, 'empty') }, join(dir, 'extra')), []);
});

test('reviews fill in a license and accept one license for one version', () => {
  const reviews = {
    'nolicense@1.0.0': { license: 'MIT', note: 'license file is MIT' },
    'epl@2.0.0': { accept: 'EPL-2.0', note: 'source at example.com' },
  };
  const packages = withReviews([
    { name: 'nolicense', version: '1.0.0' },
    { name: 'epl', version: '2.0.0', license: 'EPL-2.0' },
    { name: 'epl', version: '3.0.0', license: 'EPL-2.0' }, // a new version needs a new review
    { name: 'plain', version: '1.0.0', license: 'MIT' },
  ], reviews);
  assert.equal(packages[0].license, 'MIT');
  assert.equal(packages[1].note, 'source at example.com');
  const withText = () => [{ file: 'LICENSE', text: 'x' }];
  assert.deepEqual(problems(packages, withText), ['epl 3.0.0: license "EPL-2.0" is not accepted']);
  assert.deepEqual(problems([{ name: 'p', version: '1.0.0', license: 'MIT' }], () => []), [
    'p 1.0.0: no license text (add one to EXTRA_LICENSE_TEXTS)',
  ]);
});

test('render: list, notes, shared texts, and no folder paths', () => {
  const secretDir = 'C:\\Users\\someone\\.cargo\\registry\\src\\a-1.0.0';
  const packages = [
    { name: 'a', version: '1.0.0', license: 'MIT', repository: 'https://example.com/a', dir: secretDir },
    { name: 'b', version: '2.0.0', license: 'MIT', dir: secretDir, note: 'checked by hand' },
    { name: 'c', version: '3.0.0', license: 'ISC', dir: secretDir },
  ];
  const texts = { a: 'MIT text', b: 'MIT text', c: 'ISC text' };
  const text = render({
    title: 'Title',
    intro: 'Intro.',
    packages,
    textsOf: (pkg) => [{ file: 'LICENSE', text: texts[pkg.name] }],
  });
  assert.ok(text.startsWith('Title\n=====\n\nIntro.\n'), text);
  assert.ok(text.includes('a 1.0.0  MIT  https://example.com/a\n'), text);
  assert.ok(text.includes('b 2.0.0  MIT\n    Note: checked by hand\n'), text);
  // a and b have the same text: one copy.
  assert.ok(text.includes('=== a 1.0.0 (LICENSE), b 2.0.0 (LICENSE)\n\nMIT text\n'), text);
  assert.equal(text.split('MIT text').length - 1, 1, text);
  assert.ok(text.includes('=== c 3.0.0 (LICENSE)\n\nISC text\n'), text);
  assert.ok(!text.includes('Users') && !text.includes('.cargo'), text);
  assert.ok(!text.includes('\r') && text.endsWith('\n'), text);
});

test('command line arguments: options at any position, errors are not guessed', () => {
  // The first case failed in CI: without --out, the old code removed the first argument.
  assert.deepEqual(parseArgs(['cargo', '--check']), { mode: 'cargo', check: true, outFile: undefined });
  assert.deepEqual(parseArgs(['--check', 'cargo']), { mode: 'cargo', check: true, outFile: undefined });
  assert.deepEqual(parseArgs(['cargo']), { mode: 'cargo', check: false, outFile: undefined });
  assert.deepEqual(parseArgs(['cargo', '--out', 'a.txt']), { mode: 'cargo', check: false, outFile: 'a.txt' });
  assert.deepEqual(parseArgs(['--out', 'a.txt', 'cargo']), { mode: 'cargo', check: false, outFile: 'a.txt' });
  assert.deepEqual(parseArgs(['npm', 'dir', 'mermaid', '--check']), {
    mode: 'npm', projectDir: 'dir', rootName: 'mermaid', check: true, outFile: undefined,
  });
  assert.deepEqual(parseArgs(['npm', '--out', 'b.txt', 'dir', 'mermaid']), {
    mode: 'npm', projectDir: 'dir', rootName: 'mermaid', check: false, outFile: 'b.txt',
  });
  for (const [args, error] of [
    [[], 'wrong arguments'],
    [['--check'], 'wrong arguments'],
    [['cargo', 'extra'], 'wrong arguments'],
    [['npm', 'dir'], 'wrong arguments'],
    [['npm', 'dir', 'mermaid', 'extra'], 'wrong arguments'],
    [['rust'], 'wrong arguments'],
    [['cargo', '--out'], '--out needs a file name'],
    [['cargo', '--out', '--check'], '--out needs a file name'],
    [['cargo', '--chek'], 'unknown option --chek'],
    [['cargo', '--check', '--out', 'a.txt'], 'use --check or --out, not both'],
  ]) {
    assert.deepEqual(parseArgs(args), { error }, JSON.stringify(args));
  }
});

/** Runs the tool with `args` in a separate Node.js process. */
function runTool(args) {
  return spawnSync(process.execPath, [TOOL, ...args], { encoding: 'utf8' });
}

/** Makes a fake npm project: "root" needs "y" (ISC, with its license file). */
function npmProject() {
  const dir = tempDir();
  writeTree(dir, {
    'node_modules/root/package.json': { name: 'root', version: '1.0.0', license: 'MIT', dependencies: { y: '^1' } },
    'node_modules/y/package.json': { name: 'y', version: '1.2.3', license: 'ISC' },
    'node_modules/y/LICENSE': 'ISC License\n\nCopyright (c) y authors\n',
  });
  return dir;
}

test('command line: --check, --out, and wrong arguments', () => {
  const dir = npmProject();

  const checked = runTool(['npm', dir, 'root', '--check']);
  assert.equal(checked.status, 0, checked.stderr);
  assert.equal(checked.stdout, '');
  assert.match(checked.stderr, /^1 packages, all licenses accepted\.\n$/);

  // The output file is in a folder with difficult characters, and has UTF-8 and LF only.
  const out = join(dir, 'notices é&.txt');
  const written = runTool(['npm', dir, 'root', '--out', out]);
  assert.equal(written.status, 0, written.stderr);
  const text = readFileSync(out, 'utf8');
  assert.ok(text.startsWith('Third-party notices: libraries inside root\n'), text);
  assert.ok(text.includes('\ny 1.2.3  ISC\n'), text);
  assert.ok(text.includes('Copyright (c) y authors'), text);
  assert.ok(!text.includes('\r') && !text.includes(dir), text);

  const wrong = runTool(['cargo', 'extra']);
  assert.equal(wrong.status, 2);
  assert.equal(wrong.stderr, `wrong arguments\n${USAGE}`);
});
