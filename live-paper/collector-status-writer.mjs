import {atomicJson} from '../execution/ledger.mjs';
// These are derived UI/health projections. Preserve their existing JSON shape
// (including omission of optional undefined fields) before the durable writer.
// Authoritative journals, accounting and model bindings do not use this adapter.
export function writeCollectorStatus(file,value){atomicJson(file,JSON.parse(JSON.stringify(value)));}
