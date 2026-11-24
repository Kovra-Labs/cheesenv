#!/usr/bin/env node
// Stackyard registry tool.
//   keygen [--key <path>]            create an Ed25519 signing key + src-tauri/registry.pub
//   hash                             fill empty "sha256" fields by downloading assets
//   build  [--key <path>] [--out d]  write dist/ (index.json, .minisig, modules/*.json)
// Signatures use the minisign format (prehashed "ED", BLAKE2b-512), so they
// can also be checked with `minisign -Vm index.json -p registry.pub`.
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const srcDir = path.join(here, 'modules');
const pubPath = path.join(here, '..', 'src-tauri', 'registry.pub');
const args = process.argv.slice(2);
const opt = (name, def) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : def;
};
const keyPath = opt('key', process.env.STACKYARD_REGISTRY_KEY ?? path.join(os.homedir(), '.stackyard-registry', 'minisign.key.json'));

function keygen() {
  if (fs.existsSync(keyPath)) throw new Error(`refusing to overwrite existing key ${keyPath}`);
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const keyId = crypto.randomBytes(8);
  const rawPub = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url');
  fs.mkdirSync(path.dirname(keyPath), { recursive: true, mode: 0o700 });
  fs.writeFileSync(
    keyPath,
    JSON.stringify({ key_id: keyId.toString('hex'), private_pem: privateKey.export({ format: 'pem', type: 'pkcs8' }) }),
    { mode: 0o600 },
  );
  const idText = Buffer.from(keyId).reverse().toString('hex').toUpperCase();
  const pub = `untrusted comment: minisign public key ${idText}\n${Buffer.concat([Buffer.from('Ed'), keyId, rawPub]).toString('base64')}\n`;
  fs.writeFileSync(pubPath, pub);
  console.log(`private key: ${keyPath} (keep offline, never commit)\npublic key:  ${pubPath}`);
}

function sign(data, fileName) {
  const { key_id, private_pem } = JSON.parse(fs.readFileSync(keyPath, 'utf8'));
  const key = crypto.createPrivateKey(private_pem);
  const keyId = Buffer.from(key_id, 'hex');
  const digest = crypto.createHash('blake2b512').update(data).digest();
  const sig = crypto.sign(null, digest, key);
  const trusted = `timestamp:${Math.floor(Date.now() / 1000)}\tfile:${fileName}\thashed`;
  const globalSig = crypto.sign(null, Buffer.concat([sig, Buffer.from(trusted)]), key);
  return [
    'untrusted comment: signature from stackyard registry tool',
    Buffer.concat([Buffer.from('ED'), keyId, sig]).toString('base64'),
    `trusted comment: ${trusted}`,
    globalSig.toString('base64'),
    '',
  ].join('\n');
}

const sources = () =>
  fs
    .readdirSync(srcDir)
    .filter((f) => f.endsWith('.json'))
    .sort()
    .map((f) => ({ file: path.join(srcDir, f), json: JSON.parse(fs.readFileSync(path.join(srcDir, f), 'utf8')) }));

async function sha256Url(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  const h = crypto.createHash('sha256');
  for await (const chunk of res.body) h.update(chunk);
  return h.digest('hex');
}

// Every downloadable of a version: [label, asset], including `extra` archives.
const downloads = (v) =>
  Object.entries(v.assets).flatMap(([p, a]) => [[p, a], ...(a.extra ?? []).map((x) => [`${p} ${x.dest}`, x])]);

async function hash() {
  const known = new Map(); // same URL in several platforms: download once
  for (const { file, json } of sources()) {
    let changed = false;
    for (const v of json.versions) {
      for (const [platform, asset] of downloads(v)) {
        if (asset.sha256) continue;
        if (known.has(asset.url)) {
          asset.sha256 = known.get(asset.url);
          changed = true;
          continue;
        }
        process.stdout.write(`${json.name} ${v.version} ${platform} ... `);
        asset.sha256 = await sha256Url(asset.url);
        known.set(asset.url, asset.sha256);
        console.log(asset.sha256.slice(0, 12));
        changed = true;
      }
    }
    if (changed) fs.writeFileSync(file, JSON.stringify(json, null, 2) + '\n');
  }
}

const versionKey = (v) => v.split(/\D+/).filter(Boolean).map(Number);
const cmpVersion = (a, b) => {
  const [x, y] = [versionKey(a), versionKey(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) - (y[i] ?? 0);
  return 0;
};

function build() {
  const out = opt('out', path.join(here, 'dist'));
  fs.mkdirSync(path.join(out, 'modules'), { recursive: true });
  const modules = [];
  for (const { json } of sources()) {
    for (const v of json.versions)
      for (const [p, a] of downloads(v))
        if (!/^[0-9a-f]{64}$/.test(a.sha256 ?? '')) throw new Error(`${json.name} ${v.version} ${p}: missing sha256 (run hash)`);
    const bytes = Buffer.from(JSON.stringify(json, null, 2) + '\n');
    fs.writeFileSync(path.join(out, 'modules', `${json.name}.json`), bytes);
    modules.push({
      name: json.name,
      category: json.category,
      description: json.description ?? {},
      latest: json.versions.map((v) => v.version).sort(cmpVersion).at(-1),
      manifest_sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
      license: json.license ?? '',
      homepage: json.homepage ?? '',
    });
  }
  const index = Buffer.from(JSON.stringify({ schema: 1, updated: new Date().toISOString(), modules }, null, 2) + '\n');
  fs.writeFileSync(path.join(out, 'index.json'), index);
  fs.writeFileSync(path.join(out, 'index.json.minisig'), sign(index, 'index.json'));
  console.log(`built ${modules.length} modules into ${out}`);
}

const commands = { keygen, hash, build };
const cmd = commands[args[0]];
if (!cmd) {
  console.error('usage: node registry/tool.mjs <keygen|hash|build> [--key path] [--out dir]');
  process.exit(1);
}
Promise.resolve(cmd()).catch((e) => {
  console.error(e.message);
  process.exit(1);
});
