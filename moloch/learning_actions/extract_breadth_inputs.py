"""Extract only declared regular public inputs and verify every original byte."""
import argparse,hashlib,json,tarfile,shutil
from pathlib import Path
from .breadth_frame_index import sha
from .audit_breadth_inventory import write


def extract(archive,manifest,offset_manifest,output):
    import zstandard
    archive=Path(archive);manifest=Path(manifest);offset_manifest=Path(offset_manifest)
    raw=manifest.read_bytes();proof=json.loads(raw)
    if(proof['format']!='copied_original_sig_inputs_tar_zstd_v1'
            or proof['sourceOffsetManifestSha256']!=sha(offset_manifest)
            or proof['archiveBytes']!=archive.stat().st_size or proof['archiveSha256']!=sha(archive)):
        raise ValueError('PINNED_COMPLETE_PUBLIC_INPUT_ARCHIVE_REQUIRED')
    expected={m['file']:m for m in proof['members']}
    if len(expected)!=len(proof['members']):raise ValueError('UNIQUE_DECLARED_ARCHIVE_MEMBERS_REQUIRED')
    for name in expected:
        if(Path(name).name!=name or not name.startswith('breadth-')or not name.endswith('.jsonl')):
            raise ValueError('SCOPED_PUBLIC_ARCHIVE_MEMBER_REQUIRED')
    output=Path(output).resolve()
    if shutil.disk_usage(output.parent).free<proof['sourceBytes']+32*1024**3:
        raise ValueError('LOCAL_ORIGINAL_COPY_DISK_HEADROOM')
    output.mkdir(exist_ok=False,parents=True);seen=set();written=0
    try:
        with archive.open('rb')as src,zstandard.ZstdDecompressor().stream_reader(src)as decoded,tarfile.open(fileobj=decoded,mode='r|')as tar:
            for member in tar:
                name=member.name
                if name not in expected or name in seen or not member.isfile()or member.size!=expected[name]['bytes']:
                    raise ValueError('EXACT_REGULAR_DECLARED_ARCHIVE_MEMBER_REQUIRED')
                path=output/name
                if path.parent.resolve()!=output:raise ValueError('EXTRACTION_SCOPE_CHANGED')
                digest=hashlib.sha256();count=0
                with tar.extractfile(member)as source,path.open('xb')as target:
                    for block in iter(lambda:source.read(1024*1024),b''):
                        target.write(block);digest.update(block);count+=len(block)
                if count!=member.size or digest.hexdigest()!=expected[name]['sha256']:
                    raise ValueError('ORIGINAL_ARCHIVE_MEMBER_HASH_CHANGED')
                seen.add(name);written+=count
                write(output/'extraction-status.json',{'phase':'extracting_verified_originals',
                    'membersVerified':len(seen),'membersTotal':len(expected),'bytesVerified':written})
        if seen!=set(expected)or written!=proof['sourceBytes']or manifest.read_bytes()!=raw:
            raise ValueError('COMPLETE_UNCHANGED_ORIGINAL_COPY_REQUIRED')
        result={'format':'verified_local_original_sig_inputs_v1','archiveSha256':proof['archiveSha256'],
            'archiveManifestSha256':hashlib.sha256(raw).hexdigest(),'offsetManifestSha256':proof['sourceOffsetManifestSha256'],
            'members':len(seen),'bytesVerified':written,'allOriginalMemberHashesVerified':True,
            'marketClocksRenewed':False,'trainingAdmitted':False,'independentTestUsed':False,'productionChanged':False}
        write(output/'EXTRACTION_COMPLETE.json',result);return result
    except BaseException as error:
        write(output/'EXTRACTION_FAILED.json',{'errorType':type(error).__name__,'reason':str(error),
              'membersVerified':len(seen),'trainingAdmitted':False});raise


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for key in('archive','manifest','offset-manifest','output'):p.add_argument('--'+key,required=True)
    a=p.parse_args();print(json.dumps(extract(a.archive,a.manifest,a.offset_manifest,a.output)))
