param(
 [Parameter(Mandatory=$true)][string]$ValidatedRevisionEvidence,
 [switch]$Execute
)
$ErrorActionPreference='Stop'
$taskWorkspace=[IO.Path]::GetFullPath((Get-Location).Path)
$taskRuntime=[IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'))
$taskRunTag=[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfff')+'-'+[Guid]::NewGuid().ToString('N').Substring(0,8)
function Assert-Scoped([string]$taskPath,[string]$taskRoot){
 $taskResolved=[IO.Path]::GetFullPath($taskPath)
 if(!$taskResolved.StartsWith($taskRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Path outside intended workspace/runtime'}
 return $taskResolved
}
function Read-Prefix([string]$taskPath,[long]$taskLength=-1){
 $taskFile=Assert-Scoped $taskPath $taskRuntime
 $taskBefore=Get-Item -LiteralPath $taskFile
 if($taskLength -lt 0){$taskLength=$taskBefore.Length}
 if($taskLength -gt $taskBefore.Length){throw 'Preserved prefix truncated'}
 $taskHash=[Security.Cryptography.SHA256]::Create()
 $taskStream=[IO.File]::Open($taskFile,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
 $taskChunk=[byte[]]::new(1048576);$taskOffset=0L
 try{
  while($taskOffset -lt $taskLength){$taskRead=$taskStream.Read($taskChunk,0,[int][Math]::Min($taskChunk.Length,$taskLength-$taskOffset));if($taskRead -le 0){throw 'Preserved prefix read incomplete'};$null=$taskHash.TransformBlock($taskChunk,0,$taskRead,$taskChunk,0);$taskOffset+=$taskRead}
  $null=$taskHash.TransformFinalBlock([byte[]]::new(0),0,0)
  $taskDigest=[Convert]::ToHexString($taskHash.Hash).ToLowerInvariant()
 }finally{$taskStream.Dispose();$taskHash.Dispose()}
 $taskAfter=Get-Item -LiteralPath $taskFile
 if($taskAfter.CreationTimeUtc.Ticks -ne $taskBefore.CreationTimeUtc.Ticks -or $taskAfter.Length -lt $taskLength){throw 'Preserved file replaced/truncated'}
 return @{path=[IO.Path]::GetRelativePath($taskRuntime,$taskFile);bytes=$taskLength;hash=$taskDigest;creationTicks=$taskBefore.CreationTimeUtc.Ticks}
}
function Assert-Service([int]$taskPid,[string]$taskPattern){
 $taskProcess=Get-CimInstance Win32_Process -Filter ('ProcessId='+$taskPid)
 if(!$taskProcess -or $taskProcess.Name -ne 'node.exe' -or $taskProcess.CommandLine -notmatch $taskPattern){throw 'Exact existing service not verified'}
 return $taskProcess
}
function Assert-ThreeServices{
 foreach($taskPattern in @('live-paper[/\\]runner\.mjs','probability-receipt-observe-v3\.mjs','next.*--port 3000')){
  $taskFound=@(Get-CimInstance Win32_Process|Where-Object {$_.Name -eq 'node.exe' -and $_.CommandLine -match $taskPattern})
  if($taskFound.Count -ne 1){throw 'Exactly one instance of each existing service required'}
 }
}
$taskEvidenceFile=Assert-Scoped $ValidatedRevisionEvidence $taskWorkspace
$taskRevision=Get-Content -LiteralPath $taskEvidenceFile -Raw|ConvertFrom-Json
if($taskRevision.schemaVersion -ne 1 -or $taskRevision.testsPassed -ne $true -or !$taskRevision.sourceFiles -or $taskRevision.sourceFiles.Count -lt 1){throw 'Parent-validated revision evidence required'}
foreach($taskSource in $taskRevision.sourceFiles){
 $taskSourceFile=Assert-Scoped (Join-Path $taskWorkspace $taskSource.path) $taskWorkspace
 if($taskSource.hash -notmatch '^[a-f0-9]{64}$' -or (Get-FileHash -LiteralPath $taskSourceFile -Algorithm SHA256).Hash.ToLowerInvariant() -ne $taskSource.hash){throw 'Validated revision source changed'}
}
$taskPids=Get-Content -LiteralPath (Join-Path $taskWorkspace 'data/current-runtime-pids.json') -Raw|ConvertFrom-Json
$null=Assert-Service $taskPids.collector 'live-paper[/\\]runner\.mjs'
$null=Assert-Service $taskPids.paper 'probability-receipt-observe-v3\.mjs'
$null=Assert-Service $taskPids.dashboard 'next.*--port 3000'
Assert-ThreeServices
$taskMarker=Join-Path $taskRuntime 'COLLECTOR_STOP'
foreach($taskStop in @((Join-Path $taskWorkspace 'STOP'),(Join-Path $taskRuntime 'STOP'),(Join-Path $taskRuntime 'EXECUTION_STOP'),(Join-Path $taskRuntime 'PROBABILITY_STOP'),$taskMarker)){if(Test-Path -LiteralPath $taskStop){throw 'Existing STOP requires provenance review'}}
$taskExact=@()
foreach($taskRuntimeSource in @($taskRevision.runtimeFiles)){
 if(!$taskRuntimeSource){continue}
 $taskSaved=Read-Prefix (Join-Path $taskRuntime $taskRuntimeSource.path)
 if($taskRuntimeSource.hash -notmatch '^[a-f0-9]{64}$' -or $taskSaved.hash -ne $taskRuntimeSource.hash){throw 'Validated runtime revision/source changed'}
 $taskExact+=$taskSaved
}
foreach($taskName in @('competition-activation.json','coordinated-network-budget.json','COUPLED_BULK_ENTRY_DRAIN','dual-no-pair-active.json','dual-no-pair-v2-active.json','settlement-paper-active.json','calibrated-poll-paper-active.json','primary-breadth-paper-active.json','frozen-code/index.json')){
 $taskFile=Join-Path $taskRuntime $taskName
 if(Test-Path -LiteralPath $taskFile){$taskExact+=Read-Prefix $taskFile}
 elseif($taskName -in @('competition-activation.json','dual-no-pair-active.json','dual-no-pair-v2-active.json','settlement-paper-active.json')){throw 'Required current activation/cohort binding absent'}
}
$taskPrefixes=@();$taskOwnedWorkerLocks=@()
foreach($taskFolder in @('dual-no-pair-cohorts','dual-no-pair-v2-cohorts','settlement-paper-cohorts','research-cohorts')){
 $taskFolderPath=Join-Path $taskRuntime $taskFolder
 if(!(Test-Path -LiteralPath $taskFolderPath)){continue}
 foreach($taskDir in Get-ChildItem -LiteralPath $taskFolderPath -Directory){
  $taskBankFile=Join-Path $taskDir.FullName 'bank.json'
  if(Test-Path -LiteralPath $taskBankFile){$taskExact+=Read-Prefix $taskBankFile}
  $taskJournalFile=Join-Path $taskDir.FullName 'journal.jsonl'
  if(Test-Path -LiteralPath $taskJournalFile){$taskPrefixes+=Read-Prefix $taskJournalFile}
  if($taskFolder -ne 'research-cohorts'){$taskOwnedWorkerLocks+=Join-Path $taskDir.FullName 'worker.lock'}
 }
}
foreach($taskName in @('public-books.jsonl','market-prices.jsonl','anonymous-trades.jsonl')){$taskFile=Join-Path $taskRuntime $taskName;if(Test-Path -LiteralPath $taskFile){$taskPrefixes+=Read-Prefix $taskFile}}
$taskManifest=@{schemaVersion=1;at=[DateTime]::UtcNow.ToString('o');runTag=$taskRunTag;revisionEvidenceHash=(Get-FileHash -LiteralPath $taskEvidenceFile -Algorithm SHA256).Hash.ToLowerInvariant();priorPids=$taskPids;exactFiles=$taskExact;appendOnlyPrefixes=$taskPrefixes;executed=[bool]$Execute}
$taskManifestFile=Join-Path $taskWorkspace ('data/fundamental-revision-preflight-'+$taskRunTag+'.json')
$taskManifest|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $taskManifestFile
if(!$Execute){Write-Output 'Prepared read-only preservation manifest; no process or STOP changed. Parent must validate revision before -Execute.';return}
$taskPayload=@{source='fundamental_revision_deploy_20261005';at=[DateTime]::UtcNow.ToString('o');runTag=$taskRunTag;collectorPid=$taskPids.collector;activationChanged=$false;revisionEvidenceHash=$taskManifest.revisionEvidenceHash}|ConvertTo-Json -Compress
$taskStream=[IO.File]::Open($taskMarker,[IO.FileMode]::CreateNew)
try{$taskBytes=[Text.Encoding]::UTF8.GetBytes($taskPayload);$taskStream.Write($taskBytes,0,$taskBytes.Length);$taskStream.Flush($true)}finally{$taskStream.Dispose()}
for($taskAttempt=0;$taskAttempt -lt 60;$taskAttempt++){if(!(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue)){break};Start-Sleep -Milliseconds 500}
if(Get-Process -Id $taskPids.collector -ErrorAction SilentlyContinue){throw 'Original collector still running; STOP retained'}
$taskActivation=Get-Content -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Raw|ConvertFrom-Json
foreach($taskLock in @((Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock'),(Join-Path $taskRuntime ('competition-runs/'+$taskActivation.runId+'/execution-owner.lock')))+$taskOwnedWorkerLocks){if(Test-Path -LiteralPath $taskLock){throw 'Original collector/owned paper lease remains; STOP retained'}}
$null=Assert-Service $taskPids.paper 'probability-receipt-observe-v3\.mjs'
$null=Assert-Service $taskPids.dashboard 'next.*--port 3000'
foreach($taskSaved in $taskExact){$taskFile=Join-Path $taskRuntime $taskSaved.path;$taskAfter=Read-Prefix $taskFile;if($taskAfter.bytes -ne $taskSaved.bytes -or $taskAfter.hash -ne $taskSaved.hash -or $taskAfter.creationTicks -ne $taskSaved.creationTicks){throw 'Activation, pointer, frozen bank or operator setting changed; STOP retained'}}
foreach($taskSaved in $taskPrefixes){$taskAfter=Read-Prefix (Join-Path $taskRuntime $taskSaved.path) $taskSaved.bytes;if($taskAfter.hash -ne $taskSaved.hash -or $taskAfter.creationTicks -ne $taskSaved.creationTicks){throw 'Original journal/source prefix changed; STOP retained'}}
if((Get-Content -LiteralPath $taskMarker -Raw) -ne $taskPayload){throw 'STOP provenance changed'}
if((Get-FileHash -LiteralPath $taskEvidenceFile -Algorithm SHA256).Hash.ToLowerInvariant() -ne $taskManifest.revisionEvidenceHash){throw 'Revision validation changed; STOP retained'}
foreach($taskSource in $taskRevision.sourceFiles){if((Get-FileHash -LiteralPath (Join-Path $taskWorkspace $taskSource.path) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $taskSource.hash){throw 'Validated revision code changed; STOP retained'}}
Move-Item -LiteralPath $taskMarker -Destination ($taskMarker+'.fundamental-revision-'+$taskRunTag+'.archived')
& (Join-Path $taskWorkspace 'Start-BaalServices.ps1')
$taskRestarted=$false
for($taskAttempt=0;$taskAttempt -lt 20;$taskAttempt++){
 Start-Sleep -Milliseconds 500
 $taskNewPids=Get-Content -LiteralPath (Join-Path $taskWorkspace 'data/current-runtime-pids.json') -Raw|ConvertFrom-Json
 if($taskNewPids.collector -ne $taskPids.collector -and (Get-Process -Id $taskNewPids.collector -ErrorAction SilentlyContinue)){$taskRestarted=$true;break}
}
if(!$taskRestarted){throw 'Collector launch not verified; inspect detached launcher failure record'}
if($taskNewPids.paper -ne $taskPids.paper -or $taskNewPids.dashboard -ne $taskPids.dashboard){throw 'Sibling process unexpectedly replaced'}
Assert-ThreeServices
foreach($taskSaved in $taskExact){$taskAfter=Read-Prefix (Join-Path $taskRuntime $taskSaved.path);if($taskAfter.bytes -ne $taskSaved.bytes -or $taskAfter.hash -ne $taskSaved.hash){throw 'Preserved exact binding changed after launch'}}
foreach($taskSaved in $taskPrefixes){$taskAfter=Read-Prefix (Join-Path $taskRuntime $taskSaved.path) $taskSaved.bytes;if($taskAfter.hash -ne $taskSaved.hash -or $taskAfter.creationTicks -ne $taskSaved.creationTicks){throw 'Original journal/source prefix changed after launch'}}
$taskResult=@{schemaVersion=1;at=[DateTime]::UtcNow.ToString('o');runTag=$taskRunTag;stoppedCollectorPid=$taskPids.collector;newCollectorPid=$taskNewPids.collector;retainedPaperPid=$taskPids.paper;retainedDashboardPid=$taskPids.dashboard;activationUnchanged=$true;banksAndOriginalPrefixesVerified=$true;additionalAccountClients=0;additionalPersistentServices=0;startupOnly=$true;strategyProfitabilityEstablished=$false;preflightManifest=[IO.Path]::GetRelativePath($taskWorkspace,$taskManifestFile)}
$taskResultFile=Join-Path $taskWorkspace ('data/fundamental-revision-deployment-'+$taskRunTag+'.json')
$taskResult|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $taskResultFile
Copy-Item -LiteralPath $taskResultFile -Destination (Join-Path $taskWorkspace 'data/fundamental-revision-deployment-20261005.json')
Write-Output ('Collector restart verified; full ready/receipt checks remain separate. Evidence '+$taskResultFile)
