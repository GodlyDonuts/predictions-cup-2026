"""Conservative diagnostic preflight; training has priority over analysis.

This is not a distributed device lease. CPU diagnostics remain available, and
callers must not schedule GPU diagnostics during a learning run or handoff.
"""
import json,os,subprocess


def training_pids():
    if os.name!='nt':raise ValueError('GPU_DIAGNOSTIC_OWNER_CHECK_UNAVAILABLE_USE_CPU')
    script=("$workers = @(Get-CimInstance Win32_Process -Filter \"Name = 'python.exe'\" | "
        "Where-Object { $_.ProcessId -ne "+str(os.getpid())+" -and $_.CommandLine -match "
        "'moloch[.]learning_actions[.](experiment|batch_benchmark|rollout_statistics_benchmark|acting_validation_benchmark|evaluation_heads_benchmark|evaluation_benchmark)' } | "
        "Select-Object -ExpandProperty ProcessId); ConvertTo-Json -InputObject @($workers) -Compress")
    result=subprocess.run(['powershell','-NoLogo','-NoProfile','-NonInteractive','-Command',script],
        capture_output=True,text=True,check=True,timeout=20,creationflags=getattr(subprocess,'CREATE_NO_WINDOW',0))
    rows=json.loads(result.stdout)
    if type(rows)is not list or any(type(p)is not int for p in rows):raise ValueError('GPU_DIAGNOSTIC_OWNER_CHECK_INVALID')
    return rows


def require_idle_gpu(device):
    if not str(device).startswith('cuda'):return
    owners=training_pids()
    if owners:raise ValueError('GPU_DIAGNOSTIC_BLOCKED_ACTIVE_ML_WORKERS:'+','.join(map(str,owners)))
    result=subprocess.run(['nvidia-smi','--query-compute-apps=pid','--format=csv,noheader,nounits'],
        capture_output=True,text=True,check=True,timeout=20,creationflags=getattr(subprocess,'CREATE_NO_WINDOW',0))
    other=[int(line.strip())for line in result.stdout.splitlines()if line.strip().isdigit()and int(line.strip())!=os.getpid()]
    if other:raise ValueError('GPU_DIAGNOSTIC_BLOCKED_OTHER_COMPUTE_PROCESSES:'+','.join(map(str,other)))
