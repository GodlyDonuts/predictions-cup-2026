import fs from 'node:fs';import assert from 'node:assert/strict';
const file='data/native-request-clock-milestone-20261004.json',archive='data/native-request-clock-milestone-20261004-v1.json';assert(!fs.existsSync(archive));fs.writeFileSync(archive,fs.readFileSync(file),{flag:'wx'});console.log('Preserved original request-clock evidence before lossless source-clock encoding correction');
