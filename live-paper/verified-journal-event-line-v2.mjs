import {createHash} from 'node:crypto';
// The publisher appends hash last to JSON.stringify({sequence,previous,at,
// event,state}). Verify those exact body bytes; decode only the event projection.
// Unused state is authenticated as bytes, not interpreted as wallet state.
export function verifiedJournalEventLineV2(line){
 const tail=line.slice(-75),m=/,"hash":"([a-f0-9]{64})"}$/.exec(tail);if(!m)throw Error('EVENT_JOURNAL_TAIL');
 const end=line.length-m[0].length,expected=Buffer.from(m[1],'utf8').toString('utf8');if(createHash('sha256').update(line.slice(0,end)).update('}').digest('hex')!==expected)throw Error('EVENT_JOURNAL_HASH');
 const match=/^\{"sequence":([1-9]\d*),"previous":"([a-f0-9]{64}|)","at":"[^"]+","event":/.exec(line);if(!match)throw Error('EVENT_JOURNAL_HEADER');
 let depth=0,string=false,escaped=false,finish=-1;const start=match[0].length;
 if(line[start]!=='{'&&line[start]!=='[')throw Error('EVENT_JOURNAL_EVENT');
 for(let i=start;i<end;i++){const c=line[i];if(string){if(escaped)escaped=false;else if(c==='\\')escaped=true;else if(c==='"')string=false;continue;}if(c==='"')string=true;else if(c==='{'||c==='[')depth++;else if(c==='}'||c===']'){depth--;if(depth===0){finish=i+1;break;}}}
 if(finish<0||line.slice(finish,finish+9)!==',"state":')throw Error('EVENT_JOURNAL_LAYOUT');
 const row=JSON.parse(Buffer.from(line.slice(0,finish)+'}','utf8').toString('utf8'));if(!Number.isSafeInteger(row.sequence)||!Number.isFinite(Date.parse(row.at)))throw Error('EVENT_JOURNAL_METADATA');return{...row,hash:expected};
}

