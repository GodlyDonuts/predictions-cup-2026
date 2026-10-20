"""Copy pinned public-frame inputs into a verified archive; retain originals."""
import argparse,hashlib,json,os,shutil,sqlite3,subprocess,time
from pathlib import Path
from .breadth_frame_index import FORMAT,sha
from .audit_breadth_inventory import identity,write


def archive(root,index,index_sha,output,*,minimum_free_bytes=8*1024**3):
    root=Path(root).resolve();index=Path(index);raw=(index/'complete.json').read_bytes()
    if hashlib.sha256(raw).hexdigest()!=index_sha:raise ValueError('PINNED_OFFSET_MANIFEST_REQUIRED')
    manifest=json.loads(raw)
    if manifest['format']!=FORMAT or sha(index/'frames.sqlite')!=manifest['databaseSha256']:
        raise ValueError('PINNED_OFFSET_DATABASE_REQUIRED')
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    db=sqlite3.connect((index/'frames.sqlite').resolve().as_uri()+'?mode=ro&immutable=1',uri=True)
    try:files=list(db.execute('SELECT name,sha,bytes FROM files ORDER BY i'))
    finally:db.close()
    identities={}
    for name,digest,size in files:
        p=root/name
        if (Path(name).name!=name or not name.startswith('breadth-')or not name.endswith('.jsonl')
                or p.is_symlink()or not p.is_file()or p.stat().st_size!=size):
            raise ValueError('REGULAR_SCOPED_SOURCE_REQUIRED')
        identities[name]=identity(p)
    (output/'members.list').write_bytes(b''.join(name.encode()+b'\0'for name,_,_ in files))
    part=output/'corpus.tar.zst.pending';tar=zstd=None;started=time.monotonic()
    try:
        if shutil.disk_usage(output).free<minimum_free_bytes:raise ValueError('ARCHIVE_DISK_HEADROOM')
        with(output/'tar.log').open('xb')as tlog,(output/'zstd.log').open('xb')as zlog:
            tar=subprocess.Popen(['tar','-C',str(root),'--null','-T',str(output/'members.list'),'-cf','-'],
                stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=tlog)
            zstd=subprocess.Popen(['zstd','-1','-T2','-o',str(part)],stdin=tar.stdout,stdout=subprocess.DEVNULL,stderr=zlog)
            tar.stdout.close()
            while zstd.poll()is None:
                if shutil.disk_usage(output).free<minimum_free_bytes:raise ValueError('ARCHIVE_DISK_HEADROOM')
                if time.monotonic()-started>3600:raise ValueError('ARCHIVE_TIME_BOUND')
                write(output/'status.json',{'phase':'compressing_original_copy','archiveBytes':part.stat().st_size if part.exists()else 0,'accountRequests':0})
                time.sleep(3)
            if zstd.returncode!=0 or tar.wait(timeout=10)!=0:raise ValueError('ARCHIVE_PIPELINE_FAILED')
        subprocess.run(['zstd','-t',str(part)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        if any(identity(root/name)!=before for name,before in identities.items()):raise ValueError('ORIGINAL_CHANGED_DURING_ARCHIVE')
        if(index/'complete.json').read_bytes()!=raw:raise ValueError('OFFSET_MANIFEST_CHANGED')
        target=output/'corpus.tar.zst';os.replace(part,target)
        result={'format':'copied_original_sig_inputs_tar_zstd_v1','sourceOffsetManifestSha256':index_sha,
            'archiveSha256':sha(target),'archiveBytes':target.stat().st_size,'sourceBytes':sum(x[2]for x in files),
            'members':[{'file':n,'sha256':h,'bytes':s}for n,h,s in files],
            'originalsRemoved':False,'localMemberHashVerificationStillRequired':True,
            'trainingAdmitted':False,'independentTestUsed':False,'productionChanged':False,'accountRequests':0}
        write(output/'complete.json',result);write(output/'status.json',{'phase':'complete_verified_compressed_copy','archiveBytes':result['archiveBytes'],'accountRequests':0})
        return result
    except BaseException as error:
        write(output/'FAILED.json',{'phase':'failed_originals_retained','errorType':type(error).__name__,'reason':str(error)});raise
    finally:
        for child in(zstd,tar):
            if child is not None and child.poll()is None:child.kill();child.wait()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for key in('root','index','index-sha256','output'):p.add_argument('--'+key,required=True)
    a=p.parse_args();r=archive(a.root,a.index,a.index_sha256,a.output)
    print(json.dumps({k:r[k]for k in('archiveBytes','sourceBytes','archiveSha256')}))
