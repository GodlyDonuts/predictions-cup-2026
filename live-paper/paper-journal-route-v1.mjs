import fs from 'node:fs';
import path from 'node:path';

const DEFAULT_JOURNAL = 'journal.jsonl';
const SAFE_JOURNAL_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,119}\.jsonl$/i;
const WINDOWS_DEVICE_NAME = /^(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/i;

export function resolvePaperJournalRoute(runtimeDir, journalFileName) {
  if (typeof runtimeDir !== 'string' || !runtimeDir || !path.isAbsolute(runtimeDir)) {
    throw new Error('Paper runtime directory must be absolute');
  }

  const root = path.resolve(runtimeDir);
  const isNative = journalFileName === undefined;
  if (!isNative && (
    typeof journalFileName !== 'string' ||
    journalFileName !== journalFileName.trim() ||
    !SAFE_JOURNAL_NAME.test(journalFileName) ||
    WINDOWS_DEVICE_NAME.test(journalFileName) ||
    journalFileName === DEFAULT_JOURNAL
  )) {
    throw new Error('Invalid paper journal filename');
  }

  const fileName = isNative ? DEFAULT_JOURNAL : journalFileName;
  const file = path.resolve(root, fileName);
  if (path.dirname(file) !== root) throw new Error('Paper journal escapes runtime directory');

  if (!isNative && fs.existsSync(file)) {
    const stat = fs.lstatSync(file);
    if (stat.isSymbolicLink() || !stat.isFile()) throw new Error('Named paper journal must be a regular file');
  }

  return Object.freeze({ file, fileName, nativeJournal: isNative });
}
