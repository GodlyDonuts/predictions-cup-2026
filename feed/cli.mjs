import fs from 'node:fs';
import {MarketCollector} from './collector.mjs';
import {Capture} from './capture.mjs';
import {config, requireThat, timestamp} from './contract.mjs';

export async function replay(text, {capture = null, stop = () => false} = {}) {
  let now = 0, collector = null, ticket = null, count = 0;
  for (const line of text.split(/\r?\n/)) {
    if (count++ % 128 === 0) await new Promise(setImmediate);
    if (stop()) { collector?.stop(); break; }
    if (!line.trim()) continue;
    requireThat(line.length <= 8 * 1024 * 1024, 'FRAME_TOO_LARGE');
    const row = JSON.parse(line); requireThat(row.format === 'sig-feed-replay' && row.schemaVersion === 1, 'UNKNOWN_REPLAY_SCHEMA');
    const at = Date.parse(timestamp(row.observedAt)); requireThat(at >= now, 'NONMONOTONIC_RECEIPT'); now = at;
    if (row.kind === 'config') { requireThat(!collector, 'DUPLICATE_CONFIG'); collector = new MarketCollector(config(row.scope), {now: () => now, capture, staleMs: row.staleMs ?? 10000}); continue; }
    requireThat(collector, 'CONFIG_REQUIRED');
    if (row.kind === 'subscribed') collector.subscribed(row.reason ?? 'INITIAL');
    else if (row.kind === 'snapshot') { ticket = collector.beginRecovery(); collector.completeRecovery(ticket, row.payload); }
    else if (row.kind === 'event') collector.ingest(row.event, row.payload, {receivedAt: row.observedAt});
    else if (row.kind === 'disconnect') collector.disconnected();
    else if (row.kind === 'tick') collector.tick();
    else if (row.kind === 'stop') collector.stop();
    else throw new Error('UNSUPPORTED_REPLAY_RECORD');
  }
  requireThat(collector, 'EMPTY_REPLAY'); const result = collector.status(); capture?.append({kind: 'summary', observedAt: new Date(now).toISOString(), result}); return result;
}
export async function main(args = process.argv.slice(2)) {
  // Fail before reading inputs, environment variables, or creating output.
  if (!args.length || (args.length === 1 && args[0] === '--help')) {
    console.log('Collection is disabled. Offline only: node feed/cli.mjs --replay feed/fixtures/scenario.jsonl [--capture new.jsonl]'); return 0;
  }
  if (args[0] !== '--replay' || !args[1] || ![2,4].includes(args.length) || (args.length === 4 && (args[2] !== '--capture' || !args[3]))) throw new Error('ONLY_OFFLINE_REPLAY_SUPPORTED');
  if (fs.existsSync('STOP')) { console.log(JSON.stringify({state: 'stopped', reason: 'STOP_PRESENT', networkRequests: 0})); return 0; }
  let stopped = false; const onStop = () => { stopped = true; };
  process.on('SIGINT', onStop); process.on('SIGTERM', onStop);
  let capture = null;
  try {
    requireThat(fs.statSync(args[1]).size <= 64 * 1024 * 1024, 'REPLAY_TOO_LARGE');
    if (args[3]) capture = new Capture(args[3]);
    console.log(JSON.stringify(await replay(fs.readFileSync(args[1], 'utf8'), {capture, stop: () => stopped || fs.existsSync('STOP')}), null, 2));
    return 0;
  } finally { capture?.close(); process.off('SIGINT', onStop); process.off('SIGTERM', onStop); }
}
if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  try { process.exitCode = await main(); } catch { console.error('Feed replay failed: invalid input or unavailable output. No raw payload/error is logged.'); process.exitCode = 1; }
}
