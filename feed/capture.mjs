import fs from 'node:fs';
import {CAPTURE_SCHEMA_VERSION} from './contract.mjs';
const sensitive = /(?:authorization|cookie|token|secret|password|api.?key|anon.?key|credential|headers)/i;
const tokenLike = /(?:Bearer\s+\S+|eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+|(?:sk|sig)[_-][A-Za-z0-9_-]{12,})/gi;
export function sanitize(value, depth = 0) {
  if (depth > 30) return '[REDACTED_DEPTH]';
  if (typeof value === 'string') return value.replace(tokenLike, '[REDACTED]');
  if (Array.isArray(value)) return value.map(v => sanitize(v, depth + 1));
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).filter(([k]) => !sensitive.test(k)).map(([k,v]) => [k, sanitize(v, depth + 1)]));
  return value;
}
// A new exclusive file per session; never truncate or reopen an old capture.
// Only normalized contract/state records reach this sink; no raw frame/error/header logging.
export class Capture {
  constructor(path, {sync = true} = {}) { this.fd = fs.openSync(path, 'wx', 0o600); this.sync = sync; this.index = 0; }
  append(record) {
    if (this.fd === null) throw new Error('CAPTURE_CLOSED');
    const row = {format: 'sig-feed-capture', schemaVersion: CAPTURE_SCHEMA_VERSION, captureIndex: this.index++, ...sanitize(record)};
    fs.writeSync(this.fd, JSON.stringify(row) + '\n');
    if (this.sync) fs.fsyncSync(this.fd);
  }
  close() { if (this.fd !== null) { fs.closeSync(this.fd); this.fd = null; } }
}
