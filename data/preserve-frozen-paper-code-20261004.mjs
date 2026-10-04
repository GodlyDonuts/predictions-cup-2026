import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {payloadHash, atomicJson} from '../execution/ledger.mjs';
const runtime = path.join(process.env.LOCALAPPDATA, 'SigTradingCup/paper-runtime');
const workspace = path.resolve('.');
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const banks = [];
for (const folder of ['no-maker-cohorts', 'baal-pair-cohorts', 'probability-receipt-cohorts']) {
  for (const name of fs.readdirSync(path.join(runtime, folder))) {
    const file = path.join(runtime, folder, name, 'bank.json');
    if (fs.existsSync(file)) banks.push(JSON.parse(fs.readFileSync(file)));
  }
}
const files = {}, bytes = {};
function capture(relative) {
  relative = path.posix.normalize(relative.replaceAll('\\', '/'));
  if (files[relative]) return;
  if (!relative.endsWith('.mjs') || relative.startsWith('../') || path.isAbsolute(relative)) throw Error('Source scope');
  const data = fs.readFileSync(path.join(workspace, relative));
  files[relative] = sha(data); bytes[relative] = data;
  const source = data.toString();
  for (const match of source.matchAll(/(?:from\s*|import\s*\()\s*['"](\.{1,2}\/[^'"]+\.mjs)['"]/g)) {
    capture(path.posix.join(path.posix.dirname(relative), match[1]));
  }
}
for (const bank of banks) for (const [relative, hash] of Object.entries(bank.codeIdentity)) {
  const file = path.posix.normalize(path.posix.join('live-paper', relative));
  capture(file);
  if (files[file] !== hash) throw Error('Existing frozen bank source mismatch: ' + bank.name);
}
const snapshot = payloadHash(files), root = path.join(runtime, 'frozen-code', snapshot);
if (!fs.existsSync(root)) {
  fs.mkdirSync(root, {recursive:true, mode:0o700});
  for (const [file, data] of Object.entries(bytes)) {
    fs.mkdirSync(path.dirname(path.join(root, file)), {recursive:true, mode:0o700});
    fs.writeFileSync(path.join(root, file), data, {flag:'wx', mode:0o600});
  }
  atomicJson(path.join(root, 'manifest.json'), {schemaVersion:1, files});
}
for (const [file, hash] of Object.entries(files)) if (sha(fs.readFileSync(path.join(root, file))) !== hash) throw Error('Snapshot verification');
const indexFile = path.join(runtime, 'frozen-code', 'index.json');
const index = fs.existsSync(indexFile) ? JSON.parse(fs.readFileSync(indexFile)) : {schemaVersion:1, banks:{}};
for (const bank of banks) {
  const hash = payloadHash(bank);
  if (index.banks[hash] && index.banks[hash].snapshot !== snapshot) throw Error('Existing binding differs');
  index.banks[hash] = {snapshot};
}
atomicJson(indexFile, index);
const receipt = {at:new Date().toISOString(), snapshot, files, banks:banks.map(b=>({cohort:b.name, bankHash:payloadHash(b)})), accountingReset:false, competitionOrders:0};
fs.writeFileSync('data/baal-frozen-code-preservation-20261004.json', JSON.stringify(receipt,null,2)+'\n');
console.log(JSON.stringify({snapshot, files:Object.keys(files).length, banks:banks.length, verified:true}));
