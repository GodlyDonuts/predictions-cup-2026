param(
  [ValidatePattern('^[1-9][0-9]*$')][string]$MarketId,
  [ValidatePattern('^[1-9][0-9]*$')][string]$ExchangeId,
  [Parameter(Mandatory=$true)][string]$OutputDirectory,
  [switch]$HumanAuthorizedSharedReadCapture,
  [ValidateRange(1,24)][int]$HistoryDays=7,
  [ValidateRange(1,100)][int]$Cycles=1,
  [switch]$AllHistory,
  [switch]$HistoryOnly,
  [switch]$DiagnosticBookProbe,
  [switch]$CatalogOnly,
  [string]$ScreenPairsFile
)
$ErrorActionPreference='Stop'
if(-not $HumanAuthorizedSharedReadCapture){throw 'EXPLICIT_SHARED_READ_AUTHORIZATION_REQUIRED: preserve the prior one-collector policy unless human explicitly authorizes recreating read-only capture.'}
if($CatalogOnly -and $ScreenPairsFile){throw 'ONE_DISCOVERY_MODE_REQUIRED'}
if(-not $CatalogOnly -and -not $ScreenPairsFile -and (-not $MarketId -or -not $ExchangeId)){throw 'MARKET_AND_EXCHANGE_REQUIRED'}
$taskOutput=[IO.Path]::GetFullPath($OutputDirectory)
if(Test-Path -LiteralPath $taskOutput){throw 'OUTPUT_EXISTS: preserve previous capture; choose a new directory.'}
New-Item -ItemType Directory -Path $taskOutput | Out-Null
$taskMutex=[Threading.Mutex]::new($false,'Local\Moloch-SIG-Read-Capture')
if(-not $taskMutex.WaitOne(0)){ $taskMutex.Dispose();throw 'EXISTING_MOLOCH_CAPTURE_ACTIVE' }
$taskBudgetDirectory=Join-Path $env:LOCALAPPDATA 'Moloch'
New-Item -ItemType Directory -Path $taskBudgetDirectory -Force | Out-Null
$taskBudgetFile=Join-Path $taskBudgetDirectory 'sig-read-budget.json'
$taskBudget=if(Test-Path -LiteralPath $taskBudgetFile){Get-Content -LiteralPath $taskBudgetFile -Raw | ConvertFrom-Json}else{[pscustomobject]@{times=@();nextAt=0;blocked=$false}}
if($taskBudget.blocked -and -not $DiagnosticBookProbe){$taskMutex.ReleaseMutex();$taskMutex.Dispose();throw 'SIG_LOCAL_BUDGET_BLOCKED: previous request failed; inspect evidence before another run.'}
$taskTournament='bda92870-621e-47b0-bc3c-3602c5c26f55'
$taskCredential=Join-Path $env:LOCALAPPDATA 'Codex\secrets\sig-predictions-cup-2026.dpapi'
$taskSecure=Get-Content -LiteralPath $taskCredential -Raw | ConvertTo-SecureString
$taskPointer=[Runtime.InteropServices.Marshal]::SecureStringToBSTR($taskSecure)
$taskKey=$null
try {
  $taskKey=[Runtime.InteropServices.Marshal]::PtrToStringBSTR($taskPointer)
  if($taskKey -notmatch '^ace_[a-zA-Z0-9]+$'){throw 'APPROVED_CREDENTIAL_FORMAT'}
  $taskHeaders=@{Authorization=('Bearer '+$taskKey)}
  $taskEnd=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:00:00.000Z')
  $taskFrom=if($AllHistory){'1970-01-01T00:00:00.000Z'}else{[DateTime]::UtcNow.AddDays(-$HistoryDays).ToString('yyyy-MM-ddTHH:00:00.000Z')}
  $taskTapeFrom=[DateTime]::UtcNow.AddMinutes(-15).ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
  $taskTapeTo=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
  $taskRequests=@(
    @{kind='book';endpoint="/markets/$MarketId/orderbook?tournamentId=$taskTournament&depth=200"},
    @{kind='trades';endpoint="/exchanges/$ExchangeId/trades?tournamentId=$taskTournament&from=$([Uri]::EscapeDataString($taskTapeFrom))&to=$([Uri]::EscapeDataString($taskTapeTo))&limit=200"},
    @{kind='history';endpoint="/exchanges/$ExchangeId/price-history?tournamentId=$taskTournament&resolution=1h&from=$([Uri]::EscapeDataString($taskFrom))&to=$([Uri]::EscapeDataString($taskEnd))&limit=1000"}
  )
  if($DiagnosticBookProbe){$taskRequests=@($taskRequests | Where-Object kind -eq 'book')}elseif($HistoryOnly){$taskRequests=@($taskRequests | Where-Object kind -eq 'history')}elseif($Cycles -gt 1){$taskRequests=@($taskRequests)+@(for($taskCycle=1;$taskCycle -lt $Cycles;$taskCycle++){$taskRequests[0];$taskRequests[1]})}
  if($CatalogOnly){$taskRequests=@(@{kind='catalog';endpoint='/tournaments/midterm-elections/markets?status=open&limit=100'})}
  if($ScreenPairsFile){
    $taskPairs=@(Get-Content -LiteralPath $ScreenPairsFile -Raw | ConvertFrom-Json -DateKind String)
    if($taskPairs.Count -lt 1 -or $taskPairs.Count -gt 30){throw 'SCREEN_SCOPE_ONE_TO_THIRTY_PAIRS'}
    $taskRequests=@(foreach($taskPair in $taskPairs){
      if([string]$taskPair.marketId -notmatch '^[1-9][0-9]*$' -or [string]$taskPair.exchangeId -notmatch '^[1-9][0-9]*$'){throw 'SCREEN_SCOPE_INVALID'}
      @{kind='book';marketId=[string]$taskPair.marketId;exchangeId=[string]$taskPair.exchangeId;endpoint="/markets/$($taskPair.marketId)/orderbook?tournamentId=$taskTournament&depth=200"}
      @{kind='trades';marketId=[string]$taskPair.marketId;exchangeId=[string]$taskPair.exchangeId;endpoint=''}
    })
    $taskScreenCycle=@($taskRequests)
    if($Cycles -gt 1){$taskRequests=@(for($taskCycle=0;$taskCycle -lt $Cycles;$taskCycle++){$taskScreenCycle})}
    $taskRecovery=@(foreach($taskPair in $taskPairs){
      if($taskPair.recoveryFrom -or $taskPair.recoveryTo){
        $taskRecoveryFrom=[DateTimeOffset]::Parse($taskPair.recoveryFrom)
        $taskRecoveryTo=[DateTimeOffset]::Parse($taskPair.recoveryTo)
        if($taskRecoveryFrom -ge $taskRecoveryTo -or $taskRecoveryTo -gt [DateTimeOffset]::UtcNow){throw 'RECOVERY_WINDOW_INVALID'}
        $taskRecoveryFromText=$taskRecoveryFrom.UtcDateTime.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
        $taskRecoveryToText=$taskRecoveryTo.UtcDateTime.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
        @{kind='trades';fixedWindow=$true;marketId=[string]$taskPair.marketId;exchangeId=[string]$taskPair.exchangeId;endpoint="/exchanges/$($taskPair.exchangeId)/trades?tournamentId=$taskTournament&from=$([Uri]::EscapeDataString($taskRecoveryFromText))&to=$([Uri]::EscapeDataString($taskRecoveryToText))&limit=200"}
      }
    })
    $taskRequests=$taskRecovery+@($taskRequests)
  }
  $taskReceipts=0
  for($taskIndex=0;$taskIndex -lt $taskRequests.Count;$taskIndex++){
    $taskRequest=$taskRequests[$taskIndex]
    $taskMarket=if($taskRequest.marketId){$taskRequest.marketId}else{$MarketId}
    $taskExchange=if($taskRequest.exchangeId){$taskRequest.exchangeId}else{$ExchangeId}
    if(Test-Path -LiteralPath (Join-Path $taskOutput 'STOP')){break}
    if($taskRequest.kind -eq 'trades' -and -not $taskRequest.fixedWindow){
      $taskTapeFrom=[DateTime]::UtcNow.AddMinutes(-15).ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
      $taskTapeTo=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
      $taskRequest=@{kind='trades';endpoint="/exchanges/$taskExchange/trades?tournamentId=$taskTournament&from=$([Uri]::EscapeDataString($taskTapeFrom))&to=$([Uri]::EscapeDataString($taskTapeTo))&limit=200"}
    }
    while($true){
      if(Test-Path -LiteralPath (Join-Path $taskOutput 'STOP')){break}
      $taskNow=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
      $taskBudget.times=@($taskBudget.times | Where-Object {$_ -gt ($taskNow-60000)})
      $taskWaitUntil=[long]$taskBudget.nextAt
      if($taskBudget.times.Count -ge 5){$taskWaitUntil=[Math]::Max($taskWaitUntil,[long]$taskBudget.times[0]+60001)}
      if($taskNow -ge $taskWaitUntil){break};Start-Sleep -Milliseconds ([int][Math]::Min(250,$taskWaitUntil-$taskNow))
    }
    if(Test-Path -LiteralPath (Join-Path $taskOutput 'STOP')){break}
    $taskBudget.times=@($taskBudget.times)+@([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
    $taskBudget | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $taskBudgetFile -Encoding utf8
    $taskStarted=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
    try {$taskResponse=Invoke-WebRequest -Uri ('https://sig.thesuper.market/api/v1'+$taskRequest.endpoint) -Headers $taskHeaders -Method Get -TimeoutSec 30 -UseBasicParsing}
    catch {
      $taskStatus=if($_.Exception.Response){[int]$_.Exception.Response.StatusCode}else{0}
      $taskDelay=60
      if($_.Exception.Response.Headers.RetryAfter.Delta){$taskDelay=[Math]::Max(60,$_.Exception.Response.Headers.RetryAfter.Delta.TotalSeconds)}
      $taskBudget.blocked=$true;$taskBudget.nextAt=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()+[long]($taskDelay*1000)
      $taskFailure=[ordered]@{at=[DateTime]::UtcNow.ToString('o');status=$taskStatus;endpoint=$taskRequest.endpoint;minimumBackoffSeconds=$taskDelay;orderWrites=0}
      $taskFailure | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskOutput 'failure.json') -Encoding utf8
      $taskBudget | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $taskBudgetFile -Encoding utf8
      throw "SIG_GET_FAILED_$taskStatus : capture stopped; no blind retry or credential logging."
    }
    $taskReceived=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
    $taskBudget.nextAt=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()+12000
    $taskBudget | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $taskBudgetFile -Encoding utf8
    $taskData=$taskResponse.Content | ConvertFrom-Json -DateKind String
    $taskReceipt=[ordered]@{schemaVersion=1;source='official_sig_api_v1';kind=$taskRequest.kind;endpoint=$taskRequest.endpoint;tournamentId=$taskTournament;marketId=$taskMarket;exchangeId=$taskExchange;requestStartedAt=$taskStarted;receivedAt=$taskReceived;status=[int]$taskResponse.StatusCode;data=$taskData}
    $taskReceipt | ConvertTo-Json -Depth 60 -Compress | Add-Content -LiteralPath (Join-Path $taskOutput 'receipts.jsonl') -Encoding utf8
    if($taskRequest.kind -eq 'history'){$taskReceipt | ConvertTo-Json -Depth 60 | Set-Content -LiteralPath (Join-Path $taskOutput 'history.json') -Encoding utf8}
    $taskReceipts++
    if($taskRequest.kind -eq 'catalog' -and $taskData.pagination.hasMore){
      if(-not $taskData.pagination.nextCursor -or $taskReceipts -ge 10){throw 'CATALOG_PAGINATION_INCOMPLETE'}
      $taskNext='/tournaments/midterm-elections/markets?status=open&limit=100&cursor='+[Uri]::EscapeDataString($taskData.pagination.nextCursor)
      if($taskRequests.endpoint -contains $taskNext){throw 'CATALOG_CURSOR_LOOP'}
      $taskRequests+=@{kind='catalog';endpoint=$taskNext}
    }
    # At most five sequential requests/minute, with completion pacing. No order or token writes.
    if($taskReceipts -lt $taskRequests.Count){Start-Sleep -Seconds 12}
  }
  [pscustomobject]@{Source='official_sig_api_v1';Receipts=$taskReceipts;AccountSharedWithBaal=$true;MaximumLocalReadsPerMinute=5;HumanAuthorizedRecreatedReadCapture=$true;ContestOrders=0;TokenWrites=0;Output=$taskOutput} | ConvertTo-Json
} finally {
  if($taskHeaders){$taskHeaders.Clear()};$taskKey=$null
  [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($taskPointer)
  $taskSecure.Dispose()
  $taskMutex.ReleaseMutex();$taskMutex.Dispose()
}
