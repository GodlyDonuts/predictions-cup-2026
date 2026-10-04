import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {collectFrozenPaperFiles} from './frozen-paper-graph.mjs';import {payloadHash} from '../execution/ledger.mjs';
import {nativePublicationBinding} from './native-publication.mjs';import {createBookPublicationState} from './book-publication-v2.mjs';
const workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url)));
export const collectorPublicationCodeIdentity=()=>collectFrozenPaperFiles(workspace,['live-paper/collector-publication-native.mjs','live-paper/book-publication-v3.mjs']);
export function collectorNativePublicationConfig(runtimeDir,universe){
 const file=path.join(runtimeDir,'collector-native-publication.json');if(!fs.existsSync(file))return null;
 const c=JSON.parse(fs.readFileSync(file));
 if(c.schemaVersion!==1||c.mode!=='native_publication_collector_v1'||c.scopeHash!==createBookPublicationState(universe).scopeHash||payloadHash(c.codeIdentity)!==payloadHash(collectorPublicationCodeIdentity()))throw Error('COLLECTOR_NATIVE_BINDING_CHANGED');
 nativePublicationBinding(runtimeDir,c.nativeBackend);return c;
}
