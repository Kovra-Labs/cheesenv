// Run by `npm version <x>`: copies package.json's version into Cargo.toml so
// the app, installer and updater all report the same version.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';

const { version } = JSON.parse(fs.readFileSync('package.json', 'utf8'));
const cargoPath = 'src-tauri/Cargo.toml';
const cargo = fs.readFileSync(cargoPath, 'utf8');
const next = cargo.replace(/^(\[package\][^[]*?\nversion = )"[^"]*"/m, `$1"${version}"`);
if (next === cargo && !cargo.includes(`version = "${version}"`)) throw new Error('version line not found in Cargo.toml');
fs.writeFileSync(cargoPath, next);
try {
  execFileSync('git', ['add', cargoPath], { stdio: 'ignore' });
} catch {
  // not a git checkout; the file is still updated
}
console.log(`version ${version} → ${cargoPath}`);
