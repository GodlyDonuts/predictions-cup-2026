import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from datetime import datetime,timezone
import sys

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('sig_screen_under_test',HERE/'analyze.py');A=importlib.util.module_from_spec(spec);sys.modules[spec.name]=A;spec.loader.exec_module(A)
def iso(ms):return datetime.fromtimestamp(ms/1000,timezone.utc).isoformat(timespec='milliseconds').replace('+00:00','Z')
def book(ms,ex='2',market='1',seq=None,bid='0.450000',ask='0.455000',qty=100):
 return {'type':'book','receivedAt':iso(ms),'processedAt':iso(ms),'sourceRef':{'id':'fixture','sequence':ms+1,'hash':'a'*64},'book':{'tournamentId':'fixture-tournament','marketId':market,'exchangeId':ex,'sequence':str(ms+1 if seq is None else seq),'sourceAt':iso(ms),'requestStartedAt':iso(ms),'receivedAt':iso(ms),'bids':[{'price':bid,'quantity':qty}],'asks':[{'price':ask,'quantity':qty}]}}
def send(s,e):s.event(e,A.sha(A.compact(e)))
def warm(s):
 for t in [0,30000,55000,59000]:send(s,book(t))
def native_seal(v):return {**v,'hash':A.sha(A.compact(v))}
def native_bytes(v):return A.compact(native_seal(v))+b'\n'

