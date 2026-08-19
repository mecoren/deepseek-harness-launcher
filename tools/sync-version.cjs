// Cross-platform version sync (Node, no PowerShell dependency).
// Mirrors package.json's "version" into src-tauri/tauri.conf.json so the
// produced bundles carry the correct version. Safe on Windows/macOS/Linux
// runners and on both Node LTS and older versions (no ESM, no TLA).
'use strict';

const fs = require('fs');
const path = require('path');

const root = path.resolve(__dirname, '..');
const pkgPath = path.join(root, 'package.json');
const confPath = path.join(root, 'src-tauri', 'tauri.conf.json');

const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
const version = String(pkg.version);

const confRaw = fs.readFileSync(confPath, 'utf8');
const conf = JSON.parse(confRaw);

if (conf.version === version) {
  console.log(`version already in sync: ${version}`);
  process.exit(0);
}

conf.version = version;
// Preserve the original file formatting style (2-space indent) and a trailing
// newline so the diff stays clean.
fs.writeFileSync(confPath, JSON.stringify(conf, null, 2) + '\n', 'utf8');
console.log(`synced tauri.conf.json version -> ${version}`);
