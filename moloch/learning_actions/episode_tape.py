"""Compact frozen-behavior TRAIN episodes, exact replay and full-return labels.

Feature matrices are reconstructed, not retained for an entire multi-day run.
Each reconstructed frame must match its original tensor hash; each chunk must
match physical finance/liquidity. Only sealed, complete training episodes expose
return targets. This is not a financial-world restart or an exchange client.
"""
import hashlib,io,json,math,sqlite3
from pathlib import Path
import numpy as np
import torch
from .behavior_guard import identity,contract,require_contract
from .selection import frozen
from .batched import BatchStream,inputs
from .training import Chunk
from moloch.learning_v2.train_inner import fingerprint
from moloch.learning_realism.protocol import valid_sha


def tensor_digest(tree):
    digest=hashlib.sha256()
    def walk(node):
        if isinstance(node,dict):
            for key in sorted(node):digest.update(key.encode());walk(node[key])
        else:
            value=node.detach().cpu().contiguous()
            digest.update((str(value.dtype)+repr(tuple(value.shape))).encode());digest.update(value.numpy().tobytes())
    walk(tree);return digest.hexdigest()


def observation_fields(frame):return {k:frame[k]for k in('contracts','global_state','present','masks')}


def financial_digest(env):
    consumed,history=getattr(env,'_episode_fill_digest',(0,hashlib.sha256()))
    if consumed>len(env.api_fills):raise ValueError('APPEND_ONLY_EPISODE_FILLS_REQUIRED')
    for row in env.api_fills[consumed:]:history.update(json.dumps(row,sort_keys=True,allow_nan=False).encode())
    env._episode_fill_digest=(len(env.api_fills),history)
    books=[(key,b.book,b.invalidated,sorted(b.depleted.items()))for key,b in sorted(env.books.items())]
    value=(env.now,env.cash,sorted(env.lots.items()),env.realized,env.fees,
           sorted(env.api_orders.items()),env.api_id,len(env.api_fills),history.hexdigest(),env.finance_revision,books)
    if env.order_history is not None:value=(*value,env.order_history.digest)
    return hashlib.sha256(repr(value).encode()).hexdigest()


def array_bytes(tensor):
    buffer=io.BytesIO();np.save(buffer,tensor.detach().cpu().numpy(),allow_pickle=False);return buffer.getvalue()


