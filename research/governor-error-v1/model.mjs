import assert from 'node:assert/strict';import {identity} from '../election-baseline-v1/model.mjs';
const name=s=>identity(s).split(' ').filter(t=>t.length>1).join(' '),day=86400000,date=s=>/^\d{4}-\d{2}-\d{2}$/.test(s)?Date.parse(s+'T12:00:00Z'):NaN;
export function governorPollObservations(polls,outcomes){
 const grouped=new Map(),races=new Map(),excluded={},seen=new Map(),rows=[];const reject=r=>excluded[r]=(excluded[r]||0)+1;
 for(const r of outcomes){if(r.office_name!=='Governor'||r.stage!=='general'||r.special!=='false'||r.ranked_choice_round||Number(r.cycle)>2022)continue;const k=r.cycle+':'+r.state_abbrev;if(!grouped.has(k))grouped.set(k,new Map());const groups=grouped.get(k);if(!groups.has(r.race_id))groups.set(r.race_id,[]);groups.get(r.race_id).push(r);}
 for(const [key,groups]of grouped){if(groups.size!==1)continue;const all=[...groups.values()][0];if(all.some(r=>!/^\d+$/.test(r.votes)))continue;const total=all.reduce((s,r)=>s+Number(r.votes),0);if(!Number.isSafeInteger(total)||total<=0)continue;
  const candidates=new Map();for(const r of all){if(!r.candidate_name||!r.candidate_id)continue;if(!candidates.has(r.candidate_id))candidates.set(r.candidate_id,{name:name(r.candidate_name),votes:0,parties:new Set(),winner:false});const c=candidates.get(r.candidate_id);if(c.name!==name(r.candidate_name))throw Error('CONFLICTING_GOVERNOR_CANDIDATE');c.votes+=Number(r.votes);c.parties.add(r.ballot_party);c.winner ||= r.winner==='true';}
  const values=[...candidates.values()],ds=values.filter(c=>c.parties.has('DEM')),rs=values.filter(c=>c.parties.has('REP')),winners=values.filter(c=>c.winner);if(ds.length!==1||rs.length!==1||winners.length!==1||![ds[0],rs[0]].includes(winners[0])||winners[0].votes!==Math.max(...values.map(c=>c.votes)))continue;
  races.set(key,{d:ds[0],r:rs[0],total,democraticWon:winners[0]===ds[0],sourceUrls:[...new Set(all.map(r=>r.source).filter(Boolean))]});
 }
 for(const p of polls){if(p.type_simple!=='Gov-G')continue;const year=Number(p.cycle),key=year+':'+p.location,race=races.get(key);if(!Number.isSafeInteger(year)||year>2022||!race){reject('regular_final_outcome_unavailable');continue;}
  if(p.cand1_party!=='DEM'||p.cand2_party!=='REP'||name(p.cand1_name)!==race.d.name||name(p.cand2_name)!==race.r.name){reject('final_candidate_identity_unmatched');continue;}if(p.partisan&&p.partisan!=='NA'){reject('partisan_or_internal');continue;}
  const fieldAt=date(p.polldate),electionAt=date(p.electiondate),horizon=(electionAt-fieldAt)/day;if(!Number.isFinite(horizon)||horizon<=0||!p.electiondate.startsWith(year+'-11-')||Math.abs(horizon-Number(p.time_to_election))>1){reject('election_or_field_clock_invalid');continue;}
  const d=Number(p.cand1_pct),r=Number(p.cand2_pct),ad=Number(p.cand1_actual),ar=Number(p.cand2_actual);if(![d,r,ad,ar].every(x=>Number.isFinite(x)&&x>0&&x<100)||d+r>100.001){reject('invalid_vote_shares');continue;}
  const vd=100*race.d.votes/race.total,vr=100*race.r.votes/race.total;if(Math.abs(ad-vd)>.25||Math.abs(ar-vr)>.25||((ad>ar)!==race.democraticWon)){reject('separate_outcome_archive_disagrees');continue;}
  if(![p.poll_id,p.question_id,p.pollster_rating_id].every(x=>/^\d+$/.test(x))){reject('source_identity_missing');continue;}
  const id=p.race_id+':'+p.question_id,row={id,key,year,state:p.location,pollId:p.poll_id,questionId:p.question_id,pollsterId:p.pollster_rating_id,pollster:p.pollster,fieldDate:p.polldate,fieldAt,electionDate:p.electiondate,electionAt,horizonDays:horizon,democratic:d,republican:r,remaining:100-d-r,pollMargin:d-r,actualMargin:vd-vr,democraticWon:race.democraticWon,population:'not_in_archive',sourceFinalMargin:ad-ar,finalSourceUrls:race.sourceUrls};
  if(seen.has(id)){assert.deepEqual(row,seen.get(id),'conflicting_source_question');reject('identical_duplicate');continue;}seen.set(id,row);rows.push(row);
 }
 return{rows:rows.sort((a,b)=>a.year-b.year||a.key.localeCompare(b.key)||a.id.localeCompare(b.id)),excluded,verifiedOutcomeRaces:races.size};
}
