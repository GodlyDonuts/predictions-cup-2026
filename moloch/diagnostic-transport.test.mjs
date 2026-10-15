import test from 'node:test';import assert from 'node:assert/strict';import http from 'node:http';import {once} from 'node:events';
import {diagnosticTransport} from './realtime/diagnostic-transport.mjs';
import {channelFailure} from './realtime/capture.mjs';
import {createRequire} from 'node:module';
const {WebSocketServer}=createRequire(new URL('./realtime/diagnostic-transport.mjs',import.meta.url))('ws');

test('actual rejected WebSocket handshake exposes status and excludes secret-bearing response',async()=>{
 const server=http.createServer((req,res)=>{res.writeHead(403,{'x-private':'PRIVATE_HEADER'});res.end('PRIVATE_BODY');});
 server.listen(0,'127.0.0.1');await once(server,'listening');const evidence=[];
 try{
  const Transport=diagnosticTransport(value=>evidence.push(value),error=>channelFailure('CHANNEL_ERROR',error));
  const socket=new Transport(`ws://127.0.0.1:${server.address().port}/PRIVATE_URL`);
  await new Promise(resolve=>socket.once('close',resolve));
  assert.ok(evidence.some(e=>e.handshakeStatus===403));assert.doesNotMatch(JSON.stringify(evidence),/PRIVATE/);
 }finally{await new Promise(resolve=>server.close(resolve));}
});

test('actual refused socket preserves typed network error without address or message',async()=>{
 const server=http.createServer();server.listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;await new Promise(resolve=>server.close(resolve));
 const evidence=[],Transport=diagnosticTransport(value=>evidence.push(value),error=>channelFailure('CHANNEL_ERROR',error));
 const socket=new Transport(`ws://127.0.0.1:${port}/PRIVATE_URL`);await new Promise(resolve=>socket.once('close',resolve));
 assert.ok(evidence.some(e=>e.socketError?.networkCodes?.includes('ECONNREFUSED')));assert.doesNotMatch(JSON.stringify(evidence),/PRIVATE|127\.0\.0\.1/);
});
test('successful upgrade retains message delivery and reports only numeric handshake status',async()=>{
 const server=new WebSocketServer({port:0,host:'127.0.0.1'});await once(server,'listening');server.on('connection',socket=>socket.send('PRIVATE_PAYLOAD'));
 const evidence=[],Transport=diagnosticTransport(value=>evidence.push(value),error=>channelFailure('CHANNEL_ERROR',error));
 const socket=new Transport(`ws://127.0.0.1:${server.address().port}`);
 try {
  const [data]=await once(socket,'message');assert.equal(data.toString(),'PRIVATE_PAYLOAD');assert.ok(evidence.some(e=>e.handshakeStatus===101));assert.doesNotMatch(JSON.stringify(evidence),/PRIVATE/);
 }finally{socket.terminate();await new Promise(resolve=>server.close(resolve));}
});
