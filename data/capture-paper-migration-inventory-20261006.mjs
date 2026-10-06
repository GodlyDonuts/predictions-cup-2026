import fs from 'node:fs';import path from 'node:path';
import{capturePaperMigrationInventory}from'../live-paper/paper-migration-inventory.mjs';
const runtimeDir=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
const inventory=capturePaperMigrationInventory({runtimeDir});
fs.writeFileSync('data/paper-migration-inventory-20261006.json',JSON.stringify(inventory,null,2)+'\n');
console.log(JSON.stringify({at:inventory.observedAt,cohorts:inventory.current.map(x=>({cohort:x.cohort,owner:x.authoritativeOwner,status:x.status,cash:Object.fromEntries(Object.entries(x.accounting.wallets).map(([id,w])=>[id,w.cash])),realizedPnl:Object.fromEntries(Object.entries(x.accounting.wallets).map(([id,w])=>[id,w.realizedPnl]))})),cutoverReady:false,accountRequests:0}));
