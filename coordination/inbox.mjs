import { readFile, writeFile, mkdir, rename, open, unlink } from 'node:fs/promises';
import { join, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

const REPO = 'GodlyDonuts/predictions-cup-2026';
export const digest = c => createHash('sha256').update(JSON.stringify([c.id,c.updated_at,c.body])).digest('hex');
export function merge(state, comments) {
  for (const c of comments) {
    if (state.acked[c.id] === digest(c)) continue;
    state.pending[c.id] = {id:c.id, updated_at:c.updated_at, body:c.body, url:c.html_url, issue:Number(c.issue_url.split('/').at(-1)), version:digest(c)};
  }
  return state;
}
export function preview(state, limit = 8) {
  const all = Object.values(state.pending).sort((a,b)=>a.updated_at.localeCompare(b.updated_at)||a.id-b.id);
  return {status:all.length?'UNREAD':'NO_NEW_MESSAGES', unread:all.length, shown:Math.min(all.length,limit),
    messages:all.slice(0,limit).map(c=>({...c,body:c.body.slice(0,800),truncated:c.body.length>800})),
    instruction:all.length?'Use show <id> for full text; ack <id> only after handling. Unshown messages remain pending.':undefined};
}
export function acknowledge(state, ids) {
  for (const id of ids) {
    if (!state.pending[id]) throw new Error('Unknown pending comment ID: '+id);
  }
  for (const id of ids) {state.acked[id]=state.pending[id].version; delete state.pending[id];}
  return state;
}
export async function collect(fetcher, since) {
  let url='https://api.github.com/repos/'+REPO+'/issues/comments?sort=updated&direction=asc&per_page=100&since='+encodeURIComponent(since);
  const comments=[];
  while(url) {
    const r=await fetcher(url);
    if(!r.ok) throw new Error('GitHub request failed (HTTP '+r.status+'); cursor unchanged.');
    const rows=await r.json();
    if(!Array.isArray(rows)) throw new Error('Invalid GitHub response; cursor unchanged.');
    comments.push(...rows);
    const next=(r.headers.get('link')||'').split(',').find(x=>x.includes('rel="next"'));
    url=next?.match(/<([^>]+)>/)?.[1]||null;
    if(url && !url.startsWith('https://api.github.com/repos/'+REPO+'/issues/comments?')) throw new Error('Unexpected pagination URL.');
  }
  return comments;
}
async function main() {
  const args=process.argv.slice(2);
  const option=name=>{const i=args.indexOf(name);return i<0?null:args[i+1];};
  const session=option('--session');
  if(!session || !/^[a-zA-Z0-9_-]{1,80}$/.test(session)) throw new Error('Supply --session Rhaegal-task (letters, digits, underscore, hyphen).');
  const command=args[0];
  const root=process.env.AGENT_INBOX_DIR || join(tmpdir(),'sig-agent-inbox');
  await mkdir(root,{recursive:true});
  const file=join(root,session+'.json'), lock=file+'.lock';
  let handle;
  try {handle=await open(lock,'wx');} catch {throw new Error('Inbox session is already locked. Do not share one session across agents.');}
  try {
    let state;
    try {state=JSON.parse(await readFile(file,'utf8'));} catch(e) {
      if(e.code!=='ENOENT') throw e;
      if(command!=='init') throw new Error('Initialize explicitly: init --session NAME --since ISO_TIMESTAMP. First read the compact board body and relevant ownership records.');
    }
    if(command==='init') {
      if(state) throw new Error('Session already exists; use poll.');
      const since=option('--since');
      if(!since || !Number.isFinite(Date.parse(since))) throw new Error('init requires a valid --since ISO timestamp. Earlier history is not marked read.');
      state={schemaVersion:1,repo:REPO,since:new Date(since).toISOString(),pending:{},acked:{}};
    } else if(command==='poll') {
      const started=new Date().toISOString();
      let token=process.env.GH_TOKEN || process.env.GITHUB_TOKEN;
      if(!token) {
        try {token=execFileSync(process.env.GH_PATH||'gh',['auth','token'],{encoding:'utf8',stdio:['ignore','pipe','pipe']}).trim();}
        catch {throw new Error('GitHub CLI authentication unavailable. Set GH_PATH to an installed gh executable, or use an existing GH_TOKEN. No credentials were created.');}
      }
      const changes=await collect(url=>fetch(url,{headers:{Authorization:'Bearer '+token,Accept:'application/vnd.github+json','X-GitHub-Api-Version':'2022-11-28'},signal:AbortSignal.timeout(30000)}),state.since);
      merge(state,changes);
      // Overlap the request boundary; hashes deduplicate unchanged messages.
      state.since=new Date(Date.parse(started)-2000).toISOString();
    } else if(command==='show') {
      const id=args[1],c=state.pending[id];
      if(!c) throw new Error('Unknown pending comment ID.');
      console.log(JSON.stringify(c,null,2));return;
    } else if(command==='ack') {
      const ids=args.slice(1,args.indexOf('--session'));
      if(!ids.length || ids.some(x=>!/^\d+$/.test(x))) throw new Error('Use ack COMMENT_ID [COMMENT_ID...] --session NAME.');
      acknowledge(state,ids);
    } else if(command!=='status') throw new Error('Commands: init, poll, status, show, ack.');
    const temporary=file+'.tmp';
    await writeFile(temporary,JSON.stringify(state),{mode:0o600});
    await rename(temporary,file);
    console.log(JSON.stringify(preview(state),null,2));
  } finally {await handle.close();await unlink(lock);}
}
if(process.argv[1] && import.meta.url===pathToFileURL(process.argv[1]).href) {
  main().catch(e=>{console.error(e.message);process.exitCode=1;});
}
