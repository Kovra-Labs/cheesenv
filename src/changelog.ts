// Parses the changelog (bundled with the app, and sent as updater notes) into
// releases the UI lists: what was added, changed, fixed or removed. It comes
// in one edition per app language: CHANGELOG.md (id) and CHANGELOG.en.md (en).
import id from '../CHANGELOG.md?raw';
import en from '../CHANGELOG.en.md?raw';

export type ChangeKind = 'added' | 'changed' | 'fixed' | 'removed' | 'security' | 'deprecated' | 'other';

export interface ChangeGroup {
  kind: ChangeKind;
  items: string[];
}

export interface Release {
  version: string;
  date: string | null;
  groups: ChangeGroup[];
}

/** Section names in Indonesian (the file) or English (Keep a Changelog). */
const KINDS: Record<string, ChangeKind> = {
  ditambahkan: 'added',
  added: 'added',
  diubah: 'changed',
  changed: 'changed',
  diperbaiki: 'fixed',
  fixed: 'fixed',
  dihapus: 'removed',
  removed: 'removed',
  keamanan: 'security',
  security: 'security',
  usang: 'deprecated',
  deprecated: 'deprecated',
};

const VERSION = /^\d+\.\d+\.\d+$/;

export function parseChangelog(text: string | null | undefined): Release[] {
  const out: Release[] = [];
  let release: Release | null = null;
  let group: ChangeGroup | null = null;
  let item = -1;
  for (const line of (text ?? '').split(/\r?\n/)) {
    const head = line.match(/^##\s+\[([^\]]+)\](?:\s*-\s*(\S+))?/);
    if (head) {
      // Only released versions; "Belum dirilis" and anything else is skipped.
      release = VERSION.test(head[1]) ? { version: head[1], date: head[2] ?? null, groups: [] } : null;
      if (release) out.push(release);
      group = null;
      item = -1;
      continue;
    }
    if (!release) continue;
    const sub = line.match(/^###\s+(.+?)\s*$/);
    if (sub) {
      group = { kind: KINDS[sub[1].toLowerCase()] ?? 'other', items: [] };
      release.groups.push(group);
      item = -1;
      continue;
    }
    const bullet = line.match(/^\s*[-*]\s+(.*)$/);
    if (bullet) {
      if (!group) {
        group = { kind: 'other', items: [] };
        release.groups.push(group);
      }
      item = group.items.push(bullet[1].trim()) - 1;
    } else if (group && item >= 0 && /^\s+\S/.test(line)) {
      group.items[item] += ` ${line.trim()}`; // wrapped bullet
    } else if (!line.trim()) {
      item = -1;
    }
  }
  return out.map((r) => ({ ...r, groups: r.groups.filter((g) => g.items.length) })).filter((r) => r.groups.length);
}

export function compareVersions(a: string, b: string): number {
  const x = a.split('.').map(Number);
  const y = b.split('.').map(Number);
  for (let i = 0; i < 3; i++) if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) - (y[i] || 0);
  return 0;
}

/** Releases newer than `after` up to and including `upTo`, newest first. */
export function releasesBetween(releases: Release[], after: string, upTo: string): Release[] {
  return releases
    .filter((r) => compareVersions(r.version, after) > 0 && compareVersions(r.version, upTo) <= 0)
    .sort((a, b) => compareVersions(b.version, a.version));
}

/** Text with `code` spans, rendered without HTML. */
export function segments(text: string): { code: boolean; text: string }[] {
  return text
    .split('`')
    .map((part, i) => ({ code: i % 2 === 1, text: part }))
    .filter((s) => s.text);
}

const editions: Record<string, string> = { id, en };
const parsed = new Map<string, Release[]>();

/** History shipped with this build, in `locale` (Indonesian if there is no such edition). */
export function history(locale: string): Release[] {
  const lang = locale in editions ? locale : 'id';
  let releases = parsed.get(lang);
  if (!releases) parsed.set(lang, (releases = parseChangelog(editions[lang])));
  return releases;
}

/** Updater notes in `locale`, else the default notes. */
export function localizedNotes(notes: string | null, i18n: Record<string, string> | undefined, locale: string) {
  return i18n?.[locale] ?? notes;
}

const SEEN_KEY = 'cheesenv.seenVersion';

/**
 * Versions the user has not seen since their last launch, newest first, and
 * marks `current` as seen. Without a record (first launch, or an update from a
 * build before this existed) a returning user still sees the current release.
 */
export function takeUnseen(current: string, returningUser: boolean): string[] {
  let seen: string | null;
  try {
    seen = localStorage.getItem(SEEN_KEY);
    localStorage.setItem(SEEN_KEY, current);
  } catch {
    return [];
  }
  const all = history('id');
  if (seen) return releasesBetween(all, seen, current).map((r) => r.version);
  return returningUser && all.some((r) => r.version === current) ? [current] : [];
}
