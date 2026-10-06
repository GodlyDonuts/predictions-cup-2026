import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
// Bank declarations remain immutable. Snapshot manifests include the complete
// relative dependency closure even when original declarations missed compact
// ESM syntax. Dynamic entry points must be supplied explicitly by the caller.
export function relativeMjsClosure({workspace,entryFiles}){
 const base=path.resolve(workspace),files={};
 function visit(file){const resolved=path.resolve(base,file);if(!resolved.startsWith(base+path.sep)||!resolved.endsWith('.mjs'))throw Error('FROZEN_CLOSURE_SCOPE');const name=path.relative(base,resolved).replaceAll(path.sep,'/');if(files[name])return;const bytes=fs.readFileSync(resolved);files[name]=createHash('sha256').update(bytes).digest('hex');
  const text=bytes.toString(),patterns=[/\b(?:import|export)\s*(?:[^;]*?\bfrom\s*)?['"](\.[^'"]+\.mjs)['"]/g,/\bimport\s*\(\s*['"](\.[^'"]+\.mjs)['"]\s*\)/g,/\bimport\s*\(\s*new\s+URL\s*\(\s*['"](\.[^'"]+\.mjs)['"]/g];
  for(const re of patterns)for(const m of text.matchAll(re))visit(path.resolve(path.dirname(resolved),m[1]));
 }
 for(const file of entryFiles)visit(file);return files;
}
