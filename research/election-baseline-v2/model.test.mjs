import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { senateRaces, raceFeatures, parseCsv, buildRows, presidentialTables } from './model.mjs';
const row = (name, party, votes, extra = {}) => ({ office:'US SENATE',year:'2020',state_po:'WV',stage:'gen',special:'False',unofficial:'False',candidate:name,party_detailed:party,party_simplified:party,candidatevotes:String(votes),totalvotes:'1000',mode:'TOTAL',writein:'False',...extra });

test('independent winner remains an observed Republican loss, with fusion lines merged', () => {
  const s = senateRaces([row('Republican','REPUBLICAN',200),row('Republican','REPUBLICAN',100,{party_detailed:'CONSERVATIVE'}),row('Independent','OTHER',600),row('Democrat','DEMOCRAT',100)]);
  const r=s.races.get('2020:WV'); assert.equal(r.winner.name,'Independent'); assert.equal(r.republican.votes,300); assert.equal(r.margin,-1/3);
});
test('certified runoff supersedes general and keeps first-round entrant identities', () => {
  const s=senateRaces([row('R','REPUBLICAN',490,{state_po:'GA'}),row('D','DEMOCRAT',480,{state_po:'GA'}),row('I','OTHER',30,{state_po:'GA'}),row('R','REPUBLICAN',450,{state_po:'GA',stage:'GEN RUNOFF'}),row('D','DEMOCRAT',550,{state_po:'GA',stage:'GEN RUNOFF'})]);
  const r=s.races.get('2020:GA'); assert.equal(r.margin,-.1); assert(r.entrants.includes('I')); assert.equal(r.selectedStage,'gen runoff');
});
test('unverified majority-runoff plurality cannot be treated as final winner', () => {
  const s=senateRaces([row('R','REPUBLICAN',490,{state_po:'GA'}),row('D','DEMOCRAT',480,{state_po:'GA'}),row('I','OTHER',30,{state_po:'GA'})]);
  assert.equal(s.races.size,0); assert.equal(s.exclusions[0].reason,'majority_runoff_final_unverified');
});
test('duplicate candidate ballot line cannot merge rounds silently', () => {
  const s=senateRaces([row('R','REPUBLICAN',400),row('R','REPUBLICAN',100),row('D','DEMOCRAT',500)]);
  assert.equal(s.exclusions[0].reason,'duplicate_ballot_line_mode');
});
test('competitive unknown party, cross-major fusion and unverifiable RCV are explicit exclusions', () => {
  assert.equal(senateRaces([row('Unknown','OTHER',500,{party_detailed:''}),row('R','REPUBLICAN',300),row('D','DEMOCRAT',200)]).exclusions[0].reason,'competitive_party_affiliation_unverified');
  assert.equal(senateRaces([row('Same','REPUBLICAN',600),row('Same','DEMOCRAT',400)]).exclusions[0].reason,'cross_major_fusion');
  assert.equal(senateRaces([row('R','REPUBLICAN',600,{state_po:'AK',year:'2022'}),row('D','DEMOCRAT',400,{state_po:'AK',year:'2022'})]).exclusions[0].reason,'ranked_choice_final_margin_unverified');
});
test('minor unknown affiliation is allowed only when neither possible affiliation changes the target',()=>{
  const s=senateRaces([row('Independent','OTHER',550),row('R','REPUBLICAN',400),row('Unknown','OTHER',50,{party_detailed:''})]);
  assert.equal(s.races.get('2020:WV').winner.name,'Independent');assert.equal(s.races.get('2020:WV').margin,(400-550)/950);
});
test('incumbency uses first-round entrants, not an outcome-selected strongest challenger', () => {
  const p={years:[2012,2016,2020],national:new Map([2012,2016,2020].map(y=>[y,{margin:0}])),states:new Map([['2016:WV',{margin:.3}]])};
  const s={races:new Map([['2014:WV',{year:2014,margin:.1,winner:{id:'INCUMBENT',republican:false}}]])};
  const f=raceFeatures({year:2020,state:'WV',entrants:['Incumbent','Challenger']},s,p); assert.equal(f.incumbent,-1); assert.equal(f.clocks.presYear,2016);
});
test('real source corrections produce final Georgia loss and separate Louisiana final margins', () => {
  const overrides=JSON.parse(fs.readFileSync(new URL('./final-stage-overrides.json',import.meta.url)));
  const s=senateRaces(parseCsv(fs.readFileSync('research/election-baseline-v1/sources/1976-2024-senate-state.csv','utf8')),overrides);
  assert.equal(s.races.get('2020:GA').winner.name,'JON OSSOFF'); assert.equal(s.races.get('2020:GA').total,4484902); assert(!s.races.has('2021:GA'));
  assert.equal(s.races.get('2014:LA').total,1273589); assert.equal(s.races.get('2002:LA').total,1235296); assert.equal(s.races.get('2016:LA').total,884007);
  assert(s.races.get('2018:ME').winner.parties.includes('OTHER')); assert(s.races.get('2018:ME').margin<0);
  const p=presidentialTables(parseCsv(fs.readFileSync('research/election-baseline-v1/sources/1976-2024-president.csv','utf8')));
  const rows=buildRows(s,p); assert(rows.some(r=>r.key==='2020:GA' && !r.winnerRepublican)); assert(rows.some(r=>r.key==='2018:ME' && !r.winnerRepublican));
  assert(rows.every(r=>r.clocks.presYear<r.year && r.clocks.priorSeatYear<r.year));
});
