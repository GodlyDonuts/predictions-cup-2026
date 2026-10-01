import { readFileSync } from 'node:fs';
import { fixture } from '../generate-fixture.mjs';
import { parseDataset, hash } from '../core.mjs';
export const config = JSON.parse(readFileSync(new URL('../fixtures/config.json', import.meta.url)));
export const runtime = { implementationSha256: hash('unit-test-runtime') };
export const values = () => fixture().trimEnd().split('\n').map(JSON.parse);
export const encode = rows => rows.map(JSON.stringify).join('\n') + '\n';
export const dataset = rows => parseDataset(encode(rows));
export const at = ms => new Date(Date.parse('2026-10-01T00:00:00.000Z') + ms).toISOString();
export function shortRows(n = 12) { return values().slice(0, n + 1); }
export function shiftRow(row, ms) { row.receivedAt = at(ms + 10); row.availableAt = at(ms + 20); row.sourceAt = at(ms); return row; }
export function barrier(row, reason = 'gap') { const { kind, sourceAt, sequence, quality, nextExpiryAt, bids, asks, ...rest } = row; return { ...rest, kind: 'barrier', reason }; }
