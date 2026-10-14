import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {completedTail} from '../run.mjs';
import {payloadHash} from '../../execution/ledger.mjs';

// A comparison is publishable only after both terminal banks consumed the same
// complete source. An open liability is a censored result, never a cash gain.
const [registrationFile, controlDirectory, candidateDirectory, output] = process.argv.slice(2);
if (!output) throw Error('Usage: registration control-directory candidate-directory new-output');
const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));
const registration = read(registrationFile);
const control = completedTail(controlDirectory), candidate = completedTail(candidateDirectory);
const source = path.join(registration.source, 'events.jsonl');
const sourceBytes = fs.readFileSync(source);
const sourceHash = createHash('sha256').update(sourceBytes).digest('hex');
const expected = [registration.controlPolicyHash, registration.candidatePolicyHash];
const banks = [control, candidate];
for (let i = 0; i < banks.length; i++) {
  const bank = banks[i], s = bank.snapshot;
  if (s.continuation?.parentSnapshotHash !== registration.parentSnapshotHash ||
      path.resolve(s.continuation.parentDirectory) !== path.resolve(registration.parent) ||
      s.state.policyHash !== expected[i] ||
      path.resolve(s.source.path) !== path.resolve(source) ||
      s.source.consumedBytes !== sourceBytes.length || s.source.consumedSha256 !== sourceHash)
    throw Error('FORWARD_COMPARISON_SOURCE_OR_POLICY_MISMATCH');
}
const parent = completedTail(registration.parent).snapshot.state;
const journals = banks.map(bank => fs.readFileSync(path.join(bank.directory,'events.jsonl'),'utf8').trim().split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line).event));
if (journals[0].length !== journals[1].length) throw Error('FORWARD_COMPARISON_EVENT_COUNT');
let maximumProcessingSkewMs = 0;
for (let i=0; i<journals[0].length; i++) {
  const {processedAt:a, ...left} = journals[0][i], {processedAt:b, ...right} = journals[1][i];
  if (payloadHash(left) !== payloadHash(right)) throw Error('FORWARD_COMPARISON_EVENT_MISMATCH');
  const skew = Math.abs(Date.parse(a)-Date.parse(b));
  if (!Number.isFinite(skew)) throw Error('FORWARD_COMPARISON_PROCESSING_CLOCK');
  maximumProcessingSkewMs = Math.max(maximumProcessingSkewMs,skew);
}
const amount = value => {const [whole, fraction=''] = value.split('.'); return BigInt(whole + fraction.padEnd(6, '0'));};
const decimal = value => `${value < 0n ? '-' : ''}${(value < 0n ? -value : value)/1000000n}.${((value < 0n ? -value : value)%1000000n).toString().padStart(6, '0')}`;
const result = banks.map((bank, i) => {
  const state = bank.snapshot.state;
  const report = read(path.join(bank.directory, 'report.json'));
  const closed = report.ownedCost === '0.000000' && report.unresolvedOrders === 0 && report.reservedCash === '0.000000';
  return {name:i ? registration.comparisonMode==='entryInside'?'insideEntry':'fixedOffer' : 'control', snapshotHash:bank.snapshot.hash,
    newFills:state.fills.length-parent.fills.length, closed,
    realizedChange:decimal(amount(report.realized)-amount(parent.realized)),
    cash:report.cash, ownedCost:report.ownedCost, unresolved:report.unresolvedOrders,
    classified:closed ? 'closed counterfactual paper result' : 'censored; inventory or unresolved quotes remain'};
});
const document = {schemaVersion:1, at:new Date().toISOString(), registrationHash:payloadHash(registration),
  sourceHash, sourceBytes:sourceBytes.length, identicalEventsVerified:true, maximumProcessingSkewMs,
  banks:result, contestOrders:0, profitabilityEstablished:false,
  limitations:['Separate simulations cannot be summed or treated as actual exchange fills.',
    maximumProcessingSkewMs===0 ? 'Matching processing timestamps verified for every event in this source.' : 'Processing timestamps differ; consumer latency can confound the result.',
    'This finite selected-market trial alone cannot establish scalable profitability.']};
fs.writeFileSync(output, JSON.stringify(document,null,2)+'\n', {flag:'wx'});
console.log(JSON.stringify(document,null,2));
