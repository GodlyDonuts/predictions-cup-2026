import test from 'node:test';import assert from 'node:assert/strict';import {closeRaceCalibrationV1 as score} from './close-race-calibration-v1.mjs';
const rows=[{key:'2020:A',year:2020,trainedThroughYear:2018,pollMargin:1,probability:.6,outcome:0},{key:'2020:B',year:2020,trainedThroughYear:2018,pollMargin:20,probability:.99,outcome:1}];
test('safe-seat success does not hide the close-race score',()=>{const r=score(rows);assert.equal(r.closeMarginFivePoints.races,1);assert.equal(r.closeMarginFivePoints.brier,.36);assert(r.all.brier<r.closeMarginFivePoints.brier);assert.deepEqual(r.all.cycles,[2020]);});
test('future-trained and duplicated races cannot multiply calibration evidence',()=>{assert.throws(()=>score([...rows,rows[0]]));assert.throws(()=>score([{...rows[0],trainedThroughYear:2020}]));});
test('empty strata expose missing evidence',()=>{assert.equal(score([rows[1]]).closeMarginFivePoints.brier,null);});
