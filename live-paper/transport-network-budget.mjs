import fs from 'node:fs';

// Operator coordination is separate from every frozen strategy configuration.
// All account requests still use one serialized client; there is no token burst.
const baseKeys = ['schemaVersion', 'ownerId', 'enabled', 'intervalMs', 'soleAccountClient', 'formerWriterStopCommentId'];
export function validateNetworkBudget(value) {
  if(value?.schemaVersion===3){
    const keys=['schemaVersion','ownerId','enabled','intervalMs','soleAccountClient','executionOwner','ackCommentId','maxReadsPerMinute','writesEnabled'];
    if(Object.keys(value).length!==keys.length||Object.keys(value).some(k=>!keys.includes(k))||value.ownerId!=='Codex'||value.enabled!==true||value.intervalMs!==3000||value.soleAccountClient!==false||value.executionOwner!=='Daenerys'||value.ackCommentId!==5966761902||value.maxReadsPerMinute!==20||value.writesEnabled!==false)throw Error('Invalid shared read-only network budget');
    return Object.freeze({...value});
  }
  const keys = value?.schemaVersion === 2 ? [...baseKeys, 'writeIntervalMs'] : baseKeys;
  if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).length !== keys.length
    || Object.keys(value).some(key => !keys.includes(key)) || ![1,2].includes(value.schemaVersion) || value.ownerId !== 'Codex'
    || value.enabled !== true || value.soleAccountClient !== true || value.formerWriterStopCommentId !== 5947535773
    || (value.schemaVersion === 1 ? value.intervalMs !== 3000 : value.intervalMs !== 1000 || value.writeIntervalMs !== 2500)) throw Error('Invalid coordinated network budget');
  return Object.freeze({...value});
}
export function networkWriteInterval(interval = 6000, budget = null) {
  const read = networkInterval(interval, budget);
  return budget?.schemaVersion === 2 ? validateNetworkBudget(budget).writeIntervalMs : read;
}
export function loadNetworkBudget(file) {
  return fs.existsSync(file) ? validateNetworkBudget(JSON.parse(fs.readFileSync(file, 'utf8'))) : null;
}
export function networkInterval(interval = 6000, budget = null) {
  if (!Number.isSafeInteger(interval) || interval < 5000) throw Error('Read budget requires >=5 second pacing unless explicitly coordinated');
  return budget === null ? interval : validateNetworkBudget(budget).intervalMs;
}