class EpisodeWriter:
    def __init__(self,path,stream,*,dataset_sha256,specification_sha256):
        if not isinstance(stream,BatchStream)or any(e.role!='train'for e in stream.envs):
            raise ValueError('ONLY_TRAIN_BATCH_EPISODES_MAY_BE_RECORDED')
        if not valid_sha(dataset_sha256)or not valid_sha(specification_sha256):raise ValueError('PINNED_EPISODE_SOURCE_REQUIRED')
        self.path=Path(path);self.path.mkdir(parents=True,exist_ok=False);self.stream=stream
        self.worlds={id(e):i for i,e in enumerate(stream.envs)};self.clocks=[e.now for e in stream.envs]
        self.guard=identity(stream.model,stream.behavior_guard);self.actor=fingerprint(stream.model)
        self.metadata={'classification':'closed_frozen_behavior_training_episode','role':'train',
            'dataset_sha256':dataset_sha256,'specification_sha256':specification_sha256,
            'actor_sha256':self.actor,'architecture':stream.model.architecture,
            'behavior_contract':contract(stream.model),
            'observation_protocol':stream.model.observation_protocol,'calibration':stream.calibration,
            'world_bounds':[[e.now,e.end]for e in stream.envs],
            'production_authority':False,'independent_test_used':False}
        self.db=sqlite3.connect(self.path/'episode.sqlite');self.sealed=False;self.batches=0
        self.db.executescript('''
CREATE TABLE batches(id INTEGER PRIMARY KEY,worlds TEXT NOT NULL,hidden BLOB NOT NULL,final_state TEXT NOT NULL);
CREATE TABLE frames(batch INTEGER,step INTEGER,payload TEXT NOT NULL,observation_sha TEXT NOT NULL,PRIMARY KEY(batch,step));
CREATE TABLE targets(batch INTEGER,step INTEGER,world INTEGER,learn INTEGER,reward REAL,old_value REAL,target REAL,
 PRIMARY KEY(batch,step,world));
CREATE INDEX target_world ON targets(world,batch,step);
''')

    def record(self,chunk):
        require_contract(self.stream.model,chunk.behavior_contract)
        if self.sealed or chunk.role!='train' or not isinstance(chunk.stream,BatchStream):raise ValueError('OPEN_TRAIN_EPISODE_REQUIRED')
        if identity(self.stream.model,self.stream.behavior_guard)!=self.guard or chunk.behavior_sha256!=self.guard:
            raise ValueError('BEHAVIOR_POLICY_CHANGED_DURING_EPISODE')
        envs=chunk.stream.envs;worlds=[self.worlds[id(e)]for e in envs];batch=self.batches
        self.db.execute('INSERT INTO batches VALUES(?,?,?,?)',(batch,json.dumps(worlds),array_bytes(chunk.initial_hidden),
            json.dumps([financial_digest(e)for e in envs])))
        for step,frame in enumerate(chunk.frames):
            data={k:frame[k].detach().cpu().tolist()for k in('old_logprob','old_value','reset','terminal','learn')}
            data['actions']={k:v.detach().cpu().tolist()for k,v in frame['actions'].items()}
            data['action_dtypes']={k:str(v.dtype).split('.')[-1]for k,v in frame['actions'].items()}
            data['value_dtype']=str(frame['old_value'].dtype).split('.')[-1]
            data['reward']=list(frame['economic_reward']);data['elapsed']=list(frame['elapsed_ms'])
            data['before']=[self.clocks[i]for i in worlds]
            for j,i in enumerate(worlds):
                if data['elapsed'][j]<0:raise ValueError('MONOTONIC_EPISODE_CLOCK')
                self.clocks[i]+=data['elapsed'][j]
                if not math.isfinite(data['reward'][j]+data['old_logprob'][j]+data['old_value'][j]):
                    raise ValueError('FINITE_RECORDED_EPISODE_REQUIRED')
                self.db.execute('INSERT INTO targets VALUES(?,?,?,?,?,?,NULL)',
                    (batch,step,i,int(data['learn'][j]),data['reward'][j],data['old_value'][j]))
            data['after']=[self.clocks[i]for i in worlds]
            self.db.execute('INSERT INTO frames VALUES(?,?,?,?)',(batch,step,json.dumps(data,allow_nan=False),
                tensor_digest(observation_fields(frame))))
        if any(self.clocks[i]!=e.now for i,e in zip(worlds,envs)):raise ValueError('RECORDED_CHUNK_CLOCK_MISMATCH')
        self.db.commit();self.batches+=1

    def seal(self):
        if self.sealed:raise ValueError('EPISODE_ALREADY_SEALED')
        require_contract(self.stream.model,self.metadata['behavior_contract'])
        if identity(self.stream.model,self.stream.behavior_guard)!=self.guard:
            raise ValueError('BEHAVIOR_POLICY_CHANGED_DURING_EPISODE')
        if any(e.now!=e.end or not ledger.closed for e,ledger in zip(self.stream.envs,self.stream.ledgers)):
            raise ValueError('ALL_TRAIN_EPISODES_MUST_COMPLETE_BEFORE_RETURNS')
        if self.clocks!=[e.end for e in self.stream.envs]:raise ValueError('EVERY_EPISODE_DECISION_MUST_BE_RECORDED')
        if fingerprint(self.stream.model)!=self.actor:raise ValueError('BEHAVIOR_POLICY_CHANGED_DURING_EPISODE')
        count=0;mean=0.;m2=0.;scores=[]
        for world in range(len(self.worlds)):
            value=0.;updates=[]
            for batch,step,reward,old_value in self.db.execute(
                'SELECT batch,step,reward,old_value FROM targets WHERE world=? AND learn=1 ORDER BY batch DESC,step DESC',(world,)):
                value+=reward;advantage=value-old_value;count+=1;delta=advantage-mean;mean+=delta/count;m2+=delta*(advantage-mean)
                updates.append((value,batch,step,world))
                if len(updates)>=1024:
                    self.db.executemany('UPDATE targets SET target=? WHERE batch=? AND step=? AND world=?',updates);updates=[]
            self.db.executemany('UPDATE targets SET target=? WHERE batch=? AND step=? AND world=?',updates)
            final=self.stream.completed[world]
            reward_sum=math.fsum(r[0]for r in self.db.execute('SELECT reward FROM targets WHERE world=? AND learn=1',(world,)))
            if not math.isclose(reward_sum,final['reward'],rel_tol=1e-10,abs_tol=1e-10):
                raise ValueError('FULL_RECORDED_REWARDS_MUST_EQUAL_ORIGINAL_SCORE')
            scores.append(final)
        if not count:raise ValueError('NONEMPTY_TRAIN_EPISODES_REQUIRED')
        self.db.execute('UPDATE targets SET target=0 WHERE learn=0');self.db.commit();self.db.close();self.sealed=True
        with(self.path/'episode.sqlite').open('rb')as stream:database_sha=hashlib.file_digest(stream,'sha256').hexdigest()
        self.metadata.update(complete=True,batches=self.batches,decisions=count,results=scores,
            advantage_mean=mean,advantage_std=max(1e-8,math.sqrt(max(0,m2/count))),
            database_sha256=database_sha)
        return frozen(self.path/'COMPLETE.json',self.metadata)

    def close(self):
        if not self.sealed:self.db.close()


