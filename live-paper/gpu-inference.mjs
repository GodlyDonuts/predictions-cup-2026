export function predictMLP(m,x){
 if(!Number.isInteger(m.width)||m.width<1||m.width>32||!Number.isInteger(m.hidden)||m.hidden<1||m.hidden>64||x.length<m.width||m.means?.length!==m.width||m.scales?.length!==m.width||m.inputWeights?.length!==m.width||m.hiddenBias?.length!==m.hidden||m.outputWeights?.length!==m.hidden)throw Error('Invalid frozen GPU model dimensions');
 const values=[...m.means,...m.scales,...m.hiddenBias,...m.outputWeights,m.outputBias,m.targetMean,m.targetScale,...x.slice(0,m.width)];
 if(!values.every(Number.isFinite)||m.scales.some(v=>v<=0)||m.targetScale<=0||m.inputWeights.some(r=>r.length!==m.hidden||!r.every(Number.isFinite)))throw Error('Invalid frozen GPU model values');
 let value=m.outputBias;for(let h=0;h<m.hidden;h++){let z=m.hiddenBias[h];for(let j=0;j<m.width;j++)z+=(x[j]-m.means[j])/m.scales[j]*m.inputWeights[j][h];value+=Math.max(0,z)*m.outputWeights[h];}
 const out=m.targetMean+m.targetScale*value;if(!Number.isFinite(out))throw Error('Nonfinite GPU forecast');return out;
}
