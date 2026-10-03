'use client';

const valid=v=>v!=null&&Number.isFinite(Number(v));
const money=v=>valid(v)?Number(v).toLocaleString('en-US',{minimumFractionDigits:2,maximumFractionDigits:2}):'—';
const count=v=>valid(v)?Number(v).toLocaleString('en-US'):'—';
const pnl=v=>valid(v)?`${Number(v)>0?'+':''}${money(v)}`:'—';
const tone=v=>valid(v)?Number(v)>0?'positive':Number(v)<0?'negative':'':'';
const short=v=>Number(v)>=1000?`${(Number(v)/1000).toFixed(1)}k`:money(v);
const marketTitle=x=>(x.label??x.instrument??'Unknown market').replace(/^Will the Democratic Party win the (.+)\?$/, '$1 · Democratic win');
const colors=['#7164e8','#3b9ecc','#e99b63','#db77a6','#72b4a0'];
const time=v=>v&&Number.isFinite(Date.parse(v))?new Date(v).toLocaleTimeString('en-US',{hour:'2-digit',minute:'2-digit'}):'Awaiting sync';

function Value({value}){return <span className={tone(value)}>{pnl(value)}</span>;}

export default function Overview({wallet:w,stats,execution,performance,performanceCost,performanceAt,healthy,connection,Icon}){
 const m=w.metrics??{},positions=w.positions??[],pending=w.pending??[],trades=w.trades??[];
 const invested=positions.filter(x=>valid(x.cost)&&Number(x.cost)>0).sort((a,b)=>Number(b.cost)-Number(a.cost));
 const total=invested.reduce((s,x)=>s+Number(x.cost),0);
 let offset=0;
 const arcs=invested.map((x,i)=>{const weight=Number(x.cost)/total*100;const arc={weight,offset,color:colors[i%colors.length]};offset+=weight;return arc;});
 const executionAge=Date.now()-Date.parse(execution?.updatedAt);
 const pilotFresh=Number.isFinite(executionAge)&&executionAge>=0&&executionAge<=60000;
 const primary=w.official?m.accountValue:m.cash;
 const latest=(w.official?trades:[...trades].reverse())[0];
 return <section className="overview-section" aria-label="Portfolio at a glance">
  <div className="summary-top">
   <article className="balance-card"><div className="balance-label"><span className="color-icon violet"><Icon name="wallet" size={18}/></span><span>{w.official?'Account value':'Available cash'}</span><span className="currency">SUSQies</span></div><strong className="balance-number">{money(primary)}</strong><div className="balance-meta"><span>{w.official?'Day P&L':'Realized P&L'}</span><b><Value value={w.official?m.periodPnl:m.realizedPnl}/></b>{!valid(w.official?m.periodPnl:m.realizedPnl)&&<small>Not available</small>}</div><div className="balance-foot"><Icon name="shield" size={13}/>{w.official?'Official tournament valuation':'Independent paper portfolio'}</div></article>
   <div className="quick-metrics">{stats.map(([label,value,note,icon,color],i)=><article className={`quick-metric metric-${i}`} key={label}><div><span className={`color-icon ${['rose','blue','peach','mint'][i]}`}><Icon name={icon} size={16}/></span><span>{label}</span></div><strong className={color}>{value}</strong><small>{note}</small></article>)}</div>
  </div>
  <div className="summary-bottom">
   <article className="summary-card holdings-card"><div className="summary-heading"><h2>Your holdings</h2><a href="#positions">View all <Icon name="chevron" size={12}/></a></div>{positions.length?<div className="holding-list">{positions.slice(0,3).map((x,i)=><div className="holding" key={x.instrument??i}><span className="holding-token" style={{background:['#eeebff','#e5f2fb','#fff1e3'][i],color:['#7361c7','#3889b2','#b97634'][i]}}>{(x.side??'P').toUpperCase().slice(0,1)}</span><div className="holding-name"><strong title={x.label??x.instrument}>{marketTitle(x)}</strong><small>{count(x.quantity??x.shares)} shares · {money(x.cost)} cost</small></div><b>{w.official?<Value value={x.unrealizedPnl}/>:money(x.liquidationValue)}</b></div>)}</div>:<div className="compact-empty"><Icon name="layers" size={24}/><p>{w.official&&!w.sourceTimes?.positions?'Holdings are awaiting sync.':'No open positions.'}</p></div>}<div className="card-caption">{w.official?'Unrealized P&L by position':'Liquidation value by position'}{positions.length>3?` · ${positions.length-3} more below`:''}</div></article>
   <article className="summary-card allocation-card"><div className="summary-heading"><h2>Allocation</h2><span>Cost basis</span></div><div className="allocation-body"><div className="donut"><svg viewBox="0 0 90 90" role="img" aria-label={total>0?`${money(total)} SUSQies invested across ${invested.length} positions`:'No capital allocated'}><circle cx="45" cy="45" r="34" fill="none" stroke="#edf0f5" strokeWidth="9"/>{arcs.map((arc,i)=><circle key={i} cx="45" cy="45" r="34" fill="none" stroke={arc.color} strokeWidth="9" pathLength="100" strokeDasharray={`${arc.weight} ${100-arc.weight}`} strokeDashoffset={-arc.offset} transform="rotate(-90 45 45)"/>)}</svg><div><strong>{short(total)}</strong><span>invested</span></div></div><div className="allocation-legend">{invested.length?invested.slice(0,3).map((x,i)=><div key={i}><i style={{background:colors[i%colors.length]}}/><span title={x.label??x.instrument}>{marketTitle(x)}</span><b>{(Number(x.cost)/total*100).toFixed(0)}%</b></div>):<p>Capital allocation appears when you hold a position.</p>}{invested.length>3&&<small>+ {invested.length-3} more positions</small>}</div></div><div className="card-caption">{money(total)} SUSQies deployed</div></article>
   <article className="summary-card execution-card"><div className="summary-heading"><h2>{w.official?pilotFresh?'Codex pilot':'Codex pilot history':'Strategy performance'}</h2><span className={`small-status ${w.official?execution?.status==='running'?'ok':'':w.status==='running'?'ok':''}`}><i/>{(w.official?pilotFresh?execution?.status:'historical':w.status)?.replaceAll('_',' ')??'Unavailable'}</span></div><dl className="performance-list">{w.official?<><div><dt>Realized P&L</dt><dd><Value value={execution?.realizedPnl}/></dd></div><div><dt>Open P&L · bid depth</dt><dd><Value value={performance.observedLiquidationUnrealizedPnl}/></dd></div><div><dt>Open P&L · midpoint</dt><dd><Value value={performance.midpointUnrealizedPnl}/></dd></div><div><dt>{performance.available?'Bot cost basis':'Last-known bot cost'}</dt><dd>{money(performanceCost)}</dd></div></>:<><div><dt>Realized P&L</dt><dd><Value value={m.realizedPnl}/></dd></div><div><dt>Reference P&L</dt><dd><Value value={m.referencePnl}/></dd></div><div><dt>Liquidation P&L</dt><dd><Value value={m.liquidationPnl}/></dd></div><div><dt>Closed round trips</dt><dd>{count(w.performance?.closedRoundTrips)}</dd></div></>}</dl>{w.official&&<div className="execution-totals"><span>{count(execution?.ordersSubmitted)} confirmed orders</span><span>{count(execution?.attributedFills)} bot fills</span></div>}<div className="card-caption">{w.official?!pilotFresh?`Last pilot update ${time(execution?.updatedAt)}`:performance.available?`Measured ${time(performanceAt)} · Zero added fees`:'Awaiting fresh execution evidence':'Independent capital · Zero added fees'}</div></article>
  </div>
  <div className="overview-ribbon"><div><span className={`dot ${healthy?'good':''}`}/><b>{connection}</b></div><span><Icon name="orders" size={13}/>{w.official&&!w.sourceTimes?.orders?'Orders awaiting sync':`${pending.length} ${w.official?'open':'pending'} orders`}</span><span><Icon name="activity" size={13}/>{latest?`Latest fill ${time(latest.at)}`:'No fills received'}</span><span><Icon name="clock" size={13}/>{w.official?`Valuation ${time(w.sourceTimes?.pnl)}`:`Worker ${time(w.updatedAt)}`}</span>{w.official&&<span className="budget-note">Order cap {money(execution?.limits?.maxOrderCost)} · Entry budget {money(execution?.limits?.maxCumulativeBuyCost)}</span>}</div>
 </section>;
}

