const accounting=name=>/^(under|over)votes$/.test(String(name).toLowerCase().replace(/[\s_-]+/g,''));
export function validVoteDenominatorV1(senate,sourceRows){
 const races=new Map(senate.races),corrections=[];
 for(const [key,race]of senate.races){
  if(race.selectedStage!=='gen')continue;
  const groups=new Map();
  for(const row of sourceRows){
   if(Number(row.year)!==race.year||row.state_po!==race.state||row.office!=='US SENATE'||String(row.stage).toLowerCase()!=='gen'||String(row.special).toLowerCase()==='true'||String(row.unofficial).toLowerCase()==='true'||!accounting(row.candidate))continue;
   const votes=Number(row.candidatevotes),label=String(row.candidate).toLowerCase().replace(/[\s_-]+/g,''),mode=String(row.mode||'total').toLowerCase();
   if(!Number.isSafeInteger(votes)||votes<0)throw Error('VALID_VOTE_COUNT');const modes=groups.get(label)??new Map();if(modes.has(mode))throw Error('VALID_VOTE_DUPLICATE_MODE');modes.set(mode,votes);groups.set(label,modes);
  }
  const removed=[...groups.values()].reduce((n,m)=>n+(m.has('total')?m.get('total'):[...m.values()].reduce((a,b)=>a+b,0)),0);
  if(!removed)continue;const total=race.total-removed;
  if(!Number.isSafeInteger(total)||total<race.republican.votes+race.opponent.votes)throw Error('VALID_VOTE_TOTAL');
  races.set(key,{...race,total,reportedBallotTotal:race.total,noncandidateVotes:removed,entrants:race.entrants.filter(x=>!accounting(x))});
  corrections.push({key,reportedBallots:race.total,noncandidateVotes:removed,validVotes:total,writeInVotesRetainedInDenominator:true});
 }
 return{...senate,races,validVoteCorrections:corrections,sourceRowsModified:false};
}
