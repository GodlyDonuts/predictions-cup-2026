import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const configUrl = new URL('./config.json', import.meta.url);

test('live-data paper baseline uses the competition fee-free assumption', async () => {
  const config = JSON.parse(await readFile(fileURLToPath(configUrl), 'utf8'));

  assert.equal(config.feeBps, 0);
  assert.equal(String(config.feePerShare ?? '0'), '0');
});
