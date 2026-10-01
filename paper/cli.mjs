import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {replay} from './engine.mjs';
import {chronologicalSplit} from './split.mjs';

const digest = text => createHash('sha256').update(text).digest('hex');
export function main(args) {
  if (args.length === 1 && args[0] === '--help') return 'Offline paper replay: node cli.mjs --events LOCAL.jsonl --config LOCAL.json\nNo network, account, credential, order, or --live mode is supported.';
  const options = {};
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i], value = args[i + 1];
    if (!['--events', '--config'].includes(key) || options[key] || !value || value.startsWith('--') || /^[a-z][a-z\d+.-]*:\/\//i.test(value)) throw new Error('Only --events LOCAL.jsonl --config LOCAL.json are supported');
    options[key] = value;
  }
  if (!options['--events'] || !options['--config']) throw new Error('Both --events and --config are required');
  const eventText = readFileSync(options['--events'], 'utf8'); const configText = readFileSync(options['--config'], 'utf8');
  const config = JSON.parse(configText);
  const events = eventText.split(/\r?\n/).filter(line => line.trim()).map((line, i) => {
    try { return JSON.parse(line); } catch { throw new Error(`Invalid event JSON at nonempty line ${i + 1}`); }
  });
  const report = replay(events, config);
  // No strategy learns from the test segment: this is a scripted synthetic replay.
  const unique = [...new Map(events.map(e => [e.eventId, e])).values()];
  const splitManifest = chronologicalSplit(unique, config.split);
  const noTrade = replay(events.filter(e => !['submit', 'cancel'].includes(e.type)), config);
  return JSON.stringify({schemaVersion: 1, experiment: 'scripted_passive_vs_no_trade', datasetClaim: 'Uncalibrated replay; fixture is synthetic, not empirical performance',
    inputs: {eventFileSha256: digest(eventText), configFileSha256: digest(configText)}, splitManifest,
    scriptedPassive: report, noTrade: {cash: noTrade.cash, realizedPnl: noTrade.realizedPnl, totalFees: noTrade.totalFees,
      referenceEquity: noTrade.referenceEquity, metrics: noTrade.metrics}}, null, 2);
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try { process.stdout.write(main(process.argv.slice(2)) + '\n'); }
  catch (error) { process.stderr.write(`Offline replay failed: ${error.message}\n`); process.exitCode = 1; }
}
