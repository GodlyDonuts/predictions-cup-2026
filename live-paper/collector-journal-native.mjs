import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {collectFrozenPaperFiles} from './frozen-paper-graph.mjs';import {payloadHash} from '../execution/ledger.mjs';
import {nativeJournalBinding,journalEngineIdentity} from './native-journal-restore.mjs';
const workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url)));
export const collectorJournalCodeIdentity=()=>collectFrozenPaperFiles(workspace,['live-paper/collector-journal-native.mjs']);
export function collectorNativeJournalConfig(runtimeDir){
 const file=path.join(runtimeDir,'collector-native-journal.json');if(!fs.existsSync(file))return null;
 const c=JSON.parse(fs.readFileSync(file));
 if(c.schemaVersion!==1||c.mode!=='native_original_journal_collector_v1'||payloadHash(c.codeIdentity)!==payloadHash(collectorJournalCodeIdentity())||payloadHash(c.codeIdentity)!==c.sourceSnapshot)throw Error('COLLECTOR_JOURNAL_BINDING_CHANGED');
 nativeJournalBinding(runtimeDir,c.nativeBackend);
 if(journalEngineIdentity(path.join(runtimeDir,'journal-native-sources',c.nativeBackend.engineHash))!==c.nativeBackend.engineHash)throw Error('COLLECTOR_JOURNAL_SOURCE_CHANGED');
 return c;
}
