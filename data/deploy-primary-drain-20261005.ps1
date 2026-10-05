$ErrorActionPreference='Stop'
$taskWorkspace=(Get-Location).Path
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
$taskPids=Get-Content -LiteralPath 'data/current-runtime-pids.json' -Raw|ConvertFrom-Json
$taskCollector=Get-CimInstance Win32_Process -Filter ('ProcessId='+$taskPids.collector)
if(!$taskCollector -or $taskCollector.Name -ne 'node.exe' -or $taskCollector.CommandLine -notmatch 'live-paper[/\\]runner\.mjs'){throw 'Exact collector not verified'}
$taskActivationFile=Join-Path $taskRuntime 'competition-activation.json'
$taskActivation=(Get-FileHash -LiteralPath $taskActivationFile -Algorithm SHA256).Hash
$taskBulk=Get-Content -LiteralPath (Join-Path $taskRuntime 'coupled-bulk-active.json') -Raw|ConvertFrom-Json
$taskBankFile=Join-Path $taskRuntime ('coupled-bulk-cohorts/'+$taskBulk.cohort+'/bank.json')
$taskBank=(Get-FileHash -LiteralPath $taskBankFile -Algorithm SHA256).Hash
$taskMarker=Join-Path $taskRuntime 'COLLECTOR_STOP'
foreach($taskStop in @((Join-Path $taskWorkspace 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'EXECUTION_STOP'),$taskMarker,(Join-Path $taskRuntime 'COUPLED_BULK_ENTRY_DRAIN'))){if(Test-Path -LiteralPath $taskStop){throw 'Existing STOP/drain requires provenance review'}}
$taskPayload=@{source='primary_breadth_drain_deploy_20261005';at=[DateTime]::UtcNow.ToString('o');collectorPid=$taskPids.collector;activationChanged=$false}|ConvertTo-Json -Compress
$taskStream=[IO.File]::Open($taskMarker,[IO.FileMode]::CreateNew);try{$taskBytes=[Text.Encoding]::UTF8.GetBytes($taskPayload);$taskStream.Write($taskBytes,0,$taskBytes.Length);$taskStream.Flush($true)}finally{$taskStream.Dispose()}
for($taskAttempt=0;$taskAttempt -lt 40;$taskAttempt++){if(!(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue)){break};Start-Sleep -Milliseconds 500}
if(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue){throw 'Original collector still running'}
if(!(Get-Process -Id $taskPids.paper -ErrorAction SilentlyContinue) -or !(Get-Process -Id $taskPids.dashboard -ErrorAction SilentlyContinue)){throw 'Existing companion services unexpectedly absent'}
foreach($taskLock in @((Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock'),(Join-Path $taskRuntime 'competition-runs/sig-pilot-settlement-20261004-human/execution-owner.lock'))){if(Test-Path -LiteralPath $taskLock){throw 'Original collector lease remains'}}
if((Get-FileHash -LiteralPath $taskActivationFile -Algorithm SHA256).Hash -ne $taskActivation -or (Get-FileHash -LiteralPath $taskBankFile -Algorithm SHA256).Hash -ne $taskBank){throw 'Activation or frozen bank changed'}
if((Get-Content -LiteralPath $taskMarker -Raw) -ne $taskPayload){throw 'STOP provenance changed'}
$taskDrain=@{source='human_profitability_focus_20261005';owner='Codex';at=[DateTime]::UtcNow.ToString('o');cohort=$taskBulk.cohort;bankHash=$taskBulk.bankHash;mode='drain_existing_inventory_and_quotes';reason='Stop unprofitable new passive entries; retain every original liability and owned exit';competitionOrders=0}|ConvertTo-Json -Compress
[IO.File]::WriteAllText((Join-Path $taskRuntime 'COUPLED_BULK_ENTRY_DRAIN'),$taskDrain,[Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $taskMarker -Destination ($taskMarker+'.primary-drain-20261005.archived')
& (Join-Path $taskWorkspace 'Start-BaalServices.ps1')
@{at=[DateTime]::UtcNow.ToString('o');stoppedCollectorPid=$taskPids.collector;retainedPaperPid=$taskPids.paper;retainedDashboardPid=$taskPids.dashboard;bulkCohort=$taskBulk.cohort;bulkBankFileHash=$taskBank;activationUnchanged=$true;additionalAccountClients=0;additionalPersistentServices=0}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath 'data/primary-drain-deployment-20261005.json'
