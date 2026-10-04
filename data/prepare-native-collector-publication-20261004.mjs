import fs from 'node:fs';import {createHash} from 'node:crypto';
const files=['live-paper/book-publication-v3.mjs','live-paper/book-publication-v3.test.mjs','live-paper/collector-publication-native.mjs','live-paper/runner.mjs',
 'data/prepare-native-collector-20261004.mjs','data/deploy-native-collector-20261004.ps1','data/verify-native-collector-20261004.mjs','data/benchmark-native-collector-20261004.mjs',
 'data/record-native-collector-20261004.mjs','data/prepare-native-collector-publication-20261004.mjs','data/native-collector-preservation-20261004.json',
 'data/native-collector-deployment-20261004.json','data/native-collector-live-20261004.json','data/native-collector-benchmark-20261004.json','data/native-collector-milestone-20261004.json','docs/BAAL.md','backend-rust/README.md'];
const elements=files.map(file=>{const content=fs.readFileSync(file,'utf8');if(/ace_[a-f0-9]{20,}|eyJ[A-Za-z0-9_-]{50,}\./.test(content))throw Error('Secret-like content: '+file);return{path:file,mode:'100644',type:'blob',content};}),text=JSON.stringify(elements);fs.writeFileSync('data/native-collector-publication-elements.json',text);
console.log(JSON.stringify({files:files.length,chars:text.length,hash:createHash('sha256').update(text).digest('hex')}));
