import WebSocket from 'ws';
import {boundedLookup} from './dns-lookup.mjs';

// Supported Supabase transport hook. Record only protocol status and typed
// errors; never copy request URLs, headers, response bodies or close reasons.
export function diagnosticTransport(record,classify) {
 return class DiagnosticWebSocket extends WebSocket {
  constructor(address,protocols) {
   super(address,protocols,{followRedirects:false,lookup:boundedLookup(record)});
   this.on('upgrade',()=>record({handshakeStatus:101}));
   this.on('unexpected-response',(request,response)=>{
    const status=response.statusCode;
    if(Number.isInteger(status)&&status>=100&&status<=599)record({handshakeStatus:status});
    response.destroy();request.destroy();this.terminate();
   });
   this.on('error',error=>record({socketError:classify(error)}));
   this.on('close',code=>{
    if(Number.isInteger(code)&&code>=1000&&code<=4999)record({socketCloseCode:code});
   });
  }
 };
}
