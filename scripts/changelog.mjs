#!/usr/bin/env node
// CHANGELOG.md tooling (format: see the top of CHANGELOG.md).
//   check-unreleased   fail unless "Belum dirilis" lists changes (npm preversion,
//                      tagged bumps only)
//   release <version>  move "Belum dirilis" into a dated <version> section
//   check <version>    fail unless <version> has a section (release workflow)
//   section <version>  print that section's body (GitHub release page)
//   notes              print every released section
//   updater-notes <f>  put every released section into the updater file <f>
//                      (latest.json), so apps list all versions they skipped
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const CHANGELOG = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'CHANGELOG.md');
const UNRELEASED = /^## \[(Belum dirilis|Unreleased)\]\s*$/i;
const HEADING = /^## \[([^\]]+)\]/;

/** Splits the file into its preamble and `## [...]` sections (heading + body lines). */
function parse(text) {
  const lines = text.split(/\r?\n/);
  const sections = [];
  const preamble = [];
  for (const line of lines) {
    const m = line.match(HEADING);
    if (m) sections.push({ name: m[1], heading: line, body: [] });
    else (sections.at(-1)?.body ?? preamble).push(line);
  }
  return { preamble, sections };
}

/** `npm version` without --no-git-tag-version (npm then sets this to ""), i.e. a real release. */
export const isTaggedBump = () => [undefined, 'true'].includes(process.env.npm_config_git_tag_version);

const trimLines = (lines) => lines.join('\n').trim();
const hasEntries = (body) => body.some((l) => /^\s*[-*] \S/.test(l));

function unreleased(doc) {
  const s = doc.sections.find((x) => UNRELEASED.test(x.heading));
  if (!s) throw new Error('CHANGELOG.md has no "## [Belum dirilis]" section');
  return s;
}

export function checkUnreleased(text) {
  if (!hasEntries(unreleased(parse(text)).body)) {
    throw new Error('CHANGELOG.md: add the changes of this release under "## [Belum dirilis]" first');
  }
}

/** Moves the unreleased changes into `## [version] - date`. Idempotent. */
export function release(text, version, date = new Date().toISOString().slice(0, 10)) {
  const doc = parse(text);
  if (doc.sections.some((s) => s.name === version)) return text;
  const open = unreleased(doc);
  if (!hasEntries(open.body)) throw new Error(`CHANGELOG.md: "Belum dirilis" is empty, nothing to release as ${version}`);
  const entry = { name: version, heading: `## [${version}] - ${date}`, body: ['', ...trimLines(open.body).split('\n'), ''] };
  open.body = [''];
  doc.sections.splice(doc.sections.indexOf(open) + 1, 0, entry);
  const out = [...doc.preamble, ...doc.sections.flatMap((s) => [s.heading, ...s.body])];
  return `${out.join('\n').trimEnd()}\n`;
}

export function section(text, version) {
  const s = parse(text).sections.find((x) => x.name === version);
  if (!s || !hasEntries(s.body)) throw new Error(`CHANGELOG.md has no entries for ${version}`);
  return trimLines(s.body);
}

export function notes(text) {
  return parse(text)
    .sections.filter((s) => !UNRELEASED.test(s.heading) && hasEntries(s.body))
    .map((s) => `${s.heading}\n\n${trimLines(s.body)}`)
    .join('\n\n');
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const [cmd, version] = process.argv.slice(2);
  const needVersion = () => {
    if (!/^\d+\.\d+\.\d+$/.test(version ?? '')) throw new Error(`usage: changelog.mjs ${cmd} <x.y.z>`);
    return version;
  };
  try {
    const text = fs.readFileSync(CHANGELOG, 'utf8');
    if (cmd === 'check-unreleased') {
      if (isTaggedBump()) checkUnreleased(text);
    }
    else if (cmd === 'release') fs.writeFileSync(CHANGELOG, release(text, needVersion()));
    else if (cmd === 'check') section(text, needVersion());
    else if (cmd === 'section') process.stdout.write(`${section(text, needVersion())}\n`);
    else if (cmd === 'notes') process.stdout.write(`${notes(text)}\n`);
    else if (cmd === 'updater-notes') {
      const file = version ?? '';
      const json = JSON.parse(fs.readFileSync(file, 'utf8'));
      json.notes = notes(text);
      fs.writeFileSync(file, `${JSON.stringify(json, null, 2)}\n`);
    } else throw new Error('usage: changelog.mjs <check-unreleased|release|check|section|notes|updater-notes> [version|file]');
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}
