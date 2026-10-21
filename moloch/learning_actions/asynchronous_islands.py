"""Independent island progress under one parent neural-compute coordinator.

Each island retains the existing complete training/evaluation/replacement loop.
Source waits can overlap; PPO updates remain serialized and on-policy per model.
Only immutable, comparably evaluated champions cross island boundaries. There is
one optional global independent test after all requested research cycles.
"""
import argparse
import copy
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
import math
from pathlib import Path
import threading

from moloch.learning_realism.protocol import digest, independent_test_claim
from .experiment import Experiment
from .neural_sessions import NeuralCoordinator
from .process_worlds import WaitingDataUse
from .selection import frozen, IndependentTest
from .training import load_checkpoint


def configurations(base):
    members = base.get('members', 4)
    islands = (base.get('population_search') or {}).get('islands')
    if (type(members) is not int or type(islands) is not int or islands < 2
            or members % islands or members // islands < 2):
        raise ValueError('AT_LEAST_TWO_MEMBERS_PER_EQUAL_ISLAND')
    parents = base.get('development_initial_checkpoints') or (
        [base['development_initial_checkpoint']] if base.get('development_initial_checkpoint') else [])
    fresh = base.get('fresh_founders', 0)
    if type(fresh) is not int or not 0 <= fresh <= members:
        raise ValueError('VALID_GLOBAL_FRESH_FOUNDER_COUNT')
    if parents and len(parents) != members - fresh:
        raise ValueError('EXPLICIT_PARENT_PER_INHERITED_GLOBAL_SLOT_REQUIRED')
    order = base.get('first_cycle_member_order', list(range(members)))
    if type(order) is not list or any(type(i) is not int for i in order) or sorted(order) != list(range(members)):
        raise ValueError('EVERY_GLOBAL_MEMBER_ORDERED_ONCE')
    result = []
    for island in range(islands):
        ids = list(range(island, members, islands)); c = copy.deepcopy(base)
        c['members'] = len(ids); c['population_search']['islands'] = 1
        c['fresh_founders'] = sum(i >= members - fresh for i in ids)
        c.pop('development_initial_checkpoint', None)
        c['development_initial_checkpoints'] = [copy.deepcopy(parents[i]) for i in ids if parents and i < members-fresh]
        c['first_cycle_member_order'] = [ids.index(i) for i in order if i in ids]
        c['test_finalist_once'] = False
        c['asynchronous_global_member_ids'] = ids
        for key in ['inherited_wait_logit_shifts', 'founder_family_logit_shifts', 'founder_price_logit_shifts']:
            values = base.get(key)
            if values is not None:
                if type(values) is not list or len(values) != members:
                    raise ValueError('ONE_GLOBAL_FOUNDER_PROFILE_PER_MEMBER')
                c[key] = [copy.deepcopy(values[i]) for i in ids]
        for key, default in [('initial_learning_rates', [base.get('learning_rate', 1e-5)]),
                             ('initial_entropy_coefficients', [base.get('entropy_coefficient', .01)])]:
            values = base.get(key, default)
            if type(values) is not list or not values:
                raise ValueError('NONEMPTY_GLOBAL_FOUNDER_HYPERPARAMETERS')
            c[key] = [values[i % len(values)] for i in ids]
        result.append(c)
    return result


class ChampionBridge:
    def __init__(self, directory, base_sha, *, islands, migration_every=4, migrate=True):
        if type(islands) is not int or islands < 2 or type(migration_every) is not int or migration_every < 0:
            raise ValueError('EXPLICIT_ISLAND_RING_AND_MIGRATION_PERIOD')
        self.directory = Path(directory); self.base_sha = base_sha; self.migrate = migrate
        self.islands = islands; self.migration_every = migration_every
        self.lock = threading.RLock(); self.champions = {}; self.imported = {}; self.events = []

    def context(self, owner, selection):
        fields = ['start_ms', 'end_ms', 'stress', 'selection_seed', 'evaluation_compute_mode',
                  'classification', 'valuation', 'D']
        return digest(dict(baseConfigurationSha256=self.base_sha, datasetSha256=owner.dataset_sha,
            source=owner.frozen['source_sha256'], mechanics=owner.frozen['mechanics_sha256'],
            actorCostMs=owner.calibration['decision_latency_ms'],
            cases=[{k:case[k] for k in fields} for case in selection['cases']]))

    def publish(self, island, owner, cycle, ranking, best):
        if (len(ranking) != len(owner.population)
                or any(type(row['member']) is not int for row in ranking)
                or {row['member']for row in ranking} != set(range(len(owner.population)))):
            raise ValueError('COMPLETE_CURRENT_LOCAL_POPULATION_REQUIRED')
        selection = best['selection']; checkpoint = best['checkpoint']
        if (selection['role'] not in ('development_selection', 'evolution')
                or selection['checkpoint_sha256'] != checkpoint['sha256'] or not selection['cases']
                or not math.isfinite(selection['reward'])
                or not math.isclose(selection['reward'], min(x['reward'] for x in selection['cases']), rel_tol=1e-12, abs_tol=1e-12)):
            raise ValueError('EXACT_COMPLETED_CHAMPION_SELECTION_REQUIRED')
        context = self.context(owner, selection)
        record = dict(island=island, publishedCycle=cycle, contextSha256=context,
                      experimentSha256=owner.spec_sha, row=copy.deepcopy(best))
        with self.lock:
            previous = self.champions.get(island)
            if previous and previous['contextSha256'] != context:
                raise ValueError('ISLAND_EVALUATION_CONTEXT_CHANGED')
            if previous and previous['row']['selection']['reward'] > selection['reward']:
                record = previous
            frozen(self.directory/f'champion-{island:03d}-{cycle:06d}.json', record)
            self.champions[island] = record
            donor = self.champions.get((island-1) % self.islands)
            if donor is not None and donor['contextSha256'] != context: donor = None
            frozen(self.directory/f'published-{island:03d}-{cycle:06d}.json', dict(
                completedChampion=True, independentQualification=False, productionAuthority=False,
                currentScore=record['row']['selection']['reward'], comparableDonorAvailable=donor is not None))
        due = cycle > 0 and self.migration_every > 0 and cycle % self.migration_every == 0
        if not due or not self.migrate or donor is None or donor['row']['selection']['reward'] <= record['row']['selection']['reward']:
            return
        cp = donor['row']['checkpoint']; key = (island, cp['sha256'])
        if key in self.imported: return
        model, bundle = load_checkpoint(cp['path'], cp['sha256'], owner.device)
        if (bundle['split_specification_sha256'] != donor['experimentSha256']
                or bundle['dataset_manifest_sha256'] != owner.dataset_sha
                or bundle['hidden_size'] != owner.model_size
                or type(model) is not owner.policy_type()):
            raise ValueError('MATCHED_SOURCE_CHAMPION_WEIGHTS_REQUIRED')
        parameters = bundle['optimizer']['param_groups'][0]
        optimizer = owner.make_optimizer(model, rate=parameters['lr'],
            entropy=parameters.get('tartarus_entropy_coefficient', .01))
        slot = ranking[-1]['member']
        migrated = owner.checkpoint(f'cross-island-{cycle:06d}-{slot:03d}', model, optimizer)
        owner.population[slot] = migrated
        event = dict(event='cross_island_migration', island=island, cycle=cycle, slot=slot,
                     donorIsland=donor['island'], donorCheckpoint=cp, checkpoint=migrated,
                     contextSha256=context, optimizerMomentsReset=True, newFitnessRequired=True,
                     completedWorldsOrScoresRewritten=False, productionAuthority=False)
        with self.lock:
            self.imported[key] = migrated; self.events.append(event)
            frozen(self.directory/f'migration-{island:03d}-{cycle:06d}.json', event)
        owner.log(event)


class AsynchronousIslands:
    def __init__(self, config, output, calibration, *, workers=2, experiment_factory=None, migrate=True):
        variants = configurations(config)
        if type(workers) is not int or not 1 <= workers <= len(variants):
            raise ValueError('BOUNDED_ISLAND_COMPUTE_WORKERS')
        if type(calibration.get('decision_latency_ms')) is not int or calibration['decision_latency_ms'] <= 0:
            raise ValueError('ONE_NONZERO_SHARED_ACTOR_CALIBRATION')
        self.base = copy.deepcopy(config); self.output = Path(output)
        self.output.mkdir(parents=True, exist_ok=False)
        self.base_sha = frozen(self.output/'global-experiment.json', self.base)
        frozen(self.output/'shared-actor-calibration.json', calibration)
        self.coordinator = NeuralCoordinator(config.get('device', 'cuda'))
        self.bridge = ChampionBridge(self.output, self.base_sha, islands=len(variants),
            migration_every=config['population_search'].get('migration_every',4),migrate=migrate)
        self.workers = workers; self.variants = variants; self.calibration = copy.deepcopy(calibration)
        self.owners = [None]*len(variants); self.closed = [False]*len(variants)
        self.sessions = [self.coordinator.session(str(i),config.get('seed',190019)) for i in range(len(variants))]
        self.factory = experiment_factory or self._default_factory
        frozen(self.output/'membership.json', dict(
            globalPopulation=config.get('members',4),
            islands=[c['asynchronous_global_member_ids']for c in variants],
            sameFullTrainingWorldCounts=[c.get('training_worlds',1)for c in variants],
            oneParentNeuralOwner=True, maximumConcurrentIslands=workers,
            independentTestPerIsland=False, productionAuthority=False))

    @staticmethod
    def _default_factory(config, directory):
        owner = Experiment.__new__(Experiment)
        try: owner.__init__(config,directory,data_use=WaitingDataUse())
        except BaseException:
            owner.close(); raise
        return owner

    def close(self):
        # SQLite connections/cursors are born, used and closed in their own
        # island thread. Do not disable SQLite's thread-affinity safeguard.
        if (self.coordinator.cancelled_reason is None
                and any(owner is not None and not closed for owner,closed in zip(self.owners,self.closed))):
            raise ValueError('ISLAND_SOURCE_OWNER_THREAD_NOT_FINISHED')

    def _run_island(self, index):
        owner = None
        try:
            with self.sessions[index].bind():
                owner = self.factory(self.variants[index], self.output/f'island-{index:03d}')
                expected = (self.output/f'island-{index:03d}').resolve()
                if not owner.output.resolve().is_relative_to(expected):
                    raise ValueError('PRIVATE_ISLAND_OUTPUT_NAMESPACE_REQUIRED')
                with self.bridge.lock:
                    if any(x is owner for x in self.owners):
                        raise ValueError('INDEPENDENT_ISLAND_EXPERIMENT_OBJECTS_REQUIRED')
                    self.owners[index] = owner
                owner.calibration = copy.deepcopy(self.calibration)
                frozen(owner.output/'compute-calibration.json', self.calibration)
                return owner.run(cycle_callback=lambda e,c,r,b:self.bridge.publish(index,e,c,r,b))
        except BaseException as error:
            self.coordinator.cancel(f'Owned island {index} failed: {type(error).__name__}: {error}')
            frozen(self.output/f'island-{index:03d}-failed.json', dict(
                type=type(error).__name__,reason=str(error),automaticRestart=False,
                unfinishedWorldsResumed=False,productionChanged=False))
            raise
        finally:
            try:
                if owner is not None: owner.close()
                self.closed[index] = True
            except BaseException as error:
                self.coordinator.cancel(f'Owned island {index} source close failed: {error}')
                frozen(self.output/f'island-{index:03d}-close-failed.json', dict(
                    type=type(error).__name__,reason=str(error),productionChanged=False))
                raise

    def _independent(self):
        if self.base['mode'] != 'full_history' or not self.base.get('test_finalist_once',False): return None
        champions = list(self.bridge.champions.values())
        if len({x['contextSha256']for x in champions}) != 1:
            raise ValueError('GLOBAL_FINALIST_COMPARABLE_CONTEXT_REQUIRED')
        winner = max(champions, key=lambda x:x['row']['selection']['reward']); row = winner['row']
        if row['selection']['reward'] <= 0: return None
        if self.base.get('replay_cache') is None:
            raise ValueError('LAZY_VERIFIED_SOURCE_REQUIRED_FOR_GLOBAL_TEST')
        owner = None
        with self.sessions[winner['island']].bind():
            try:
                # A fresh, identical source owner avoids moving a SQLite
                # connection from the completed island thread to this one.
                owner = self.factory(self.variants[winner['island']],self.output/'independent-finalist')
                if not owner.output.resolve().is_relative_to((self.output/'independent-finalist').resolve()):
                    raise ValueError('PRIVATE_FINALIST_OUTPUT_NAMESPACE_REQUIRED')
                owner.calibration = copy.deepcopy(self.calibration)
                if owner.spec_sha != winner['experimentSha256']:
                    raise ValueError('EXACT_FINALIST_EXPERIMENT_SOURCE_REQUIRED')
                if not independent_test_claim(owner.original.manifest, owner.split):
                    raise ValueError('GLOBAL_INDEPENDENT_SOURCE_PROVENANCE_REQUIRED')
                cp = row['checkpoint']; identity = digest(dict(competition='SIG_Predictions_Cup_2026',test_bounds_ms=owner.split.bounds('test')))
                registry = IndependentTest(self.base['test_registry'], identity)
                registry.freeze(cp['path'],cp['sha256'],row['selection']); registry.reserve(cp['sha256'])
                model,_ = load_checkpoint(cp['path'],cp['sha256'],owner.device)
                report = owner.evaluate(model,cp,'test',cp['sha256'])
                registry.finish(report); return report
            finally:
                if owner is not None: owner.close()

    def run(self):
        results = {}
        try:
            with ThreadPoolExecutor(self.workers) as pool:
                futures = {pool.submit(self._run_island,i):i for i in range(len(self.owners))}
                for future in as_completed(futures):
                    index = futures[future]; results[index] = future.result()
                    frozen(self.output/f'island-{index:03d}-completed.json', results[index])
            independent = self._independent()
            result = dict(classification='completed_asynchronous_research_cycles_not_production_profit',
                          islands=results, independentTestUsed=independent is not None,
                          independentResult=independent, productionChanged=False,
                          fullHistoryQualificationEstablished=False, productionProfitabilityEstablished=False)
            frozen(self.output/'SUMMARY.json', result); return result
        except BaseException as error:
            self.coordinator.cancel('Owned asynchronous research failed.')
            frozen(self.output/'FAILED.json', dict(type=type(error).__name__,reason=str(error),
                automaticRestart=False, unfinishedWorldsResumed=False, sourceOwnersClosed=self.closed,
                productionChanged=False))
            raise
        finally: self.close()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['config','output','calibration']: p.add_argument('--'+name,type=Path,required=True)
    for name in ['config-sha','calibration-sha']: p.add_argument('--'+name,required=True)
    p.add_argument('--workers',type=int,default=2); a = p.parse_args()
    def pinned(path, expected):
        raw = path.read_bytes()
        if path.is_symlink() or hashlib.sha256(raw).hexdigest() != expected: raise ValueError('PINNED_ISLAND_INPUT_CHANGED')
        return json.loads(raw)
    config = pinned(a.config,a.config_sha); calibration = pinned(a.calibration,a.calibration_sha)
    from .continue_population import verify_source
    verify_source(Path(__file__).resolve().parents[2])
    # The checked external handoff must require_idle_gpu BEFORE launching this
    # process, as for process_capacity. A venv redirector is itself visible in
    # process inventory; this CLI does not exempt arbitrary ancestor PIDs.
    runner = AsynchronousIslands(config,a.output,calibration,workers=a.workers)
    result = runner.run(); print(json.dumps(dict(completed=True,independentTestUsed=result['independentTestUsed'])))


if __name__ == '__main__': main()
