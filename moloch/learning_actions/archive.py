"""Preserve existing recorder byte snapshots before rolling journals prune them.

Filesystem only: no network, API clients, trading, collector reconfiguration or
original-file mutation. These are byte snapshots, not semantic replay seals.
"""
import argparse,hashlib,json,os,shutil,time,uuid
from pathlib import Path


class RecorderArchive:
    def __init__(self,root,sources,*,minimum_age_seconds=60,reserve_bytes=100*1024**3):
        self.root=Path(root).resolve()
        self.sources={name:Path(path).resolve()for name,path in sources.items()}
        if not self.sources or any(not name.replace('_','').isalnum()for name in sources):
            raise ValueError('EXPLICIT_NAMED_JOURNAL_SOURCES_REQUIRED')
        for path in self.sources.values():
            if not path.is_dir()or self.root==path or path in self.root.parents or self.root in path.parents:
                raise ValueError('ARCHIVE_MUST_BE_SEPARATE_FROM_EXISTING_SOURCE')
        self.root.mkdir(parents=True,exist_ok=True)
        self.age=minimum_age_seconds;self.reserve=reserve_bytes
        self.lock=(self.root/'archive.lock').open('a+b')
        if self.lock.seek(0,os.SEEK_END)==0:self.lock.write(b'0');self.lock.flush()
        self.lock.seek(0)
        if os.name=='nt':
            import msvcrt
            msvcrt.locking(self.lock.fileno(),msvcrt.LK_NBLCK,1)
        else:
            import fcntl
            fcntl.flock(self.lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        self.index={}
        if (self.root/'manifest.jsonl').exists():
            for line in (self.root/'manifest.jsonl').read_text(encoding='utf8').splitlines():
                row=json.loads(line);self.index[(row['source'],row['filename'])]=row

    def close(self):self.lock.close()

    def cycle(self):
        copied=0;copied_bytes=0;deferred=0
        for name,directory in self.sources.items():
            for source in sorted(directory.glob('*.gz')):
                if source.is_symlink()or source.resolve().parent!=directory:
                    raise ValueError('SOURCE_MUST_BE_ORDINARY_DIRECT_CHILD')
                before=source.stat();key=(name,source.name);old=self.index.get(key)
                if old and old['source_size']==before.st_size and old['source_mtime_ns']==before.st_mtime_ns:continue
                if self.age>0 and time.time()-before.st_mtime<self.age:deferred+=1;continue
                if shutil.disk_usage(self.root).free-before.st_size<self.reserve:
                    raise ValueError('ARCHIVE_DISK_RESERVE_REACHED')
                destination=self.root/name;destination.mkdir(exist_ok=True)
                temporary=destination/(uuid.uuid4().hex+'.partial');sha=hashlib.sha256();written=0
                try:
                    with source.open('rb')as src,temporary.open('xb')as dst:
                        for block in iter(lambda:src.read(4*1024**2),b''):
                            dst.write(block);sha.update(block);written+=len(block)
                        dst.flush();os.fsync(dst.fileno())
                    after=source.stat()
                    if (before.st_size,before.st_mtime_ns)!=(after.st_size,after.st_mtime_ns)or written!=before.st_size:
                        temporary.unlink();deferred+=1;continue
                    # Content-addressed short name also avoids legacy Windows
                    # path limits under the desktop app's redirected AppData.
                    digest=sha.hexdigest();target=destination/(digest+'.snapshot')
                    if target.exists():temporary.unlink()
                    else:temporary.rename(target)
                    # Verify destination bytes independently before recording.
                    check=hashlib.sha256()
                    with target.open('rb')as f:
                        for block in iter(lambda:f.read(4*1024**2),b''):check.update(block)
                    if check.hexdigest()!=digest:raise ValueError('ARCHIVED_BYTE_HASH_MISMATCH')
                    row={'source':name,'filename':source.name,'source_size':before.st_size,
                        'source_mtime_ns':before.st_mtime_ns,'sha256':digest,
                        'archive_relative_path':str(target.relative_to(self.root)),
                        'captured_unix':time.time(),'classification':'original_byte_snapshot_not_transport_validation'}
                    with (self.root/'manifest.jsonl').open('a',encoding='utf8')as f:
                        f.write(json.dumps(row)+'\n');f.flush();os.fsync(f.fileno())
                    self.index[key]=row;copied+=1;copied_bytes+=written
                except BaseException:
                    if temporary.exists():temporary.unlink()
                    raise
        return {'copied_files':copied,'copied_bytes':copied_bytes,'deferred_active_files':deferred,
            'preserved_source_files':len(self.index),'http_requests':0,'original_files_changed':False}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--config',type=Path,required=True)
    p.add_argument('--watch',action='store_true');args=p.parse_args()
    config=json.loads(args.config.read_text(encoding='utf-8-sig'))
    archive=RecorderArchive(config['archive'],config['sources'],
        minimum_age_seconds=config.get('minimum_age_seconds',60),reserve_bytes=config.get('reserve_bytes',100*1024**3))
    try:
        while True:
            result=archive.cycle();result['at_unix']=time.time()
            temporary=archive.root/'status.json.partial'
            temporary.write_text(json.dumps(result,indent=2));temporary.replace(archive.root/'status.json')
            print(json.dumps(result),flush=True)
            if not args.watch or (archive.root/'STOP').exists():break
            time.sleep(config.get('poll_seconds',30))
    finally:archive.close()


if __name__=='__main__':main()
