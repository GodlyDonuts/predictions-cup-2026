import fs from 'node:fs';
import assert from 'node:assert/strict';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';

const evidence=JSON.parse(fs.readFileSync('data/native-sides-milestone-20261004.json'));
assert.equal(evidence.tests.node,163);assert.equal(evidence.tests.rust,9);
assert.equal(evidence.tests.failures,0);assert.equal(evidence.originalWallets.cards,72);
assert.equal(evidence.live.additionalPersistentProcesses,0);
assert.equal(evidence.competitionOrders,0);assert.equal(evidence.profitabilityEstablished,false);
for(const [file,hash] of Object.entries(evidence.sourceHashes))assert.equal(sha(fs.readFileSync(file)),hash,file);
const files=['backend-rust/src/main.rs','backend-rust/src/quote_sides.rs','backend-rust/src/quote_endpoints.rs',
  'backend-rust/build.rs','backend-rust/README.md','docs/BAAL.md',
  'live-paper/native-quote-markouts-fixture.mjs','live-paper/native-quote-markouts.test.mjs',
  'live-paper/native-quote-sides.test.mjs','data/run-native-quote-sides-20261004.mjs',
  'data/finish-native-sides-preservation-20261004.mjs','data/record-native-sides-milestone-20261004.mjs',
  'data/prepare-native-sides-publication-20261004.mjs','data/native-sides-milestone-20261004.json',
  'data/native-sides-preservation-20261004.json'];
const elements=files.map(path=>{
  const content=fs.readFileSync(path,'utf8');
  assert(!/(?:ace_|sk-)[a-zA-Z0-9]{15,}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY|Authorization:\s*Bearer [a-zA-Z0-9]/.test(content),'Secret-like content '+path);
  return {path,mode:'100644',type:'blob',content};
});
const pack=JSON.stringify(elements);
fs.writeFileSync('data/native-sides-publication-elements.json',pack);
console.log(JSON.stringify({files:elements.length,chars:pack.length,hash:sha(pack)}));
