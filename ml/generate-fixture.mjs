#!/usr/bin/env node
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { FORMAT, hash } from './core.mjs';
import { random } from './model.mjs';
export const FIXTURE_SEED = 20261001;
export function fixture(seed = FIXTURE_SEED) {
  const rng = random(seed), start = Date.parse('2026-10-01T00:00:00.000Z');
  const timestamp = ms => new Date(start + ms).toISOString();
  const rows = [{ format: FORMAT, schemaVersion: 1, datasetId: 'synthetic-correctness-v1', dataKind: 'synthetic', normalization: 'quality-segmented-books-v1', provenance: { sourceSha256: hash(`SYNTHETIC_GENERATOR_V1 seed=${seed}; no market source`), normalizerVersion: 'synthetic-fixture-v1' } }];
  for (let s = 0; s < 16; s++) {
    let mid = .45, priorImbalance = 0;
    for (let i = 0; i < 24; i++) {
      const imbalance = rng() * 1.6 - .8;
      // Deliberately manufactured signal solely to exercise the fitting path.
      mid = Math.max(.2, Math.min(.8, mid + .002 * priorImbalance + (rng() - .5) * .001));
      priorImbalance = imbalance;
      const time = s * 30000 + i * 1000;
      rows.push({ kind: 'book', rowId: `synthetic-${s}-${i}`, sessionId: `synthetic-session-${s}`, tournamentId: 'synthetic-cup', marketId: 'synthetic-market', exchangeId: 'synthetic-exchange', segmentId: `synthetic-segment-${s}`, receivedAt: timestamp(time + 10), availableAt: timestamp(time + 20), sourceAt: timestamp(time), sequence: s * 1000 + i,
        quality: { continuous: true, recovering: false, stale: false, expiryKnown: true }, nextExpiryAt: null,
        bids: [{ price: Number((mid - .005).toFixed(8)), quantity: Number((100 * (1 + imbalance)).toFixed(8)) }],
        asks: [{ price: Number((mid + .005).toFixed(8)), quantity: Number((100 * (1 - imbalance)).toFixed(8)) }] });
    }
  }
  return rows.map(r => JSON.stringify(r)).join('\n') + '\n';
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 3 || process.argv[2].includes('://') || process.argv[2].startsWith('-')) { process.stderr.write('One new local output path is required\n'); process.exitCode = 1; }
  else { try { writeFileSync(process.argv[2], fixture(), { flag: 'wx', mode: 0o600 }); } catch { process.stderr.write('Could not create exclusive fixture output\n'); process.exitCode = 1; } }
}
