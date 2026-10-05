$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskPids=Get-Content -LiteralPath 'data/current-runtime-pids.json' -Raw|ConvertFrom-Json
$taskCollector=Get-CimInstance Win32_Process -Filter ('ProcessId='+$taskPids.collector)
if(!$taskCollector -or $taskCollector.Name -ne 'node.exe' -or $taskCollector.CommandLine -notmatch 'live-paper[/\\]runner\.mjs'){throw 'Exact collector not verified'}
foreach($taskSibling in @(@{pid=$taskPids.paper;pattern='probability-receipt-observe-v3\.mjs'},@{pid=$taskPids.dashboard;pattern='next.*--port 3000'})){
 $taskProcess=Get-CimInstance Win32_Process -Filter ('ProcessId='+$taskSibling.pid)
 if(!$taskProcess -or $taskProcess.Name -ne 'node.exe' -or $taskProcess.CommandLine -notmatch $taskSibling.pattern){throw 'Existing sibling service not verified'}
}
$taskEvidence=Get-Content -LiteralPath 'data/funded-settlement-paper-initialization-20261005.json' -Raw|ConvertFrom-Json
$taskActivationFile=Join-Path $taskRuntime 'competition-activation.json'
$taskActivation=(Get-FileHash -LiteralPath $taskActivationFile -Algorithm SHA256).Hash
if($taskActivation.ToLowerInvariant() -ne $taskEvidence.activationFileHash){throw 'Activation changed since initialization'}
$taskMarker=Join-Path $taskRuntime 'COLLECTOR_STOP'
foreach($taskStop in @((Join-Path $taskWorkspace 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'EXECUTION_STOP'),$taskMarker)){if(Test-Path -LiteralPath $taskStop){throw 'Existing STOP requires provenance review'}}
$taskBankFile=Join-Path $taskRuntime ('settlement-paper-cohorts/'+$taskEvidence.cohort+'/bank.json')
$taskBank=(Get-FileHash -LiteralPath $taskBankFile -Algorithm SHA256).Hash
$taskPayload=@{source='profit_sprint_deploy_20261005';at=[DateTime]::UtcNow.ToString('o');collectorPid=$taskPids.collector;activationChanged=$false}|ConvertTo-Json -Compress
$taskStream=[IO.File]::Open($taskMarker,[IO.FileMode]::CreateNew);try{$taskBytes=[Text.Encoding]::UTF8.GetBytes($taskPayload);$taskStream.Write($taskBytes,0,$taskBytes.Length);$taskStream.Flush($true)}finally{$taskStream.Dispose()}
for($taskAttempt=0;$taskAttempt -lt 60;$taskAttempt++){if(!(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue)){break};Start-Sleep -Milliseconds 500}
if(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue){throw 'Original collector still running'}
foreach($taskLock in @((Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock'),(Join-Path $taskRuntime 'competition-runs/sig-pilot-settlement-20261004-human/execution-owner.lock'),(Join-Path $taskRuntime ('settlement-paper-cohorts/'+$taskEvidence.cohort+'/worker.lock')))){if(Test-Path -LiteralPath $taskLock){throw 'Original lease remains'}}
if((Get-FileHash -LiteralPath $taskActivationFile -Algorithm SHA256).Hash -ne $taskActivation -or (Get-FileHash -LiteralPath $taskBankFile -Algorithm SHA256).Hash -ne $taskBank){throw 'Activation or new frozen bank changed'}
foreach($taskOld in $taskEvidence.preserved){$taskOldFile=Join-Path $taskRuntime ($taskOld.folder+'/'+$taskOld.cohort+'/bank.json');if((Get-FileHash -LiteralPath $taskOldFile -Algorithm SHA256).Hash.ToLowerInvariant() -ne $taskOld.bankFileHash){throw 'Original frozen bank changed'}}
if((Get-Content -LiteralPath $taskMarker -Raw) -ne $taskPayload){throw 'STOP provenance changed'}
Move-Item -LiteralPath $taskMarker -Destination ($taskMarker+'.profit-sprint-20261005.archived')
& (Join-Path $taskWorkspace 'Start-BaalServices.ps1')
@{at=[DateTime]::UtcNow.ToString('o');stoppedCollectorPid=$taskPids.collector;retainedPaperPid=$taskPids.paper;retainedDashboardPid=$taskPids.dashboard;cohort=$taskEvidence.cohort;bankHash=$taskEvidence.bankHash;activationUnchanged=$true;additionalAccountClients=0;additionalPersistentServices=0}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath 'data/profit-sprint-deployment-20261005.json'
