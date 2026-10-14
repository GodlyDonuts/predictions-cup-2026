import fs from 'node:fs';
import path from 'node:path';
import {runQueueDepletionCapture} from './run-queue-depletion-roundtrip-20261013.mjs';

const captureName = 'realtime-2026-10-13T21-15-52-245Z-d02873df-952c-4bca-96b2-d52dbef272ba.jsonl';
const runtimeRoot = process.env.LOCALAPPDATA;
if (!runtimeRoot) throw Error('QUEUE_FORWARD_RUNTIME_ROOT');
const source = path.join(runtimeRoot, 'SigTradingCup', 'paper-runtime', captureName);
const output = new URL('./queue-depletion-forward-20261014.json', import.meta.url);
if (fs.existsSync(output)) throw Error('QUEUE_OUTPUT_EXISTS');

const result = runQueueDepletionCapture({file: source, output});
console.log(JSON.stringify({
  verdict: result.verdict,
  counts: result.counts,
  aggregate: result.aggregate,
  source: result.source,
  runtimeChanges: 0,
  apiRequests: 0,
  contestOrders: 0,
  actualFillsProven: false,
  profitabilityEstablished: false,
}, null, 2));
