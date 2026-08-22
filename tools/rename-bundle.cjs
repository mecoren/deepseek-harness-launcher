// Cross-platform bundle renamer (Node, no PowerShell dependency).
// Renames Tauri build artifacts into the canonical naming scheme:
//   {Name}-{version}-{OS}-{Arch}[-{Variant}].{ext}
//
//   Windows : DeepSeek Harness Launcher-0.1.0-Windows-Amd64-Installer.msi
//             DeepSeek Harness Launcher-0.1.0-Windows-Amd64-Portable.exe
//   Linux   : DeepSeek Harness Launcher-0.1.0-Linux-Amd64.deb
//             DeepSeek Harness Launcher-0.1.0-Linux-Amd64.rpm
//             DeepSeek Harness Launcher-0.1.0-Linux-Amd64.tar.gz
//             DeepSeek Harness Launcher-0.1.0-Linux-Amd64-WebKit41.tar.gz
//   macOS   : DeepSeek Harness Launcher-0.1.0-MacOS-Amd64.dmg
//             DeepSeek Harness Launcher-0.1.0-MacOS-Amd64.app.zip
//
// Works on Windows / macOS / Linux runners. Tauri emits bundles under
// target/{triple}/release/bundle/... (cross-compile) or target/release/bundle/...
// (native). This script discovers every bundle dir, reads version + productName
// dynamically from tauri.conf.json, and rewrites each installer to an OS/arch/
// variant-explicit name matching the GitHub Release asset style. `productName`
// is preserved verbatim (including spaces) — the human-readable app name
// "DeepSeek Harness Launcher" is what users expect to see in the asset filename.
//
// Pure helpers (canonicalName/getOsArch/getVariant/tripleFromDir) are exported
// so tools/rename-bundle.test.cjs can pin the release-asset naming contract.
'use strict';

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const root = path.resolve(__dirname, '..');
const srcTauri = path.join(root, 'src-tauri');
const targetDir = path.join(srcTauri, 'target');

const artifactExts = ['.exe', '.msi', '.deb', '.rpm', '.dmg', '.appimage'];

function canonicalName(base, ver, os, arch, variant, ext) {
  const core = `${base}-${ver}-${os}-${arch}`;
  return variant ? `${core}-${variant}.${ext}` : `${core}.${ext}`;
}

function getOsArch(fileName, tripleHint) {
  let os = null;
  let arch = null;
  const lower = fileName.toLowerCase();

  if (/\.(exe|msi)$/.test(lower) || /windows|win32/.test(lower)) os = 'Windows';
  else if (/\.(dmg|app)$/.test(lower) || /macos|darwin/.test(lower)) os = 'MacOS';
  else if (/\.(deb|rpm|appimage|tar\.gz)$/.test(lower) || /linux/.test(lower)) os = 'Linux';

  if (/aarch64|arm64/.test(lower)) arch = 'Arm64';
  else if (/x86_64|amd64/.test(lower)) arch = 'Amd64';
  else if (/\b(x64)\b/.test(lower)) arch = 'Amd64';

  if (!arch && tripleHint) {
    if (/aarch64/.test(tripleHint)) arch = 'Arm64';
    else if (/x86_64/.test(tripleHint)) arch = 'Amd64';
    else if (/x64/.test(tripleHint)) arch = 'Amd64';
  }

  if (!os) {
    const ext = path.extname(fileName).slice(1).toLowerCase();
    const map = { exe: 'Windows', msi: 'Windows', dmg: 'MacOS', app: 'MacOS', deb: 'Linux', rpm: 'Linux', appimage: 'Linux' };
    os = map[ext] || 'Unknown';
  }

  if (!arch) {
    // Loud fallback: an unnamed arch silently mislabels release assets.
    console.warn(`[rename-bundle] WARN: no arch detected for "${fileName}" — defaulting to Amd64`);
    arch = 'Amd64';
  }
  if (!os) os = 'Unknown';
  return [os, arch];
}

function getVariant(fileName, ext) {
  const lower = fileName.toLowerCase();
  if (ext === 'msi') return 'Installer';
  if (ext === 'exe') return /setup/.test(lower) ? 'Installer' : 'Portable';
  if (ext === 'zip' && /portable/.test(lower)) return 'Portable';
  return null;
}

function walk(dir, files, dirs) {
  let entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); }
  catch { return; }
  for (const e of entries) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) { dirs.push(full); walk(full, files, dirs); }
    else if (e.isFile()) files.push(full);
  }
}

function tripleFromDir(dir) {
  // dir looks like .../target/<triple>/release/bundle/...
  const parts = dir.split(/[\\/]/);
  const idx = parts.indexOf('bundle');
  if (idx === -1) return '';
  // search backwards for a part containing a '-'
  for (let i = idx - 1; i >= 0; i--) {
    if (parts[i].includes('-') && !parts[i].includes('release')) return parts[i];
  }
  return '';
}

function copyDir(src, dest) {
  fs.mkdirSync(dest, { recursive: true });
  for (const e of fs.readdirSync(src, { withFileTypes: true })) {
    const s = path.join(src, e.name);
    const d = path.join(dest, e.name);
    if (e.isDirectory()) copyDir(s, d);
    else fs.copyFileSync(s, d);
  }
}

