#!/usr/bin/env node
// Refreshes module versions from their official upstream sources.
//   check                    show what would change
//   update [--only a,b]      rewrite registry/modules/*.json (versions only)
// Every module keeps its two newest supported release lines. URLs and SHA-256
// sums come from the upstream checksum files, never from hashing a download.
// Redis and Apache have no official Linux/macOS binaries: those are built by
// .github/workflows/binaries.yml into GitHub releases `<module>-<version>`;
// versions whose build is missing are listed under "builds".
// Signing stays local: run `npm run registry build` after merging.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const modulesDir = path.join(here, 'modules');
const BINARIES_REPO = process.env.STACKYARD_BINARIES_REPO ?? 'pipsyrs/stackyard';
const LINES = 2;

const headers = (url) =>
  url.startsWith('https://api.github.com/') && process.env.GITHUB_TOKEN
    ? { authorization: `Bearer ${process.env.GITHUB_TOKEN}`, 'x-github-api-version': '2022-11-28' }
    : {};
async function get(url, { allow404 = false } = {}) {
  const res = await fetch(url, { headers: headers(url) });
  if (allow404 && res.status === 404) return null;
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return res;
}
const text = async (url) => (await get(url)).text();
const json = async (url, opt) => (await get(url, opt))?.json() ?? null;

/** `<hash>  <file>` lines (sha256sum / SHASUMS256 format) → Map(file → hash). */
const sumFile = (body) =>
  new Map(
    body
      .split('\n')
      .map((l) => l.trim().split(/\s+/))
      .filter((p) => p.length === 2 && /^[0-9a-f]{64}$/i.test(p[0]))
      .map(([h, f]) => [f.replace(/^\*/, ''), h.toLowerCase()]),
  );

