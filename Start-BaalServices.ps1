param([switch]$Child)
$ErrorActionPreference='Stop'
$taskWorkspace=$PSScriptRoot
$taskRuntime=Join-Path $env:LOCALAPPDATA 'SigTradingCup/paper-runtime'
# Request a detached current-user launcher outside the tool host's job. This
# does not grant privileges, create a task, or add a persistent watcher.
if (!$Child) {
  $taskPowerShell=(Get-Command pwsh.exe).Source
  $taskCommand='"'+$taskPowerShell+'" -NoProfile -NonInteractive -WindowStyle Hidden -File "'+$PSCommandPath+'" -Child'
  Add-Type -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class BaalDetachedLauncher {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct Startup {public int cb; public string reserved,desktop,title; public int x,y,w,h,cx,cy,fill,flags; public short show,reserved2; public IntPtr reservedPtr,input,output,error;}
 [StructLayout(LayoutKind.Sequential)] public struct ProcessInfo {public IntPtr process,thread; public int pid,tid;}
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern bool CreateProcess(string app,StringBuilder cmd,IntPtr pa,IntPtr ta,bool inherit,uint flags,IntPtr env,string cwd,ref Startup si,out ProcessInfo pi);
 [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
 public static int Start(string command,string cwd) {var s=new Startup();s.cb=Marshal.SizeOf(s);s.flags=1;s.show=0;ProcessInfo p;if(!CreateProcess(null,new StringBuilder(command),IntPtr.Zero,IntPtr.Zero,false,0x01000000u|0x08000000u,IntPtr.Zero,cwd,ref s,out p))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());CloseHandle(p.process);CloseHandle(p.thread);return p.pid;}
}
'@
  $taskLaunchPid=[BaalDetachedLauncher]::Start($taskCommand,$taskWorkspace)
  Write-Output "Detached service launcher PID $taskLaunchPid; inspect data/current-runtime-pids.json and service health"
  exit
}
$taskMutex=[Threading.Mutex]::new($false,'Local\BaalServiceLaunch2026')
$taskAcquired=$false
try {
  try{$taskAcquired=$taskMutex.WaitOne(0)}catch [Threading.AbandonedMutexException]{$taskAcquired=$true}
  if(!$taskAcquired){throw 'Another service launcher is active'}
  foreach($taskStop in @('STOP','COLLECTOR_STOP','EXECUTION_STOP','PROBABILITY_STOP')){
    if(Test-Path -LiteralPath (Join-Path $taskRuntime $taskStop)){throw 'Runtime STOP exists; inspect provenance before restarting'}
  }
  if(Test-Path -LiteralPath (Join-Path $taskWorkspace 'STOP')){throw 'Workspace STOP exists'}
  $taskActivation=Get-Content -LiteralPath (Join-Path $taskRuntime 'competition-activation.json') -Raw|ConvertFrom-Json
  $taskBudget=Get-Content -LiteralPath (Join-Path $taskRuntime 'coordinated-network-budget.json') -Raw|ConvertFrom-Json
  if($taskActivation.schemaVersion -ne 2 -or !$taskActivation.enabled -or $taskActivation.ownerId -ne 'Codex' -or $taskActivation.authorityKind -ne 'direct_human_assignment' -or $taskBudget.schemaVersion -ne 4 -or !$taskBudget.enabled -or !$taskBudget.soleAccountClient -or !$taskBudget.writesEnabled -or $taskBudget.executionOwner -ne 'Codex' -or $taskBudget.intervalMs -ne 1000 -or $taskBudget.writeIntervalMs -ne 2500){throw 'Current bounded human pilot or coordinated budget mismatch'}
  foreach($taskKey in @('handoffId','ackCommentId','authorityKind','assignmentCommentId')){if($taskBudget.$taskKey -ne $taskActivation.$taskKey){throw 'Pilot/budget authority mismatch'}}
  $taskRate=Get-Content -LiteralPath (Join-Path $taskRuntime 'rate.json') -Raw|ConvertFrom-Json
  if($taskRate.manualReview){throw 'Persistent rate review requires resolution'}
  $taskPaper=Get-Content -LiteralPath (Join-Path $taskRuntime 'probability-receipt-active.json') -Raw|ConvertFrom-Json
  if($taskPaper.cohort -notmatch '^[a-z][a-z0-9-]{1,80}$'){throw 'Invalid active paper cohort'}
  $taskPaperDir=Join-Path $taskRuntime ('probability-receipt-cohorts/'+$taskPaper.cohort)
  foreach($taskStop in @('STOP','OBSERVER_STOP')){if(Test-Path -LiteralPath (Join-Path $taskPaperDir $taskStop)){throw 'Active paper STOP exists'}}
  $taskNode=(Get-Command node.exe).Source
  $taskBuild='data/frontend-build-20261004-baal-final'
  if(!(Test-Path -LiteralPath (Join-Path $taskWorkspace ($taskBuild+'/.next/BUILD_ID')))){throw 'Verified dashboard build unavailable'}
  $taskPidsFile=Join-Path $taskWorkspace 'data/current-runtime-pids.json'
  $taskPids=Get-Content -LiteralPath $taskPidsFile -Raw|ConvertFrom-Json
  $taskServices=@(
    @{key='collector';pattern='live-paper[/\\]runner\.mjs';args=@('live-paper/runner.mjs');locks=@((Join-Path $env:LOCALAPPDATA 'SigTradingCup/bot.lock'),(Join-Path $taskRuntime ('competition-runs/'+$taskActivation.runId+'/execution-owner.lock')));port=8787},
    @{key='paper';pattern='live-paper[/\\]probability-receipt-observe-v3\.mjs';args=@('live-paper/probability-receipt-observe-v3.mjs','--name',$taskPaper.cohort);locks=@((Join-Path $taskPaperDir 'worker.lock'));port=0},
    @{key='dashboard';pattern='next[/\\]dist[/\\]bin[/\\]next.*--port 3000';args=@('frontend/node_modules/next/dist/bin/next','start',$taskBuild,'--hostname','127.0.0.1','--port','3000');locks=@();port=3000}
  )
  $taskRecovered=@()
  foreach($taskService in $taskServices){
    $taskProcesses=@(Get-CimInstance Win32_Process)
    $taskRunning=@($taskProcesses|Where-Object {$_.Name -eq 'node.exe' -and $_.CommandLine -match $taskService.pattern})
    if($taskRunning.Count -gt 1){throw 'Duplicate Baal service requires reconciliation'}
    if($taskRunning.Count -eq 1){$taskPids.($taskService.key)=$taskRunning[0].ProcessId;continue}
    if($taskService.port -and (Get-NetTCPConnection -LocalPort $taskService.port -State Listen -ErrorAction SilentlyContinue)){throw 'Service port belongs to an unrecognized process'}
    foreach($taskLock in $taskService.locks){
      if(!(Test-Path -LiteralPath $taskLock)){continue}
      $taskText=Get-Content -LiteralPath $taskLock -Raw
      $taskPreviousPid=if($taskText.Trim() -match '^\d+$'){[int]$taskText}else{[int]($taskText|ConvertFrom-Json).pid}
      if($taskPreviousPid -le 0 -or ($taskProcesses|Where-Object ProcessId -eq $taskPreviousPid)){throw 'Lock owner remains alive or ambiguous'}
      if((Get-Content -LiteralPath $taskLock -Raw) -ne $taskText){throw 'Lock changed during recovery'}
      $taskArchived=$taskLock+'.retired-'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfff')
      Move-Item -LiteralPath $taskLock -Destination $taskArchived
      $taskRecovered+=@{service=$taskService.key;previousPid=$taskPreviousPid}
    }
    $taskLogPrefix=Join-Path $taskRuntime ($taskService.key+'-detached-20261004')
    $taskProcess=Start-Process -FilePath $taskNode -ArgumentList $taskService.args -WorkingDirectory $taskWorkspace -WindowStyle Hidden -RedirectStandardOutput ($taskLogPrefix+'.stdout.log') -RedirectStandardError ($taskLogPrefix+'.stderr.log') -PassThru
    $taskPids.($taskService.key)=$taskProcess.Id
    $taskPids|ConvertTo-Json|Set-Content -LiteralPath $taskPidsFile
  }
  @{at=[DateTime]::UtcNow.ToString('o');pids=$taskPids;recoveredLocks=$taskRecovered;activationChanged=$false;accountingReset=$false;startupOnly=$true}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $taskWorkspace 'data/baal-detached-service-recovery-20261004.json')
}catch {
  @{at=[DateTime]::UtcNow.ToString('o');status='launcher_failed';reason=$_.Exception.Message}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $taskWorkspace 'data/baal-detached-service-failure-20261004.json')
  throw
}finally{if($taskAcquired){$taskMutex.ReleaseMutex()};$taskMutex.Dispose()}
