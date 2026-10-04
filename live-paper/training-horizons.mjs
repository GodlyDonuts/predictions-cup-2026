export const DEFAULT_HORIZONS=Object.freeze([60,180,300,600]);
export function trainingHorizons(value=DEFAULT_HORIZONS){
 if(!Array.isArray(value)||value.length<1||value.length>8||new Set(value).size!==value.length||value.some(x=>!Number.isSafeInteger(x)||x<60||x>86400))throw Error('Invalid training horizons');
 return Object.freeze([...value]);
}
export function parseTrainingHorizons(text){
 if(typeof text!=='string'||!/^\d+(,\d+)*$/.test(text))throw Error('Invalid training horizons');
 return trainingHorizons(text.split(',').map(Number));
}
