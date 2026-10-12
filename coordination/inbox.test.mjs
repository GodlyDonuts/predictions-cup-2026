import test from 'node:test';
import assert from 'node:assert/strict';
import {merge,preview,acknowledge,collect} from './inbox.mjs';
const state=()=>({pending:{},acked:{}});
const comment=(id,body='hello')=>({id,body,updated_at:'2026-10-12T06:00:00Z',html_url:'https://github.com/x#'+id,issue_url:'https://api.github.com/repos/x/issues/1'});
test('bounded output preserves every unread message',()=>{
 const s=state();merge(s,Array.from({length:20},(_,i)=>comment(i+1,'x'.repeat(2000))));
 const p=preview(s);assert.equal(p.unread,20);assert.equal(p.messages.length,8);assert.equal(p.messages[0].body.length,800);
 assert.equal(Object.keys(s.pending).length,20);
});
test('explicit ack deduplicates repeats and resurfaces edited comments',()=>{
 const s=state();merge(s,[comment(1),comment(2)]);acknowledge(s,['1']);merge(s,[comment(1),comment(2)]);
 assert.deepEqual(Object.keys(s.pending),['2']);merge(s,[comment(1,'edited')]);assert.equal(s.pending[1].body,'edited');
 assert.throws(()=>acknowledge(s,['2','99']));assert.ok(s.pending[2]);
});
test('all pagination completes; failed pages throw before caller advances cursor',async()=>{
 let calls=0;const rows=await collect(async()=>{calls++;return {ok:true,json:async()=>[comment(calls)],headers:{get:()=>calls===1?'<https://api.github.com/repos/GodlyDonuts/predictions-cup-2026/issues/comments?page=2>; rel="next"':null}};},'2026-10-12T00:00:00Z');
 assert.equal(rows.length,2);
 await assert.rejects(collect(async()=>({ok:false,status:429}),'2026-10-12T00:00:00Z'));
});
