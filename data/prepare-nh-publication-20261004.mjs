import fs from 'node:fs';import {createHash} from 'node:crypto';
const files=['live-paper/no-maker-paper-v3.mjs','live-paper/no-maker-paper-v3.test.mjs','live-paper/no-maker-host-v5.mjs','live-paper/no-maker-host-v5.test.mjs',
 'live-paper/quote-projection-recovery.mjs','live-paper/quote-projection-recovery.test.mjs','live-paper/canonical-no-capacity-v2.mjs','live-paper/canonical-no-capacity-v2.test.mjs',
 'live-paper/baal-pair-host-v3.mjs','live-paper/baal-pair-host-v3.test.mjs','live-paper/paper-book-continuation-frontier.mjs','live-paper/paper-book-continuation-frontier.test.mjs',
 'live-paper/runner.mjs','live-paper/trade-tape.mjs','data/prepare-nh-paper-v5-20261004.mjs','data/deploy-nh-paper-v5-20261004.ps1','data/verify-nh-paper-v5-20261004.mjs',
 'data/record-nh-milestone-20261004.mjs','data/prepare-nh-publication-20261004.mjs','data/nh-source-capture-20261004.json','data/nh-conditional-refund-review-20261004.json',
 'data/no-maker-bank-20261004-v5.json','data/baal-pair-bank-20261004-v3.json','data/nh-paper-v5-preservation-20261004.json','data/nh-paper-v5-deployment-20261004.json',
 'data/nh-paper-v5-live-20261004.json','data/nh-paper-v5-milestone-20261004.json','docs/BAAL.md'];
const elements=files.map(file=>{const content=fs.readFileSync(file,'utf8');if(/ace_[a-f0-9]{20,}|eyJ[A-Za-z0-9_-]{50,}\./.test(content))throw Error('Secret-like content: '+file);return{path:file,mode:'100644',type:'blob',content};});
const text=JSON.stringify(elements);fs.writeFileSync('data/nh-publication-elements.json',text);
console.log(JSON.stringify({files:files.length,chars:text.length,hash:createHash('sha256').update(text).digest('hex')}));
