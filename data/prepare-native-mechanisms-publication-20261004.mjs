import fs from 'node:fs';
import assert from 'node:assert/strict';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';

const e=JSON.parse(fs.readFileSync('data/native-mechanisms-evidence-20261004.json'));
for(const [file,hash] of Object.entries(e.protocol.sourceHashes))assert.equal(sha(fs.readFileSync(file)),hash,file);
const milestone=JSON.parse(fs.readFileSync('data/native-mechanisms-milestone-20261004.json'));
assert.equal(milestone.tests.node,169);assert.equal(milestone.tests.failures,0);
assert.equal(milestone.preservation.originalLiveWalletCardsExact,72);
assert.equal(milestone.competitionOrders,0);assert.equal(milestone.profitabilityEstablished,false);
const files=['backend-rust/src/main.rs','backend-rust/src/quote_markouts.rs','backend-rust/src/quote_mechanisms.rs',
 'backend-rust/build.rs','backend-rust/README.md','docs/BAAL.md',
 'live-paper/native-quote-markouts-fixture.mjs','live-paper/native-quote-mechanisms.test.mjs',
 'data/prepare-native-mechanisms-20261004.mjs','data/run-native-mechanisms-20261004.mjs',
 'data/record-native-mechanisms-milestone-20261004.mjs','data/prepare-native-mechanisms-publication-20261004.mjs',
 'data/native-mechanisms-protocol-20261004.json','data/native-mechanisms-evidence-20261004.json',
 'data/native-mechanisms-future-1791141201465.json','data/native-mechanisms-milestone-20261004.json'];
const elements=files.map(path=>{
 const content=fs.readFileSync(path,'utf8');
 assert(!/(?:ace_|sk-)[a-zA-Z0-9]{15,}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY|Authorization:\s*Bearer [a-zA-Z0-9]/.test(content),'Secret-like content '+path);
 return{path,mode:'100644',type:'blob',content};
});
const pack=JSON.stringify(elements);fs.writeFileSync('data/native-mechanisms-publication-elements.json',pack);
console.log(JSON.stringify({files:elements.length,chars:pack.length,hash:sha(pack)}));
