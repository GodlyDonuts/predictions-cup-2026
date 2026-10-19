import random
import unittest
from unittest.mock import patch
import torch
from .policy import ApiPolicy
from .population import freeze_probe,behavior,distance,mutate,island_replacements,retain_island_champions,ResearchArchive
from .stream import tensors
from .test_actions import environment
from moloch.learning_v2.train_inner import fingerprint


def probe():
    env=environment();frames=[]
    for i in range(4):
        frames.append({**tensors(env.observe(),'cpu'),'reset':torch.tensor([i==0]),'at_ms':env.now})
        env.advance(env.now+100)
    return freeze_probe(frames,role='train',dataset_sha256='a'*64,bounds=(1,100000))


class RawPopulationTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(92)

    def test_probe_pins_own_history_and_refuses_selection_or_mutation(self):
        p=probe();model=ApiPolicy(16)
        self.assertEqual(distance(behavior(model,p),behavior(model,p))['family_target_kl'],0.)
        with self.assertRaisesRegex(ValueError,'TRAINING_ONLY'):
            freeze_probe(p.frames,role='evolution',dataset_sha256='a'*64,bounds=p.bounds)
        p.frames[0]['global_state'][0,0]+=1
        with self.assertRaisesRegex(ValueError,'PROBE_CHANGED'):behavior(model,p)

    def test_component_equals_chain_rule_and_ignores_disabled_families(self):
        p=probe();model=ApiPolicy(16);a=behavior(model,p)
        with torch.no_grad():model.heads['family'].bias[0]+=.7
        b=behavior(model,p);measured=distance(a,b)['family_target_kl'];expected=[]
        for (x,_),(y,__)in zip(a,b):
            px=x.sum(-1);py=y.sum(-1)
            family=(px*(px.clamp_min(1e-300).log()-py.clamp_min(1e-300).log())).sum()
            tx=x/px.clamp_min(1e-300).unsqueeze(-1);ty=y/py.clamp_min(1e-300).unsqueeze(-1)
            target=(px*(tx*(tx.clamp_min(1e-300).log()-ty.clamp_min(1e-300).log())).sum(-1)).sum()
            expected.append(float(family+target))
        self.assertAlmostEqual(measured,sum(expected)/len(expected),places=10)
        self.assertGreater(measured,0.)

    def test_mutations_reset_no_optimizer_and_preserve_critic_for_noise(self):
        p=probe();parent=ApiPolicy(16);state={k:v.clone()for k,v in parent.state_dict().items()}
        for operator in ('local','medium','large','head_reset','fresh'):
            model=ApiPolicy(16);model.load_state_dict(state);before=fingerprint(model)
            rng=torch.random.get_rng_state().clone();report=mutate(model,p,operator,71)
            self.assertTrue(torch.equal(rng,torch.random.get_rng_state()))
            self.assertNotEqual(before,fingerprint(model));self.assertTrue(report['optimizer_moments_must_be_reset'])
            self.assertTrue(report['training_only_probe']);self.assertFalse(report['full_joint_action_kl_measured'])
            self.assertGreaterEqual(report['family_target_kl'],0.)
            if operator in ('local','medium','large'):
                self.assertTrue(torch.equal(state['value.weight'],model.value.weight))
                self.assertIn('target_band_reached',report)
            if operator=='local':self.assertTrue(torch.equal(state['recurrent.weight_hh_l0'],model.recurrent.weight_hh_l0))
            second=ApiPolicy(16);second.load_state_dict(state);mutate(second,p,operator,71)
            self.assertEqual(fingerprint(model),fingerprint(second))

    def test_island_champions_survive_and_only_planned_migration_crosses(self):
        ranking=[{'member':m,'selection':{'reward':float(m)}}for m in range(8)]
        rows=island_replacements(ranking,members=8,islands=2,cycle=1,rng=random.Random(1))
        self.assertEqual({r['loser']for r in rows},{0,1})
        self.assertEqual({r['donor']for r in rows},{6,7})
        self.assertFalse(any(r['migration']for r in rows))
        migrated=island_replacements(ranking,members=8,islands=2,cycle=4,rng=random.Random(1))
        self.assertTrue(all(r['migration']for r in migrated))
        with self.assertRaisesRegex(ValueError,'TWO_MEMBERS'):
            island_replacements(ranking,members=8,islands=8,cycle=1,rng=random.Random(1))

    def test_research_stepping_stones_cannot_become_fake_profit_or_test_authority(self):
        archive=ResearchArchive('a'*64,'b'*64)
        cases=[{'reward':-.1,'stress':s,'start_ms':10,'end_ms':20,'family_counts':{1:3,10:2}}for s in (False,True)]
        row={'member':0,'cycle':0,'checkpoint':{'path':'fixture','sha256':'c'*64},
             'selection':{'role':'development_selection','checkpoint_sha256':'c'*64,'reward':-.1,'cases':cases}}
        self.assertTrue(archive.consider(row));entry=archive.json()['entries'][0]
        self.assertFalse(entry['positive_repeated_development_evidence']);self.assertFalse(entry['production_eligible'])
        self.assertFalse(archive.consider(row))
        for c in cases:c['reward']=.01
        row['selection']['reward']=.01
        self.assertTrue(archive.consider(row));self.assertFalse(archive.json()['entries'][0]['positive_repeated_development_evidence'])
        row['selection']['cases']=cases+[{**c,'start_ms':20,'end_ms':30}for c in cases]
        row['selection']['reward']=.02
        for c in row['selection']['cases']:c['reward']=.02
        self.assertTrue(archive.consider(row));self.assertTrue(archive.json()['entries'][0]['positive_repeated_development_evidence'])
        self.assertFalse(archive.json()['entries'][0]['independent_test_qualification'])
        row['selection']['role']='test'
        with self.assertRaisesRegex(ValueError,'NOT_INDEPENDENT_TEST'):archive.consider(row)

    def test_all_negative_generations_keep_least_loss_and_restore_regressed_champions(self):
        from copy import deepcopy
        def rows(cycle,scores):
            return [{'member':m,'cycle':cycle,'checkpoint':{'path':f'{cycle}-{m}','sha256':str(m)*64},
                'selection':{'role':'development_selection','checkpoint_sha256':str(m)*64,
                    'reward':score,'cases':[{'reward':score,'start_ms':10,'end_ms':20,'stress':False}]}}
                for m,score in enumerate(scores)]
        first=rows(0,[-.4,-.3,-.1,-.2])
        selected,champions,events=retain_island_champions(first,{},members=4,islands=2)
        self.assertFalse(events);self.assertEqual([champions[i]['member']for i in range(2)],[2,3])
        second=rows(1,[-.8,-.9,-.7,-.6]);saved=deepcopy(second)
        selected,next_champions,events=retain_island_champions(second,champions,members=4,islands=2)
        self.assertEqual(second,saved);self.assertEqual(len(events),2)
        self.assertEqual([next_champions[i]['selection']['reward']for i in range(2)],[-.1,-.2])
        replacements=island_replacements(selected,members=4,islands=2,cycle=1,rng=random.Random(1))
        self.assertEqual({r['donor']for r in replacements},{0,1})
        self.assertEqual({r['checkpoint']['path']for r in selected if r['member']in(0,1)},{'0-2','0-3'})
        third=rows(2,[-.05,-.06,-.7,-.8])
        _,best,events=retain_island_champions(third,next_champions,members=4,islands=2)
        self.assertFalse(events);self.assertEqual([best[i]['selection']['reward']for i in range(2)],[-.05,-.06])
        third[0]['selection']['cases'][0]['start_ms']=11
        with self.assertRaisesRegex(ValueError,'CONTEXT_CHANGED'):
            retain_island_champions(third,best,members=4,islands=2)
        third=rows(2,[-.05,-.06,-.7,-.8]);third[0]['selection']['role']='test'
        with self.assertRaisesRegex(ValueError,'NOT_INDEPENDENT_TEST'):
            retain_island_champions(third,best,members=4,islands=2)

    def test_full_archive_admits_better_negative_unseen_behavior(self):
        archive=ResearchArchive('a'*64,'b'*64,maximum=1)
        def row(score,family):
            return {'member':0,'cycle':0,'checkpoint':{'path':'fixture','sha256':'c'*64},
                'selection':{'role':'development_selection','checkpoint_sha256':'c'*64,'reward':score,
                    'cases':[{'reward':score,'stress':False,'start_ms':10,'end_ms':20,'family_counts':{family:3}}]}}
        self.assertTrue(archive.consider(row(-.3,1)))
        self.assertTrue(archive.consider(row(-.1,2)))
        self.assertEqual(len(archive.entries),1)
        self.assertEqual(next(iter(archive.entries.values()))['score'],-.1)
        self.assertFalse(archive.consider(row(-.2,3)))
        self.assertFalse(next(iter(archive.entries.values()))['positive_repeated_development_evidence'])

    def test_failed_behavior_measurement_restores_original_parameters(self):
        model=ApiPolicy(16);p=probe();before=fingerprint(model);parent=behavior(model,p)
        with patch('moloch.learning_actions.population.behavior',side_effect=[parent,ValueError('probe failed')]):
            with self.assertRaisesRegex(ValueError,'probe failed'):mutate(model,p,'large',81)
        self.assertEqual(before,fingerprint(model))