const versionKey = (v) => v.split(/\D+/).filter(Boolean).map(Number);
const cmp = (a, b) => {
  const [x, y] = [versionKey(a), versionKey(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) - (y[i] ?? 0);
  return 0;
};
/** Newest version of each of the `n` newest lines (`line` maps a version to its line). */
function newestPerLine(versions, line, n = LINES) {
  const best = new Map();
  for (const v of versions) {
    const l = line(v);
    if (!best.has(l) || cmp(v, best.get(l)) > 0) best.set(l, v);
  }
  return [...best.values()].sort((a, b) => cmp(b, a)).slice(0, n);
}
const major = (v) => v.split('.')[0];
const minor = (v) => v.split('.').slice(0, 2).join('.');
const isStable = (v) => /^\d+(\.\d+)+$/.test(v);

const asset = (url, sha256, format, extra = {}) => ({ url, sha256, format, strip: 1, ...extra });
const unixBin = (key) => (key.startsWith('windows') ? '' : 'bin');

/** Digests of a GitHub release's assets (GitHub records sha256 for each upload). */
async function releaseDigests(repo, tag) {
  const rel = await json(`https://api.github.com/repos/${repo}/releases/tags/${encodeURIComponent(tag)}`, { allow404: true });
  const out = new Map();
  for (const a of rel?.assets ?? []) {
    if (a.digest?.startsWith('sha256:')) out.set(a.name, { url: a.browser_download_url, sha256: a.digest.slice(7) });
  }
  return out;
}

const sources = {
  async node() {
    const index = await json('https://nodejs.org/dist/index.json');
    const lts = index.filter((r) => r.lts).map((r) => r.version.slice(1));
    const plats = {
      'linux-x64': ['linux-x64', 'tar.xz'],
      'linux-arm64': ['linux-arm64', 'tar.xz'],
      'macos-x64': ['darwin-x64', 'tar.xz'],
      'macos-arm64': ['darwin-arm64', 'tar.xz'],
      'windows-x64': ['win-x64', 'zip'],
    };
    const versions = [];
    for (const v of newestPerLine(lts, major)) {
      const base = `https://nodejs.org/dist/v${v}/`;
      const sums = sumFile(await text(base + 'SHASUMS256.txt'));
      const assets = {};
      for (const [key, [p, fmt]] of Object.entries(plats)) {
        const file = `node-v${v}-${p}.${fmt}`;
        if (sums.has(file)) assets[key] = asset(base + file, sums.get(file), fmt, { bin_dir: unixBin(key) });
      }
      versions.push({ version: v, assets });
    }
    return { versions };
  },

  async python() {
    const repo = 'astral-sh/python-build-standalone';
    const { tag_name: tag } = await json(`https://api.github.com/repos/${repo}/releases/latest`);
    const base = `https://github.com/${repo}/releases/download/${tag}/`;
    const sums = sumFile(await text(base + 'SHA256SUMS'));
    const triples = {
      'linux-x64': 'x86_64-unknown-linux-gnu',
      'linux-arm64': 'aarch64-unknown-linux-gnu',
      'macos-x64': 'x86_64-apple-darwin',
      'macos-arm64': 'aarch64-apple-darwin',
      'windows-x64': 'x86_64-pc-windows-msvc',
    };
    const found = [...sums.keys()]
      .map((f) => f.match(/^cpython-([\d.]+)\+\d+-x86_64-unknown-linux-gnu-install_only\.tar\.gz$/)?.[1])
      .filter(Boolean);
    const versions = [];
    for (const v of newestPerLine(found, minor)) {
      const assets = {};
      for (const [key, t] of Object.entries(triples)) {
        const file = `cpython-${v}+${tag}-${t}-install_only.tar.gz`;
        if (sums.has(file))
          assets[key] = asset(base + file.replace('+', '%2B'), sums.get(file), 'tar.gz', { bin_dir: unixBin(key) });
      }
      versions.push({ version: v, assets });
    }
    return { versions };
  },

  async mariadb() {
    const api = 'https://downloads.mariadb.org/rest-api/mariadb/';
    const { major_releases } = await json(api);
    const lines = major_releases
      .filter((r) => r.release_status === 'Stable' && r.release_support_type === 'Long Term Support')
      .map((r) => r.release_id)
      .sort((a, b) => cmp(b, a))
      .slice(0, LINES);
    const versions = [];
    for (const line of lines) {
      const { releases } = await json(`${api}${line}/latest/`);
      const [[v, rel]] = Object.entries(releases);
      const assets = {};
      for (const f of rel.files) {
        const key =
          f.file_name === `mariadb-${v}-linux-systemd-x86_64.tar.gz`
            ? 'linux-x64'
            : f.file_name === `mariadb-${v}-winx64.zip`
              ? 'windows-x64'
              : null;
        const sha = f.checksum?.sha256sum;
        // The HTTPS redirector picks a nearby mirror; the hash pins the file.
        if (key && sha) assets[key] = asset(`${api}${v}/${f.file_name}`, sha, key === 'linux-x64' ? 'tar.gz' : 'zip', { bin_dir: 'bin' });
      }
      versions.push({ version: v, assets });
    }
    return { versions };
  },

  async mongodb() {
    const { versions: all } = await json('https://downloads.mongodb.org/full.json');
    const prod = all.filter((r) => r.production_release && r.version.split('.')[1] === '0'); // major (LTS) lines only
    const shell = await mongosh();
    const versions = [];
    for (const v of newestPerLine(
      prod.map((r) => r.version),
      major,
    )) {
      const rel = prod.find((r) => r.version === v);
      const pick = (edition, target, arch) =>
        rel.downloads.find((d) => d.edition === edition && d.target === target && d.arch === arch)?.archive;
      const plats = {
        // Ubuntu 22.04 builds: glibc 2.35 + OpenSSL 3, the widest Linux reach.
        'linux-x64': pick('targeted', 'ubuntu2204', 'x86_64'),
        'linux-arm64': pick('targeted', 'ubuntu2204', 'aarch64'),
        'macos-x64': pick('base', 'macos', 'x86_64'),
        'macos-arm64': pick('base', 'macos', 'arm64'),
        'windows-x64': pick('base', 'windows', 'x86_64'),
      };
      const assets = {};
      for (const [key, a] of Object.entries(plats)) {
        if (!a?.sha256 || !shell[key]) continue;
        assets[key] = asset(a.url, a.sha256, a.url.endsWith('.zip') ? 'zip' : 'tar.gz', {
          bin_dir: 'bin',
          extra: [shell[key]],
        });
      }
      versions.push({ version: v, assets });
    }
    return { versions };
  },

  async redis() {
    const releases = await json('https://api.github.com/repos/redis/redis/releases?per_page=50');
    const stable = releases.filter((r) => !r.prerelease && !r.draft && isStable(r.tag_name)).map((r) => r.tag_name);
    const win = await json('https://api.github.com/repos/redis-windows/redis-windows/releases?per_page=30');
    const builds = [];
    const versions = [];
    for (const v of newestPerLine(stable, minor)) {
      const assets = await builtAssets('redis', v, builds);
      const zip = win
        .find((r) => r.tag_name === v)
        ?.assets.find((a) => a.name === `Redis-${v}-Windows-x64-msys2.zip` && a.digest?.startsWith('sha256:'));
      if (zip) assets['windows-x64'] = asset(zip.browser_download_url, zip.digest.slice(7), 'zip', { bin_dir: '' });
      versions.push({ version: v, assets });
    }
    return { versions, builds };
  },

  async apache() {
    const listing = await text('https://downloads.apache.org/httpd/');
    const found = [...listing.matchAll(/httpd-(2\.4\.\d+)\.tar\.bz2"/g)].map((m) => m[1]);
    const [v] = newestPerLine(found, minor, 1); // 2.4 is the only maintained line
    const builds = [];
    const assets = await builtAssets('apache', v, builds);
    // Windows: Apache Lounge, the build httpd.apache.org links to.
    const page = await text('https://www.apachelounge.com/download/');
    const zip = page.match(new RegExp(`/download/(VS\\d+)/binaries/httpd-${v.replace(/\./g, '\\.')}-\\d+-[Ww]in64-VS\\d+\\.zip`))?.[0];
    if (zip) {
      const url = `https://www.apachelounge.com${zip}`;
      const sha = (await text(`${url}.txt`)).match(/SHA256-Checksum for: [^\n]+\n([0-9A-F]{64})/i)?.[1];
      if (sha) assets['windows-x64'] = asset(url, sha.toLowerCase(), 'zip', { bin_dir: 'bin' });
    }
    return { versions: [{ version: v, assets }], builds };
  },
};

/** mongosh shell (bundled into the MongoDB module as `mongosh/`). */
async function mongosh() {
  const rel = await json('https://api.github.com/repos/mongodb-js/mongosh/releases/latest');
  const v = rel.tag_name.replace(/^v/, '');
  const names = {
    'linux-x64': `mongosh-${v}-linux-x64.tgz`,
    'linux-arm64': `mongosh-${v}-linux-arm64.tgz`,
    'macos-x64': `mongosh-${v}-darwin-x64.zip`,
    'macos-arm64': `mongosh-${v}-darwin-arm64.zip`,
    'windows-x64': `mongosh-${v}-win32-x64.zip`,
  };
  const out = {};
  for (const [key, name] of Object.entries(names)) {
    const a = rel.assets.find((x) => x.name === name && x.digest?.startsWith('sha256:'));
    if (a) out[key] = { url: a.browser_download_url, sha256: a.digest.slice(7), format: name.endsWith('.zip') ? 'zip' : 'tar.gz', strip: 1, dest: 'mongosh' };
  }
  return out;
}

/** Linux/macOS archives built by binaries.yml; records a needed build when any is missing. */
async function builtAssets(name, v, builds) {
  const plats = ['linux-x64', 'linux-arm64', 'macos-x64', 'macos-arm64'];
  const digests = await releaseDigests(BINARIES_REPO, `${name}-${v}`);
  const assets = {};
  for (const key of plats) {
    const d = digests.get(`${name}-${v}-${key}.tar.gz`);
    if (d) assets[key] = asset(d.url, d.sha256, 'tar.gz', { bin_dir: 'bin' });
  }
  if (Object.keys(assets).length < plats.length) builds.push({ module: name, version: v });
  return assets;
}

const ORDER = ['linux-x64', 'linux-arm64', 'macos-x64', 'macos-arm64', 'windows-x64'];
const sortAssets = (assets) => Object.fromEntries(ORDER.filter((k) => assets[k]).map((k) => [k, assets[k]]));

async function run(write) {
  const args = process.argv.slice(3);
  const only = args.includes('--only') ? args[args.indexOf('--only') + 1].split(',') : Object.keys(sources);
  const changed = [];
  const builds = [];
  const errors = [];
  for (const name of only) {
    if (!sources[name]) throw new Error(`unknown module ${name}`);
    const file = path.join(modulesDir, `${name}.json`);
    const manifest = JSON.parse(fs.readFileSync(file, 'utf8'));
    try {
      const res = await sources[name]();
      builds.push(...(res.builds ?? []));
      const versions = res.versions
        .map((v) => ({ version: v.version, assets: sortAssets(v.assets) }))
        .filter((v) => Object.keys(v.assets).length > 0);
      if (versions.length === 0) throw new Error('no downloadable version found');
      const before = manifest.versions.map((v) => v.version).join(', ') || '-';
      if (JSON.stringify(versions) === JSON.stringify(manifest.versions)) {
        console.log(`${name}: up to date (${before})`);
        continue;
      }
      changed.push(name);
      const after = versions.map((v) => `${v.version} [${Object.keys(v.assets).join(' ')}]`).join(', ');
      console.log(`${name}: ${before} -> ${after}`);
      if (write) fs.writeFileSync(file, JSON.stringify({ ...manifest, versions }, null, 2) + '\n');
    } catch (e) {
      errors.push(name);
      console.error(`${name}: ${e.message}`);
    }
  }
  for (const b of builds) console.log(`build needed: ${b.module} ${b.version}`);
  if (process.env.GITHUB_OUTPUT) {
    fs.appendFileSync(process.env.GITHUB_OUTPUT, `changed=${changed.join(',')}\nbuilds=${JSON.stringify(builds)}\n`);
  }
  if (errors.length) process.exitCode = 1;
}

const commands = { check: () => run(false), update: () => run(true) };
const cmd = commands[process.argv[2]];
if (!cmd) {
  console.error('usage: node registry/upstream.mjs <check|update> [--only name,name]');
  process.exit(1);
}
cmd().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
