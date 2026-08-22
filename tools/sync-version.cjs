// Cross-platform version sync (Node, no PowerShell dependency).
// Mirrors package.json's "version" into:
//   * src-tauri/tauri.conf.json  — bundle/installer metadata
//   * src-tauri/Cargo.toml       — CARGO_PKG_VERSION, shown in the About dialog
// so all user-visible version strings stay identical. Safe on
// Windows/macOS/Linux runners and on both Node LTS and older versions.
'use strict';

const fs = require('fs');
const path = require('path');

const root = path.resolve(__dirname, '..');
const pkgPath = path.join(root, 'package.json');
const confPath = path.join(root, 'src-tauri', 'tauri.conf.json');
const cargoPath = path.join(root, 'src-tauri', 'Cargo.toml');

const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
const version = String(pkg.version);

let changed = false;

// --- tauri.conf.json -------------------------------------------------------
const confRaw = fs.readFileSync(confPath, 'utf8');
const conf = JSON.parse(confRaw);
if (conf.version !== version) {
  conf.version = version;
  // Preserve a clean 2-space-indent diff.
  fs.writeFileSync(confPath, JSON.stringify(conf, null, 2) + '\n', 'utf8');
  console.log(`synced tauri.conf.json version -> ${version}`);
  changed = true;
}

// --- Cargo.toml ------------------------------------------------------------
// Rewrite only the `[package] version = "..."` line; everything else stays
// byte-identical (no toml round-trip, no comment churn).
const cargoRaw = fs.readFileSync(cargoPath, 'utf8');
const cargoRe = /^(version\s*=\s*")([^"]*)(")/m;
const m = cargoRaw.match(cargoRe);
if (!m) {
  console.error('ERROR: could not find [package] version in Cargo.toml');
  process.exit(1);
}
if (m[2] !== version) {
  fs.writeFileSync(
    cargoPath,
    cargoRaw.replace(cargoRe, `$1${version}$3`),
    'utf8'
  );
  console.log(`synced Cargo.toml version -> ${version}`);
  changed = true;
}

// Cargo.lock must follow Cargo.toml or `cargo build --locked` fails in CI.
const lockPath = path.join(root, 'src-tauri', 'Cargo.lock');
if (fs.existsSync(lockPath)) {
  const { execFileSync } = require('child_process');
  try {
    execFileSync('cargo', ['update', '--quiet', '--workspace', '--offline'], {
      cwd: path.dirname(lockPath),
      stdio: 'inherit',
    });
    console.log('refreshed Cargo.lock via `cargo update --workspace --offline`');
  } catch (e) {
    // Fallback for environments without cargo on PATH (rare): patch the
    // root package's version entry directly.
    const lockRaw = fs.readFileSync(lockPath, 'utf8');
    const pkgName = pkg.name;
    const re = new RegExp(`(name = "${pkgName}"[\\s\\S]*?version = ")([^"]*)(")`);
    if (re.test(lockRaw)) {
      fs.writeFileSync(lockPath, lockRaw.replace(re, `$1${version}$3`), 'utf8');
      console.log(`patched Cargo.lock version -> ${version}`);
    } else {
      console.warn(`WARN: could not patch Cargo.lock (${e.message}); run 'cargo update -w' manually.`);
    }
  }
} else if (!changed) {
  console.log(`version already in sync: ${version}`);
}
