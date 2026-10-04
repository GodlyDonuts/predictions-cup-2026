import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {payloadHash} from '../execution/ledger.mjs';

const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const need = (ok, code) => { if (!ok) throw Error('FROZEN_PAPER_' + code); };
const safePath = (root, relative) => {
  need(typeof relative === 'string' && /^[a-z0-9/._-]+\.mjs$/i.test(relative), 'PATH');
  const target = path.resolve(root, relative);
  need(target.startsWith(path.resolve(root) + path.sep), 'PATH_SCOPE');
  return target;
};

// Frozen wallets use the exact reducers and dependencies captured at deployment.
// Collector transport can evolve independently; a cache is never a substitute
// for the bank's own source identity, prefix, journal and accounting checks.
export async function frozenPaperModule({runtimeDir, bank, modulePath, fallback}) {
  const indexFile = path.join(runtimeDir, 'frozen-code', 'index.json');
  if (!fs.existsSync(indexFile)) return fallback;
  const index = JSON.parse(fs.readFileSync(indexFile));
  const binding = index.banks[payloadHash(bank)];
  if (!binding) return fallback;
  need(/^[a-f0-9]{64}$/.test(binding.snapshot), 'SNAPSHOT');
  const root = path.join(runtimeDir, 'frozen-code', binding.snapshot);
  const manifest = JSON.parse(fs.readFileSync(path.join(root, 'manifest.json')));
  need(payloadHash(manifest.files) === binding.snapshot, 'MANIFEST');
  for (const [file, hash] of Object.entries(manifest.files)) {
    need(sha(fs.readFileSync(safePath(root, file))) === hash, 'FILE_CHANGED');
  }
  need(manifest.files[modulePath], 'MODULE');
  for (const [relative, hash] of Object.entries(bank.codeIdentity)) {
    const file = path.posix.normalize(path.posix.join('live-paper', relative));
    need(manifest.files[file] === hash, 'BANK_IDENTITY');
  }
  return import(pathToFileURL(safePath(root, modulePath)).href);
}
