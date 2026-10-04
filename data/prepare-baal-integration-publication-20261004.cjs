const fs=require('node:fs');
const files=[
 'live-paper/maker-read-plan.mjs','live-paper/maker-read-plan.test.mjs','live-paper/baal-read-attention.mjs','live-paper/baal-read-attention.test.mjs','live-paper/trade-tape.mjs','live-paper/trade-tape.test.mjs','live-paper/runner.mjs',
 'live-paper/baal-pair-paper.mjs','live-paper/baal-pair-paper.test.mjs','live-paper/baal-pair-host.mjs','live-paper/baal-pair-host.test.mjs','live-paper/baal-pair-paper-v2.mjs','live-paper/baal-pair-paper-v2.test.mjs','live-paper/baal-pair-host-v2.mjs','live-paper/baal-pair-host-v2.test.mjs',
 'live-paper/frozen-paper-runtime.mjs','live-paper/frozen-paper-runtime.test.mjs','live-paper/no-maker-host-v4.mjs','live-paper/no-maker-host-v4.test.mjs',
 'frontend/app/baal-pair-view.mjs','frontend/baal-pair-view.test.mjs','frontend/app/api/wallets/route.js','frontend/app/comparison-view.mjs','frontend/app/page.js','docs/BAAL.md',
 'data/preserve-baal-attention-20261004.mjs','data/deploy-baal-attention-20261004.ps1','data/baal-attention-preservation-20261004.json','data/baal-attention-deployment-20261004.json','data/verify-baal-attention-20261004.mjs','data/baal-attention-live-verification-20261004.json',
 'data/audit-baal-maker-roundtrip-20261004.mjs','data/baal-maker-first-roundtrip-20261004.json','data/canonical-entry-exit-subsequent-20261004.json',
 'data/prepare-baal-pair-20261004.mjs','data/deploy-baal-pair-20261004.ps1','data/baal-pair-bank-20261004-v1.json','data/baal-pair-preservation-20261004.json','data/baal-pair-deployment-20261004.json',
 'data/prepare-baal-pair-v2-20261004.mjs','data/deploy-baal-pair-v2-20261004.ps1','data/baal-pair-bank-20261004-v2.json','data/baal-pair-v1-preservation-20261004.json','data/baal-pair-depth-deployment-20261004.json',
 'data/deploy-baal-pair-dashboard-20261004.ps1','data/baal-pair-dashboard-deployment-20261004.json','data/preserve-frozen-paper-code-20261004.mjs','data/baal-frozen-code-preservation-20261004.json',
 'data/recover-baal-services-20261004.ps1','data/baal-service-recovery-20261004.json','data/verify-baal-recovery-20261004.mjs','data/baal-recovery-verification-20261004.json','data/prepare-no-maker-v4-20261004.mjs',
 'data/baal-integration-monitor-refs-20261004.json','data/baal-integration-milestone-20261004.json','data/prepare-baal-integration-publication-20261004.cjs','data/quant-development-state.json','data/daenerys-monitor.json','data/current-runtime-pids.json',
];
const elements=files.map(path=>({path,mode:'100644',type:'blob',content:fs.readFileSync(path,'utf8')}));
for(const f of elements)if(/ace_[a-f0-9]{32,}|Bearer\s+[A-Za-z0-9_-]{20,}|BEGIN\s+PRIVATE\s+KEY/i.test(f.content))throw Error('Secret scan failed: '+f.path);
const pack={parent:'2b7fc72302aaf07874e55467bb2671ab96a554ca',baseTree:'efe5bff6d1d72ffa67c0b7bcf8238446207d753f',elements};
fs.writeFileSync('data/baal-integration-publication-pack-20261004.json',JSON.stringify(pack));
console.log(JSON.stringify({files:elements.length,characters:JSON.stringify(pack).length,secretScan:'passed'}));