class EpisodeReader:
    def __init__(self,path,manifest_sha256):
        self.path=Path(path);raw=(self.path/'COMPLETE.json').read_bytes()
        if hashlib.sha256(raw).hexdigest()!=manifest_sha256:raise ValueError('CLOSED_EPISODE_MANIFEST_CHANGED')
        self.metadata=json.loads(raw);self.manifest_sha256=manifest_sha256;m=self.metadata
        if m.get('complete')is not True or m.get('role')!='train' or m.get('production_authority')is not False or m.get('independent_test_used')is not False:
            raise ValueError('ONLY_COMPLETE_TRAIN_EPISODE_RETURNS')
        with(self.path/'episode.sqlite').open('rb')as stream:
            if hashlib.file_digest(stream,'sha256').hexdigest()!=m['database_sha256']:raise ValueError('CLOSED_EPISODE_DATABASE_CHANGED')
        self.db=sqlite3.connect((self.path/'episode.sqlite').resolve().as_uri()+'?mode=ro&immutable=1',uri=True)

    def replay(self,envs,ledgers,device='cpu'):
        if any(e.role!='train'for e in envs)or [[e.now,e.end]for e in envs]!=self.metadata['world_bounds']:
            raise ValueError('EXACT_ORIGINAL_TRAIN_EPISODES_REQUIRED')
        if len(ledgers)!=len(envs)or any(ledger.env is not env for ledger,env in zip(ledgers,envs)):
            raise ValueError('EXACT_TRAIN_REWARD_LEDGERS_REQUIRED')
        for batch,world_json,hidden_blob,final_json in self.db.execute('SELECT * FROM batches ORDER BY id'):
            worlds=json.loads(world_json);selected=[envs[i]for i in worlds];frames=[];returns=[]
            hidden=torch.from_numpy(np.load(io.BytesIO(hidden_blob),allow_pickle=False)).to(device)
            for step,payload,expected in self.db.execute('SELECT step,payload,observation_sha FROM frames WHERE batch=? ORDER BY step',(batch,)):
                row=json.loads(payload)
                for j,e in enumerate(selected):
                    if e.now!=row['before'][j]:raise ValueError('EXACT_ACTION_REPLAY_CLOCK_REQUIRED')
                    if row['learn'][j]:e.advance(e.now)
                observed=[e.observe()for e in selected];prepared=inputs(observed,device)
                if tensor_digest(prepared)!=expected:raise ValueError('EXACT_ACTION_REPLAY_OBSERVATION_REQUIRED')
                for j,(i,e)in enumerate(zip(worlds,selected)):
                    if not row['learn'][j]:continue
                    before=e.now;e.advance(min(e.end,before+self.metadata['calibration']['decision_latency_ms']))
                    if e.now<e.end:
                        result=e.act_raw({k:v[j]for k,v in row['actions'].items()},observed[j])
                        if result['status']=='ALARM':
                            target=e.decision_times(result.get('ready_ms'));e.advance_until_observable(min(e.end,max(before+1,target)))
                    reward=ledgers[i].transition()
                    if e.now>=e.end:reward+=ledgers[i].finish()['terminal_reward_correction']
                    if e.now!=row['after'][j]or reward!=row['reward'][j]:raise ValueError('EXACT_ACTION_REPLAY_CLOCK_AND_REWARD_REQUIRED')
                frame={**prepared,'actions':{k:torch.tensor(v,dtype=getattr(torch,row['action_dtypes'][k]),device=device)
                    for k,v in row['actions'].items()},'reset':torch.tensor(row['reset'],device=device),
                    'terminal':torch.tensor(row['terminal'],device=device),'learn':torch.tensor(row['learn'],device=device),
                    'old_logprob':torch.tensor(row['old_logprob'],dtype=torch.float64,device=device),
                    'old_value':torch.tensor(row['old_value'],dtype=getattr(torch,row['value_dtype']),device=device),
                    'economic_reward':row['reward'],'elapsed_ms':row['elapsed']}
                frames.append(frame)
                targets=dict(self.db.execute('SELECT world,target FROM targets WHERE batch=? AND step=?',(batch,step)))
                returns.append([targets[i]for i in worlds])
            if [financial_digest(e)for e in selected]!=json.loads(final_json):raise ValueError('EXACT_ACTION_REPLAY_FINANCIAL_STATE_REQUIRED')
            chunk=Chunk(frames,self.metadata['actor_sha256'],'train',hidden,
                torch.zeros(len(worlds),device=device),None,'sha256',self.metadata.get('behavior_contract'))
            chunk.episode_reader=self;chunk.episode_batch=batch;chunk.episode_worlds=worlds
            yield chunk,returns
        if any(e.now!=e.end or not l.closed for e,l in zip(envs,ledgers)):
            raise ValueError('REPLAY_MUST_COMPLETE_EVERY_TRAIN_EPISODE')

    def close(self):self.db.close()
