#!/usr/bin/env node
// Changelog tooling. CHANGELOG.md (Indonesian) and CHANGELOG.en.md (English)
// are two editions of one changelog; the format is described in CHANGELOG.md.
//   check-unreleased   fail unless the unreleased section of both editions
//                      lists changes (npm preversion, tagged bumps only)
//   release <version>  move the unreleased changes into a dated <version>
//                      section in both editions
//   check <version>    fail unless both editions have <version> (release workflow)
//   body <version>     GitHub release page text: <version> in both languages
//   notes [lang]       every released section of one edition (default: id)
//   updater-notes <f>  put the released sections into the updater file <f>
//                      (latest.json): `notes` (id, read by apps up to 0.5.0)
//                      and `notes_i18n` per language, so apps list every
//                      version they skipped in their own language
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');

/** Editions by language; the first one is the default. */
export const EDITIONS = {
  id: { file: path.join(ROOT, 'CHANGELOG.md'), download: 'Unduh installer untuk sistem operasi kamu di bagian **Assets**.' },
  en: { file: path.join(ROOT, 'CHANGELOG.en.md'), download: 'Download the installer for your operating system under **Assets**.' },
};
const LANGS = Object.keys(EDITIONS);
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
  if (!s) throw new Error('no unreleased section ("## [Belum dirilis]" / "## [Unreleased]")');
  return s;
}

export function checkUnreleased(text) {
  if (!hasEntries(unreleased(parse(text)).body)) {
    throw new Error('add the changes of this release to the unreleased section first');
  }
}

/** Moves the unreleased changes into `## [version] - date`. Idempotent. */
export function release(text, version, date = new Date().toISOString().slice(0, 10)) {
  const doc = parse(text);
  if (doc.sections.some((s) => s.name === version)) return text;
  const open = unreleased(doc);
  if (!hasEntries(open.body)) throw new Error(`the unreleased section is empty, nothing to release as ${version}`);
  const entry = { name: version, heading: `## [${version}] - ${date}`, body: ['', ...trimLines(open.body).split('\n'), ''] };
  open.body = [''];
  doc.sections.splice(doc.sections.indexOf(open) + 1, 0, entry);
  const out = [...doc.preamble, ...doc.sections.flatMap((s) => [s.heading, ...s.body])];
  return `${out.join('\n').trimEnd()}\n`;
}

export function section(text, version) {
  const s = parse(text).sections.find((x) => x.name === version);
  if (!s || !hasEntries(s.body)) throw new Error(`no entries for ${version}`);
  return trimLines(s.body);
}

export function notes(text) {
  return parse(text)
    .sections.filter((s) => !UNRELEASED.test(s.heading) && hasEntries(s.body))
    .map((s) => `${s.heading}\n\n${trimLines(s.body)}`)
    .join('\n\n');
}

/** Released versions, newest first. */
export const versions = (text) =>
  parse(text)
    .sections.filter((s) => !UNRELEASED.test(s.heading) && hasEntries(s.body))
    .map((s) => s.name);

export const read = (lang) => fs.readFileSync(EDITIONS[lang].file, 'utf8');

/** Runs `fn` on every edition; errors name the file. */
export function eachEdition(fn) {
  return Object.fromEntries(
    LANGS.map((lang) => {
      try {
        return [lang, fn(read(lang), lang)];
      } catch (e) {
        throw new Error(`${path.basename(EDITIONS[lang].file)}: ${e.message}`);
      }
    }),
  );
}

/** Both editions must list the same released versions. */
export function checkInStep() {
  const lists = eachEdition(versions);
  const [first, ...rest] = LANGS;
  for (const lang of rest) {
    if (lists[lang].join() !== lists[first].join()) {
      throw new Error(
        `${path.basename(EDITIONS[lang].file)} lists ${lists[lang].join(', ') || 'no versions'}, ` +
          `${path.basename(EDITIONS[first].file)} lists ${lists[first].join(', ') || 'no versions'}`,
      );
    }
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const [cmd, arg] = process.argv.slice(2);
  const needVersion = () => {
    if (!/^\d+\.\d+\.\d+$/.test(arg ?? '')) throw new Error(`usage: changelog.mjs ${cmd} <x.y.z>`);
    return arg;
  };
  try {
    if (cmd === 'check-unreleased') {
      if (isTaggedBump()) eachEdition(checkUnreleased);
    } else if (cmd === 'release') {
      const version = needVersion();
      const out = eachEdition((text) => release(text, version));
      for (const lang of LANGS) fs.writeFileSync(EDITIONS[lang].file, out[lang]);
    } else if (cmd === 'check') {
      const version = needVersion();
      eachEdition((text) => section(text, version));
      checkInStep();
    } else if (cmd === 'body') {
      const version = needVersion();
      const parts = eachEdition((text, lang) => `${section(text, version)}\n\n${EDITIONS[lang].download}`);
      process.stdout.write(`${LANGS.map((lang) => parts[lang]).join('\n\n---\n\n')}\n`);
    } else if (cmd === 'notes') {
      const lang = arg ?? LANGS[0];
      if (!EDITIONS[lang]) throw new Error(`unknown language ${lang} (${LANGS.join(', ')})`);
      process.stdout.write(`${notes(read(lang))}\n`);
    } else if (cmd === 'updater-notes') {
      const file = arg ?? '';
      const json = JSON.parse(fs.readFileSync(file, 'utf8'));
      checkInStep();
      json.notes_i18n = eachEdition(notes);
      json.notes = json.notes_i18n[LANGS[0]];
      fs.writeFileSync(file, `${JSON.stringify(json, null, 2)}\n`);
    } else throw new Error('usage: changelog.mjs <check-unreleased|release|check|body|notes|updater-notes> [version|lang|file]');
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}