class CausalTests(unittest.TestCase):
 def test_fixed_protocol_source(self):self.assertEqual(A.sha((HERE/'protocol.json').read_bytes().replace(b'\r\n',b'\n')),A.PROTOCOL_SHA)
 def test_missing_anchor_is_not_zero_or_same_block_rescue(self):
  s=A.Screen();send(s,book(60000));send(s,book(61000));self.assertEqual(s.counts['block_missing_lag_anchors'],1);self.assertEqual(s.counts['nominations'],0)
 def test_processed_availability_prevents_late_source_lookahead(self):
  s=A.Screen();warm(s);e=book(59000);e['processedAt']=iso(60000);e['book']['sequence']='60001';send(s,e)
  self.assertEqual(s.counts['block_missing_lag_anchors'],1);self.assertEqual(s.counts['nominations'],1)
  anchors=next(iter(s.pending.values()))['row']['lagAnchorHashes'];self.assertNotIn(A.sha(A.compact(e)),anchors)
 def test_exact_crossing_no_complement_and_fixed_margin(self):
  s=A.Screen();warm(s);send(s,book(60000));self.assertEqual(s.counts['nominations'],1);send(s,book(65000));self.assertEqual(s.counts['entry_marks'],1)
  send(s,book(120000));send(s,book(130000,bid='0.460000',ask='0.465000'))
  self.assertEqual(len(s.examples),1);r=s.examples[0];self.assertEqual(r['yesUnits'],0);self.assertEqual(r['noUnits'],-20000);self.assertEqual(r['entryMarkAvailableNs'],65000*A.NS);self.assertEqual(r['labelAvailableNs'],130000*A.NS)
 def test_gap_clears_anchors_and_censors_pending(self):
  s=A.Screen();warm(s);send(s,book(60000));send(s,{'type':'gap','exchangeId':'2','receivedAt':iso(61000),'processedAt':iso(61000)})
  self.assertEqual(len(s.pending),0);self.assertEqual(s.counts['censored_gap'],1);send(s,book(62000));self.assertEqual(len(s.examples),0)
 def test_missing_future_label_never_flat(self):
  s=A.Screen();warm(s);send(s,book(60000));s.advance(100000*A.NS);self.assertEqual(len(s.examples),0);self.assertEqual(s.counts['censored_entry_no_timely_book'],1)
 def test_stale_first_book_cannot_be_replaced_by_later_good_book(self):
  s=A.Screen();warm(s);send(s,book(60000));e=book(63000);e['processedAt']=iso(65000);send(s,e);self.assertEqual(s.counts['censored_entry_first_book_clock'],1);send(s,book(66000));self.assertEqual(s.counts['entry_marks'],0)
 def test_original_clock_future_rejects_or_marks_unavailable(self):
  s=A.Screen();e=book(1000);e['receivedAt']=iso(2000)
  with self.assertRaises(ValueError):send(s,e)
  s=A.Screen();e=book(1000);e['book']['sourceAt']=iso(1014);send(s,e);self.assertEqual(s.counts['unavailable_book_invalid_clock'],1)
 def test_unsigned_flow_requires_actual_coverage_and_dedups(self):
  s=A.Screen();warm(s);trade={'type':'trade','tournamentId':'fixture-tournament','marketId':'1','exchangeId':'2','id':'41','yesPrice':'0.450000','quantity':17,'executedAt':iso(59000),'receivedAt':iso(59500),'processedAt':iso(59500)};send(s,trade);send(s,trade)
  send(s,{'type':'coverage','exchangeId':'2','complete':True,'from':iso(30000),'to':iso(60000),'projectedThroughSequence':'99999','receivedAt':iso(60000),'processedAt':iso(60000)})
  send(s,book(60000));r=next(iter(s.pending.values()))['row'];self.assertEqual(r['unsignedVolume30s'],17);self.assertEqual(len(r['x']),6);self.assertEqual(s.counts['duplicate_trade'],1)
 def test_original_empty_complete_flow_is_observed_zero(self):
  s=A.Screen();warm(s);send(s,{'type':'coverage','exchangeId':'2','complete':True,'from':iso(30000),'to':iso(60000),'projectedThroughSequence':'99999','receivedAt':iso(60000),'processedAt':iso(60000)});send(s,book(60000));self.assertEqual(next(iter(s.pending.values()))['row']['unsignedVolume30s'],0)
 def test_unsigned_volume_missing_not_zero(self):
  s=A.Screen();warm(s);send(s,book(60000));self.assertIsNone(next(iter(s.pending.values()))['row']['unsignedVolume30s'])
 def test_changed_trade_duplicate_rejected(self):
  s=A.Screen();e={'type':'trade','tournamentId':'fixture-tournament','marketId':'1','exchangeId':'2','id':'41','yesPrice':'0.450000','quantity':17,'executedAt':iso(1000),'receivedAt':iso(1100),'processedAt':iso(1100)};send(s,e);e['quantity']=18
  with self.assertRaisesRegex(ValueError,'TRADE_CONFLICT'):send(s,e)
 def test_same_book_version_conflict_rejected(self):
  s=A.Screen();e=book(1000);send(s,e);e['book']['bids'][0]['quantity']=99
  with self.assertRaisesRegex(ValueError,'BOOK_VERSION_CONFLICT'):send(s,e)
 def test_known_own_depth_and_old_fill_duplicate(self):
  s=A.Screen();ack={'type':'actual_order_ack','orderId':1,'actualOrderId':'201','exchangeId':'2','side':'YES','action':'buy','price':'0.450000','quantity':10,'receivedAt':iso(0),'processedAt':iso(0)};send(s,ack)
  e={'type':'actual_fill','orderId':1,'actualOrderId':'201','fillId':'301','side':'YES','action':'buy','quantity':3,'price':'0.450000','fee':'0.000000','executedAt':iso(1000),'receivedAt':iso(2000),'processedAt':iso(2000)};send(s,e);e['receivedAt']=iso(40000);e['processedAt']=iso(40000);send(s,e);self.assertEqual(s.own[1]['quantity'],7)
  ack['receivedAt']=iso(40000);ack['processedAt']=iso(40000);send(s,ack);self.assertEqual(s.own[1]['quantity'],7)
  b=A.NATIVE.read_book(book(40000),40000*A.NS,10000*A.NS);self.assertEqual(s.adjusted(b).bids[0][1],93)
 def test_pacer_is_serial_and_failed_probe_does_not_spend(self):
  q=A.Pacer();self.assertEqual(q.packet(0,3000),4200);self.assertEqual(q.packet(0,3000),8400);before=(q.end,list(q.reads),list(q.writes));self.assertIsNone(q.packet(0,3000));self.assertEqual((q.end,list(q.reads),list(q.writes)),before)
  for at in range(10000,180000,1000):q.packet(at,3000);self.assertLessEqual(len(q.reads),100);self.assertLessEqual(len(q.writes),24)
 def test_heldout_groups_never_enter_training_and_same_cohort(self):
  held=next(str(i) for i in range(100) if A.group_holdout(str(i)));normal=next(str(i) for i in range(100) if not A.group_holdout(str(i)));rows=[]
  for i in range(240):
   group=held if i%5==0 else normal;t=i*100000;rows.append({'id':str(i),'market':group,'group':group,'groupHeldOut':A.group_holdout(group),'at':t,'end':t+73000,'available':t+70000,'x':[.001,.1,.005,0,0,0],'yes':-.01,'no':-.01,'yesUnits':-10000,'noUnits':-10000,'unchangedYes':-.01,'unchangedNo':-.01})
  out=A.evaluate(rows);self.assertEqual(len(out['folds']),3)
  for f in out['folds']:self.assertNotIn(held,f['trainingGroups'])
  self.assertEqual(len({m['sampleIdsSha256'] for m in out['models'].values()}),1);self.assertFalse(out['executionAuthority']);self.assertFalse(out['modelApproval'])
 def test_actual_label_available_after_boundary_is_not_training(self):
  rows=[]
  for i in range(80):
   t=i*100000;rows.append({'id':str(i),'market':'m','group':'m','groupHeldOut':False,'at':t,'end':t+1,'available':10**20,'x':[0]*6,'yes':0,'no':0,'yesUnits':0,'noUnits':0,'unchangedYes':-.01,'unchangedNo':-.01})
  self.assertTrue(all(f['trainingClusters']==0 for f in A.evaluate(rows)['folds']))
 def test_pinned_suffix_links_original_anchor_and_rejects_edit(self):
  with tempfile.TemporaryDirectory() as d:
   root=Path(d);bank=native_seal({'mode':'fixture'});anchor=native_seal({'manifestHash':bank['hash'],'sequence':10,'previous':'a'*64});e=book(1000);row=native_seal({'event':e,'previous':anchor['previous'],'sequence':11});report=native_seal({'manifestHash':bank['hash'],'sequence':11,'previous':row['hash'],'report':{'at':iso(1000)}})
   values={'bankManifest':bank,'anchorHead':anchor,'report':report};manifest={'schemaVersion':1,'sealed':True}
   for name,v in values.items():raw=A.compact(v)+b'\n';p=root/(name+'.json');p.write_bytes(raw);manifest[name]={'path':p.name,'sha256':A.sha(raw),'bytes':len(raw)}
   raw=A.compact(row)+b'\n';(root/'events.jsonl').write_bytes(raw);manifest['journal']={'path':'events.jsonl','sha256':A.sha(raw),'bytes':len(raw)};mp=root/'inputs.json';mp.write_bytes(A.compact(manifest));result=A.run(mp,A.sha(mp.read_bytes()));self.assertEqual(result['nativeSuffix']['rows'],1);self.assertFalse(result['semanticGroupsAvailable']);self.assertFalse(result['sourceBodyMembershipIndependentlyVerified'])
   (root/'events.jsonl').write_bytes(raw.replace(b'0.450000',b'0.440000'))
   with self.assertRaisesRegex(ValueError,'PIN_CHANGED'):A.run(mp,A.sha(mp.read_bytes()))

if __name__=='__main__':unittest.main()
