import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';

// Preserve the transitive source graph, including newly added dependencies.
// An older cohort's manifest is not a complete description of today's imports.
export function collectFrozenPaperFiles(workspaceDir,entries){
 const root=path.resolve(workspaceDir),files={},pending=[...entries];
 const relative=p=>path.relative(root,p).split(path.sep).join('/');
 while(pending.length){
  const name=pending.pop(),file=path.resolve(root,name);
  if(!file.startsWith(root+path.sep)||!file.endsWith('.mjs'))throw Error('FROZEN_GRAPH_SCOPE');
  const key=relative(file);if(files[key])continue;
  const bytes=fs.readFileSync(file),source=bytes.toString();files[key]=createHash('sha256').update(bytes).digest('hex');
  const patterns=[/(?:\bfrom\s*|\bimport\s*(?:\(\s*)?)['"](\.[^'"]+\.mjs)['"]/g,/\bnew\s+URL\(\s*['"](\.[^'"]+\.mjs)['"]\s*,\s*import\.meta\.url/g];
  for(const pattern of patterns)for(const match of source.matchAll(pattern))pending.push(relative(path.resolve(path.dirname(file),match[1])));
 }
 return Object.fromEntries(Object.entries(files).sort(([a],[b])=>a.localeCompare(b)));
}