function main() {
  const conf = JSON.parse(fs.readFileSync(path.join(srcTauri, 'tauri.conf.json'), 'utf8'));
  const version = String(conf.version);
  // Keep the human-readable app name (with its own spacing) as the filename
  // prefix, e.g. "DeepSeek Harness Launcher-0.1.0-Windows-Amd64.msi". This is
  // the exact format users see on the GitHub Release page.
  const name = String(conf.productName);
  const isWindows = process.platform === 'win32';
  const tmpBase = process.env.RUNNER_TEMP || path.join(targetDir, '_tmp');

  // ---- locate every bundle directory (cross-compile aware) ----
  const bundleDirs = [];
  if (fs.existsSync(targetDir)) {
    const native = path.join(targetDir, 'release', 'bundle');
    if (fs.existsSync(native)) bundleDirs.push(native);
    for (const sub of fs.readdirSync(targetDir, { withFileTypes: true })) {
      if (!sub.isDirectory()) continue;
      const b = path.join(targetDir, sub.name, 'release', 'bundle');
      if (fs.existsSync(b)) bundleDirs.push(b);
    }
  }

  let found = 0;

  if (bundleDirs.length === 0) {
    console.log(`no bundle directories found under ${targetDir} (did the build run?)`);
  } else {
    for (const bundleDir of [...new Set(bundleDirs)]) {
      console.log(`scanning ${bundleDir}`);
      const files = [];
      const dirs = [];
      walk(bundleDir, files, dirs);

      for (const f of files) {
        const ext = path.extname(f).toLowerCase();
        if (!artifactExts.includes(ext)) continue;
        const triple = tripleFromDir(path.dirname(f));
        const [os, arch] = getOsArch(path.basename(f), triple);
        const variant = getVariant(path.basename(f), ext.slice(1));
        const newName = canonicalName(name, version, os, arch, variant, ext.slice(1));
        const dest = path.join(path.dirname(f), newName);
        if (path.basename(f) !== newName) {
          if (fs.existsSync(dest)) fs.unlinkSync(dest);
          fs.renameSync(f, dest);
          console.log(`renamed -> ${newName}`);
        } else {
          console.log(`skip (already named) ${newName}`);
        }
        found++;
      }

      // ---- package macOS .app bundles as .app.zip ----
      for (const d of dirs) {
        if (path.extname(d).toLowerCase() !== '.app') continue;
        const triple = tripleFromDir(path.dirname(d));
        const [os, arch] = getOsArch(path.basename(d), triple);
        const newName = canonicalName(name, version, os, arch, null, 'app.zip');
        const zipPath = path.join(path.dirname(d), newName);
        if (!fs.existsSync(zipPath)) {
          // Use `tar` (available on all runners incl. Windows via tar.exe) to zip
          // the .app directory, mirroring the GitHub Release asset style.
          if (isWindows) {
            execFileSync('tar', ['-a', '-cf', zipPath, '-C', path.dirname(d), path.basename(d)], { stdio: 'inherit' });
          } else {
            execFileSync('zip', ['-r', '-q', zipPath, path.basename(d)], { cwd: path.dirname(d), stdio: 'inherit' });
          }
          console.log(`zipped -> ${newName}`);
        } else {
          console.log(`skip (already exists) ${newName}`);
        }
        found++;
      }
    }
  }

  // ---- create Linux portable tar.gz archives (binary + runtime-host) ----
  const linuxTriples = ['x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu'];
  const binaryName = 'deepseek_harness_launcher';
  const runtimeHost = path.join(root, 'runtime-host');

  for (const triple of linuxTriples) {
    const releaseDir = path.join(targetDir, triple, 'release');
    const binary = path.join(releaseDir, binaryName);
    if (!fs.existsSync(binary)) continue;

    const arch = /aarch64/.test(triple) ? 'Arm64' : 'Amd64';
    const outDir = path.join(targetDir, triple, 'release', 'bundle');
    if (!fs.existsSync(outDir)) fs.mkdirSync(outDir, { recursive: true });

    const mkTmp = (suffix) => {
      const p = path.join(tmpBase, `linux-portable-${triple}-${suffix}`);
      if (fs.existsSync(p)) fs.rmSync(p, { recursive: true, force: true });
      fs.mkdirSync(p, { recursive: true });
      return p;
    };

    // Plain archive: just the binary.
    const plainDir = mkTmp('plain');
    fs.copyFileSync(binary, path.join(plainDir, binaryName));
    if (!isWindows) fs.chmodSync(path.join(plainDir, binaryName), 0o755);
    const plainName = canonicalName(name, version, 'Linux', arch, null, 'tar.gz');
    const plainTar = path.join(outDir, plainName);
    if (fs.existsSync(plainTar)) fs.unlinkSync(plainTar);
    execFileSync('tar', ['-czf', plainTar, '-C', plainDir, '.'], { stdio: 'inherit' });
    console.log(`created -> ${plainName}`);
    found++;
    fs.rmSync(plainDir, { recursive: true, force: true });

    // WebKit41 archive: binary + offline runtime-host.
    const wkDir = mkTmp('webkit41');
    fs.copyFileSync(binary, path.join(wkDir, binaryName));
    if (!isWindows) fs.chmodSync(path.join(wkDir, binaryName), 0o755);
    if (fs.existsSync(runtimeHost)) {
      copyDir(runtimeHost, path.join(wkDir, 'runtime-host'));
    }
    const wkName = canonicalName(name, version, 'Linux', arch, 'WebKit41', 'tar.gz');
    const wkTar = path.join(outDir, wkName);
    if (fs.existsSync(wkTar)) fs.unlinkSync(wkTar);
    execFileSync('tar', ['-czf', wkTar, '-C', wkDir, '.'], { stdio: 'inherit' });
    console.log(`created -> ${wkName}`);
    found++;
    fs.rmSync(wkDir, { recursive: true, force: true });
  }

  console.log(`renamed/created ${found} artifact(s)`);
}

if (require.main === module) {
  main();
}

module.exports = { canonicalName, getOsArch, getVariant, tripleFromDir };
