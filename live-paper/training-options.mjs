import {ROUND_TRIP, LEGACY_TARGET, targetPolicy} from './roundtrip-target.mjs';
import {ISOLATED, historyPolicy} from './history-policy.mjs';
import {parseTrainingHorizons} from './training-horizons.mjs';

// New training uses executable targets. Frozen bank interpretation remains
// unchanged, and historical options can still be requested explicitly.
export function trainingOptions(argv) {
  const valueFlags = new Set(['--journal', '--config', '--out', '--family', '--history-policy', '--since', '--target-policy', '--training-scope','--horizons']);
  const switches = new Set(['--require-instrument-coverage', '--allow-undercovered-exploration', '--breadth-context']);
  const args = new Map();
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    if ((!valueFlags.has(flag) && !switches.has(flag)) || args.has(flag)) throw Error('Unknown or repeated training option: ' + flag);
    if (switches.has(flag)) args.set(flag, true);
    else {
      const value = argv[++i];
      if (!value || value.startsWith('--')) throw Error('Required value for ' + flag);
      args.set(flag, value);
    }
  }
  for (const flag of ['--journal', '--config', '--out']) if (!args.has(flag)) throw Error('Required ' + flag);
  if (args.has('--require-instrument-coverage') && args.has('--allow-undercovered-exploration')) throw Error('Conflicting coverage options');
  const target = targetPolicy(args.get('--target-policy') ?? ROUND_TRIP);
  const coverage = !args.has('--allow-undercovered-exploration');
  const scope = args.get('--training-scope') ?? (target === ROUND_TRIP && coverage ? 'supported-instruments-v1' : 'all-markets-v1');
  if (!['all-markets-v1', 'supported-instruments-v1'].includes(scope)
      || scope === 'supported-instruments-v1' && (!coverage || target === LEGACY_TARGET)) throw Error('Supported scope requires coverage-gated roundtrip target');
  const family = args.get('--family') ?? 'ridge';
  if (!['ridge', 'boosted_stumps'].includes(family)) throw Error('Unsupported model family');
  const since = args.get('--since') ?? null;
  if (since !== null && !Number.isFinite(Date.parse(since))) throw Error('Invalid training window start');
  return {
    journal: args.get('--journal'), config: args.get('--config'), out: args.get('--out'),
    options: {family, targetPolicy: target, historyPolicy: historyPolicy(args.get('--history-policy') ?? ISOLATED),
      since, requireInstrumentCoverage: coverage, breadthContext: args.has('--breadth-context'), trainingScope: scope,...(args.has('--horizons')?{horizons:parseTrainingHorizons(args.get('--horizons'))}:{})},
  };
}
