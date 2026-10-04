import fs from 'node:fs';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';
const files=[
 'backend-rust/src/main.rs','backend-rust/build.rs','backend-rust/src/settlement_value.rs',
 'live-paper/native-settlement-value.test.mjs','live-paper/settlement-value-native.mjs','live-paper/settlement-value-native.test.mjs','live-paper/probability-native-host.test.mjs','live-paper/settlement-value-policy.md',
 'data/archive-checked-native-graph.mjs','data/check-settlement-value-20261004.mjs','data/verify-settlement-native-archive-20261004.mjs','data/capture-kansas-lead-20261004.mjs','data/record-settlement-value-milestone-20261004.mjs','data/prepare-settlement-value-publication-20261004.mjs',
 'data/settlement-value-milestone-20261004.json','data/settlement-value-snapshot-1791144422978.json','data/settlement-native-preservation-20261004.json','data/kansas-source-lead-20261004.json','data/kansas-verified-quotes-20261004.json'
];
const elements=files.map(file=>{const content=fs.readFileSync(file,'utf8');if(/ace_[a-f\d]{20,}|sk-[a-z\d]{20,}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|[?&](?:token|access_token)=/i.test(content))throw Error('Secret scan rejected publication: '+file);return{path:file,mode:'100644',type:'blob',content};});
fs.writeFileSync('data/settlement-value-publication-elements.json',JSON.stringify(elements)+'\n');
fs.writeFileSync('data/settlement-value-publication-hashes.json',JSON.stringify(Object.fromEntries(elements.map(e=>[e.path,sha(e.content)])))+'\n');
console.log(JSON.stringify({files:files.length,characters:JSON.stringify(elements).length,paths:files,credentialsIncluded:false,rawAccountIncluded:false}));
