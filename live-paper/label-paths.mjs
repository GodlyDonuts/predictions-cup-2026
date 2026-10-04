// Round-trip labels consult only later rows for the same instrument. Indexing
// those exact rows avoids scanning every unrelated market for each decision.
export function instrumentLabelPaths(rows){
 const instruments=new Map(),paths=[];
 for(const row of rows){
  const stream=instruments.get(row.instrument)||[];
  paths.push({rows:stream,index:stream.length});stream.push(row);instruments.set(row.instrument,stream);
 }
 return paths;
}
