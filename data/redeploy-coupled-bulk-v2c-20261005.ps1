$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskPids=Get-Content -LiteralPath 'data/current-runtime-pids.json' -Raw|ConvertFrom-Json
$taskProcess=Get-CimInstance Win32_Process -Filter ('ProcessId='+$taskPids.collector)
if($taskProcess -and ($taskProcess.Name -ne 'node.exe' -or $taskProcess.CommandLine -notmatch 'live-paper[/\\]runner\.mjs')){throw 'Current collector identity mismatch'}
foreach($taskStop in @('STOP','EXECUTION_STOP')){if(Test-Path -LiteralPath (Join-Path $taskRuntime $taskStop)){throw 'Existing STOP requires review'}}
if(Test-Path -LiteralPath (Join-Path $taskWorkspace 'STOP')){throw 'Workspace STOP exists'}
$taskMarker=Join-Path $taskRuntime 'COLLECTOR_STOP'
if(Test-Path -LiteralPath $taskMarker){$taskPayload=Get-Content -LiteralPath $taskMarker -Raw;$taskSaved=$taskPayload|ConvertFrom-Json;if($taskSaved.source -ne 'coupled_bulk_paper_deployment_20261005' -or $taskSaved.collectorPid -ne $taskPids.collector -or $taskSaved.reason -ne 'Actual tape projection-state compatibility repair'){throw 'STOP provenance differs'}}else{
 if(!$taskProcess){throw 'Absent collector without deployment provenance'}
 $taskPayload=@{source='coupled_bulk_paper_deployment_20261005';at=[DateTime]::UtcNow.ToString('o');collectorPid=$taskPids.collector;reason='Actual tape projection-state compatibility repair';activationChanged=$false}|ConvertTo-Json -Compress
 $taskStream=[IO.File]::Open($taskMarker,[IO.FileMode]::CreateNew)
 try{$taskBytes=[Text.Encoding]::UTF8.GetBytes($taskPayload);$taskStream.Write($taskBytes,0,$taskBytes.Length);$taskStream.Flush($true)}finally{$taskStream.Dispose()}
}
for($taskAttempt=0;$taskAttempt -lt 30;$taskAttempt++){if(!(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue)){break};Start-Sleep -Milliseconds 500}
if(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue){throw 'Collector is alive; no duplicate launched'}
if(@(Get-CimInstance Win32_Process|Where-Object {$_.Name -eq 'node.exe' -and $_.CommandLine -match 'live-paper[/\\]runner\.mjs'}).Count){throw 'Another collector exists'}
$taskActiveFile=Join-Path $taskRuntime 'coupled-bulk-active.json'
$taskActive=Get-Content -LiteralPath $taskActiveFile -Raw|ConvertFrom-Json
if($taskActive.cohort -ne 'baal-coupled-bulk-20261005-v2b-live'){throw 'Failed cohort differs'}
$taskFailedDir=Join-Path $taskRuntime ('coupled-bulk-cohorts/'+$taskActive.cohort)
$taskPrior=Get-Content -LiteralPath (Join-Path $taskFailedDir 'journal.jsonl') -Tail 1|ConvertFrom-Json
if($taskPrior.state.paper.wallets.COUPLED.fills.Count -ne 0 -or @($taskPrior.state.paper.wallets.COUPLED.quotes.PSObject.Properties).Count -ne 0){throw 'Failed cohort acquired economic state'}
@{owner='Codex';at=[DateTime]::UtcNow.ToString('o');reason='Preserved failed startup: actual tape rows carry projection state';fills=0;economicStateReset=$false}|ConvertTo-Json|Out-File -LiteralPath (Join-Path $taskFailedDir 'STOP') -NoClobber -Encoding utf8
Move-Item -LiteralPath $taskActiveFile -Destination ($taskActiveFile+'.build-failed-v2b-20261005')
& node --max-old-space-size=4096 data/prepare-coupled-bulk-live-20261005.mjs
if($LASTEXITCODE -ne 0){throw 'New cohort failed; STOP retained'}
$taskEvidence=Get-Content -LiteralPath 'data/coupled-bulk-deployment-20261005.json' -Raw|ConvertFrom-Json
foreach($taskProperty in $taskEvidence.fixedArtifactHashes.PSObject.Properties){if((Get-FileHash -LiteralPath (Join-Path $taskRuntime $taskProperty.Name) -Algorithm SHA256).Hash -ne $taskProperty.Value){throw 'Prior pointer or execution configuration changed'}}
if((Get-Content -LiteralPath $taskMarker -Raw) -ne $taskPayload){throw 'STOP provenance changed'}
Move-Item -LiteralPath $taskMarker -Destination ($taskMarker+'.coupled-bulk-v2c.archived')
& (Join-Path $taskWorkspace 'Start-BaalServices.ps1')
@{at=[DateTime]::UtcNow.ToString('o');previousCollectorPid=$taskPids.collector;paperPid=$taskPids.paper;dashboardPid=$taskPids.dashboard;fixedArtifactsPreserved=$true;activationUnchanged=$true;priorFailedBanksPreserved=$true;additionalServices=0}|ConvertTo-Json|Set-Content -LiteralPath 'data/coupled-bulk-v2c-deployment-20261005.json'
