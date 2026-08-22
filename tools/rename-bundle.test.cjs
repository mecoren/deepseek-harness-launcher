// Unit tests for the release-asset naming contract in rename-bundle.cjs.
// Run: npm test   (node --test tools/)
'use strict';

const test = require('node:test');
const assert = require('node:assert');
const { canonicalName, getOsArch, getVariant, tripleFromDir } = require('./rename-bundle.cjs');

test('canonicalName without variant', () => {
  assert.strictEqual(
    canonicalName('DeepSeek Harness Launcher', '0.1.0', 'Windows', 'Amd64', null, 'msi'),
    'DeepSeek Harness Launcher-0.1.0-Windows-Amd64.msi'
  );
});

test('canonicalName with variant', () => {
  assert.strictEqual(
    canonicalName('DeepSeek Harness Launcher', '0.1.0', 'Windows', 'Amd64', 'Installer', 'msi'),
    'DeepSeek Harness Launcher-0.1.0-Windows-Amd64-Installer.msi'
  );
});

test('getOsArch from extension alone', () => {
  assert.deepStrictEqual(getOsArch('some-app_0.2.0_amd64.deb', ''), ['Linux', 'Amd64']);
  assert.deepStrictEqual(getOsArch('setup.exe', ''), ['Windows', 'Amd64']);
  assert.deepStrictEqual(getOsArch('app.dmg', ''), ['MacOS', 'Amd64']);
});

test('getOsArch prefers explicit arch hints over triple fallback', () => {
  assert.deepStrictEqual(
    getOsArch('launcher-arm64.exe', 'x86_64-pc-windows-msvc'),
    ['Windows', 'Arm64']
  );
});

test('getOsArch uses triple hint when filename lacks arch', () => {
  assert.deepStrictEqual(
    getOsArch('bundle.dmg', 'aarch64-apple-darwin'),
    ['MacOS', 'Arm64']
  );
  assert.deepStrictEqual(
    getOsArch('bundle.msi', 'x86_64-pc-windows-msvc'),
    ['Windows', 'Amd64']
  );
});

test('getVariant mapping', () => {
  assert.strictEqual(getVariant('whatever.msi', 'msi'), 'Installer');
  assert.strictEqual(getVariant('DeepSeek Setup.exe', 'exe'), 'Installer');
  assert.strictEqual(getVariant('DeepSeek.exe', 'exe'), 'Portable');
  assert.strictEqual(getVariant('portable.zip', 'zip'), 'Portable');
  assert.strictEqual(getVariant('other.zip', 'zip'), null);
  assert.strictEqual(getVariant('x.deb', 'deb'), null);
});

test('tripleFromDir extracts cross-compile triple', () => {
  const dir = ['C:', 'proj', 'target', 'x86_64-pc-windows-msvc', 'release', 'bundle', 'nsis'].join('\\');
  assert.strictEqual(tripleFromDir(dir), 'x86_64-pc-windows-msvc');
});

test('tripleFromDir handles posix paths and native layout', () => {
  assert.strictEqual(
    tripleFromDir('/home/r/target/aarch64-unknown-linux-gnu/release/bundle/deb'),
    'aarch64-unknown-linux-gnu'
  );
  // Native build (no triple segment) falls back to ''.
  assert.strictEqual(tripleFromDir('/home/r/target/release/bundle/deb'), '');
});
