// Run by `npm version <x>`: copies package.json's version into Cargo.toml so
// the app, installer and updater all report the same version. A tagged
// release also turns the "Belum dirilis" part of CHANGELOG.md into that
// version's section; a local bump (--no-git-tag-version) leaves it alone.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { CHANGELOG, isTaggedBump, release } from './changelog.mjs';

const { version } = JSON.parse(fs.readFileSync('package.json', 'utf8'));
const cargoPath = 'src-tauri/Cargo.toml';
const cargo = fs.readFileSync(cargoPath, 'utf8');
const next = cargo.replace(/^(\[package\][^[]*?\nversion = )"[^"]*"/m, `$1"${version}"`);
if (next === cargo && !cargo.includes(`version = "${version}"`)) throw new Error('version line not found in Cargo.toml');
fs.writeFileSync(cargoPath, next);
const tagged = isTaggedBump();
if (tagged) fs.writeFileSync(CHANGELOG, release(fs.readFileSync(CHANGELOG, 'utf8'), version));
try {
  execFileSync('git', ['add', cargoPath, ...(tagged ? [CHANGELOG] : [])], { stdio: 'ignore' });
} catch {
  // not a git checkout; the file is still updated
}
console.log(`version ${version} → ${cargoPath}${tagged ? ', CHANGELOG.md' : ''}`);
