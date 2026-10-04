import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createBookPublicationState,applyBookPublication} from '../live-paper/book-publication-v2.mjs';
import {nativeExecutable} from '../live-paper/rust-backend.mjs';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';
import {archiveCheckedNativeGraph} from './archive-checked-native-graph.mjs';

const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),stamp=process.argv[2];assert(/^\d+$/.test(stamp));
const dir=path.join(runtime,'research-cohorts','settlement-value-diagnostic-20261004',stamp),read=p=>JSON.parse(fs.readFileSync(p));
const evidence=read('data/settlement-value-snapshot-'+stamp+'.json'),native=read(path.join(dir,'publication.json')),scope=read(path.join(dir,'scope.json'));
const bytes=Buffer.alloc(evidence.source.publicBookPrefixBytes),fd=fs.openSync(path.join(runtime,'public-books.jsonl'),'r');
try{assert.equal(fs.readSync(fd,bytes,0,bytes.length,0),bytes.length);}finally{fs.closeSync(fd);}
assert.equal(sha(bytes),evidence.source.publicBookPrefixHash);assert.equal(native.sourceHash,sha(bytes));
const start=performance.now();let state=createBookPublicationState(scope);
for(const line of bytes.toString().trimEnd().split('\n'))state=applyBookPublication(state,JSON.parse(line));
assert.deepEqual(native.state,state);assert.equal(native.engineHash,evidence.native.engineHash);assert.equal(sha(fs.readFileSync(nativeExecutable)),evidence.native.binaryHash);
const binding=archiveCheckedNativeGraph(runtime,{binaryHash:evidence.native.binaryHash,engineHash:native.engineHash,exactStateParity:true});
const result={schemaVersion:1,at:new Date().toISOString(),claimComment:5983667980,status:'full_actual_publication_state_parity_and_native_archive_verified',sourceBytes:bytes.length,sourceHash:sha(bytes),sourceSequence:state.sequence,sourceTailHash:state.hash,fullStateExact:true,independentJsReplayMs:performance.now()-start,nativeBackend:binding,activeBindingChanged:false,networkRequests:0,competitionOrders:0};
fs.writeFileSync('data/settlement-native-preservation-20261004.json',JSON.stringify(result)+'\n',{flag:'wx'});
console.log(JSON.stringify(result));
