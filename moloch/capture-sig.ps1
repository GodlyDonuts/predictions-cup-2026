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
  [switch]$DiagnosticTradesProbe,
  [string]$DiagnosticTradesFrom,
  [string]$DiagnosticTradesTo,
  [switch]$CatalogOnly,
  [switch]$LeaderboardOnly,
  [switch]$ExecutionHistoryOnly,
  [string]$ScreenPairsFile,
  [switch]$RealtimeCapture,
  [string]$UniversePairsFile
)
$ErrorActionPreference='Stop'
Import-Module (Join-Path $PSScriptRoot 'read-failure.psm1') -Force
if(-not $HumanAuthorizedSharedReadCapture){throw 'EXPLICIT_SHARED_READ_AUTHORIZATION_REQUIRED: preserve the prior one-collector policy unless human explicitly authorizes recreating read-only capture.'}
if($DiagnosticTradesProbe -and ($DiagnosticBookProbe -or $HistoryOnly -or $CatalogOnly -or $LeaderboardOnly -or $ExecutionHistoryOnly -or $ScreenPairsFile -or $RealtimeCapture -or $UniversePairsFile)){throw 'ONE_DIAGNOSTIC_MODE_REQUIRED'}
if($DiagnosticTradesFrom -or $DiagnosticTradesTo){
  if(-not $DiagnosticTradesProbe -or -not $DiagnosticTradesFrom -or -not $DiagnosticTradesTo){throw 'DIAGNOSTIC_WINDOW_REQUIRES_ONE_TRADE_PROBE'}
  $taskDiagnosticFrom=[DateTimeOffset]::Parse($DiagnosticTradesFrom);$taskDiagnosticTo=[DateTimeOffset]::Parse($DiagnosticTradesTo)
  if($taskDiagnosticFrom -ge $taskDiagnosticTo -or $taskDiagnosticTo -gt [DateTimeOffset]::UtcNow){throw 'DIAGNOSTIC_WINDOW_INVALID'}
}
if($ExecutionHistoryOnly -and ($LeaderboardOnly -or $CatalogOnly -or $ScreenPairsFile -or $RealtimeCapture -or $HistoryOnly -or $DiagnosticBookProbe)){throw 'ONE_DISCOVERY_MODE_REQUIRED'}
if($LeaderboardOnly -and ($CatalogOnly -or $ScreenPairsFile -or $RealtimeCapture)){throw 'ONE_DISCOVERY_MODE_REQUIRED'}
if($CatalogOnly -and $ScreenPairsFile){throw 'ONE_DISCOVERY_MODE_REQUIRED'}
if(-not $CatalogOnly -and -not $LeaderboardOnly -and -not $ExecutionHistoryOnly -and -not $ScreenPairsFile -and (-not $MarketId -or -not $ExchangeId)){throw 'MARKET_AND_EXCHANGE_REQUIRED'}
if($RealtimeCapture -and (-not $ScreenPairsFile -or $Cycles -gt 30)){throw 'STREAM_REQUIRES_SCOPED_PAIRS_AND_BOUNDED_CYCLES'}
if($UniversePairsFile -and -not $RealtimeCapture){throw 'UNIVERSE_REQUIRES_REALTIME_CAPTURE'}
if($UniversePairsFile){
  $taskUniverse=@(Get-Content -LiteralPath $UniversePairsFile -Raw | ConvertFrom-Json -DateKind String)
  if($taskUniverse.Count -lt 1 -or $taskUniverse.Count -gt 1000){throw 'UNIVERSE_SCOPE_INVALID'}
  foreach($taskUniversePair in $taskUniverse){if([string]$taskUniversePair.marketId -notmatch '^[1-9][0-9]*$' -or [string]$taskUniversePair.exchangeId -notmatch '^[1-9][0-9]*$'){throw 'UNIVERSE_ID_INVALID'}}
  if(@($taskUniverse | Select-Object exchangeId -Unique).Count -ne $taskUniverse.Count){throw 'UNIVERSE_DUPLICATE_EXCHANGE'}
}
$taskOutput=[IO.Path]::GetFullPath($OutputDirectory)
if(Test-Path -LiteralPath $taskOutput){throw 'OUTPUT_EXISTS: preserve previous capture; choose a new directory.'}
New-Item -ItemType Directory -Path $taskOutput | Out-Null
$taskMutex=[Threading.Mutex]::new($false,'Local\Moloch-SIG-Read-Capture')
if(-not $taskMutex.WaitOne(0)){ $taskMutex.Dispose();throw 'EXISTING_MOLOCH_CAPTURE_ACTIVE' }
$taskBudgetDirectory=Join-Path $env:LOCALAPPDATA 'Moloch'
New-Item -ItemType Directory -Path $taskBudgetDirectory -Force | Out-Null
$taskBudgetFile=Join-Path $taskBudgetDirectory 'sig-read-budget.json'
$taskBudget=if(Test-Path -LiteralPath $taskBudgetFile){Get-Content -LiteralPath $taskBudgetFile -Raw | ConvertFrom-Json}else{[pscustomobject]@{times=@();nextAt=0;blocked=$false}}
if($taskBudget.blocked -and -not ($DiagnosticBookProbe -or $DiagnosticTradesProbe)){$taskMutex.ReleaseMutex();$taskMutex.Dispose();throw 'SIG_LOCAL_BUDGET_BLOCKED: previous request failed; inspect evidence before another run.'}
$taskTournament='bda92870-621e-47b0-bc3c-3602c5c26f55'
$taskCredential=Join-Path $env:LOCALAPPDATA 'Codex\secrets\sig-predictions-cup-2026.dpapi'
$taskSecure=Get-Content -LiteralPath $taskCredential -Raw | ConvertTo-SecureString
$taskPointer=[Runtime.InteropServices.Marshal]::SecureStringToBSTR($taskSecure)
$taskKey=$null
$taskStream=$null
$taskTokenWrites=0
try {
  $taskKey=[Runtime.InteropServices.Marshal]::PtrToStringBSTR($taskPointer)
  if($taskKey -notmatch '^ace_[a-zA-Z0-9]+$'){throw 'APPROVED_CREDENTIAL_FORMAT'}
  $taskHeaders=@{Authorization=('Bearer '+$taskKey)}
  $taskEnd=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:00:00.000Z')
  $taskFrom=if($AllHistory){'1970-01-01T00:00:00.000Z'}else{[DateTime]::UtcNow.AddDays(-$HistoryDays).ToString('yyyy-MM-ddTHH:00:00.000Z')}
  $taskTapeFrom=[DateTime]::UtcNow.AddMinutes(-15).ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
  $taskTapeTo=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
  if($DiagnosticTradesFrom){$taskTapeFrom=$taskDiagnosticFrom.UtcDateTime.ToString('yyyy-MM-ddTHH:mm:ss.fffZ');$taskTapeTo=$taskDiagnosticTo.UtcDateTime.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')}
  $taskRequests=@(
    @{kind='book';endpoint="/markets/$MarketId/orderbook?tournamentId=$taskTournament&depth=200"},
    @{kind='trades';endpoint="/exchanges/$ExchangeId/trades?tournamentId=$taskTournament&from=$([Uri]::EscapeDataString($taskTapeFrom))&to=$([Uri]::EscapeDataString($taskTapeTo))&limit=200"},
    @{kind='history';endpoint="/exchanges/$ExchangeId/price-history?tournamentId=$taskTournament&resolution=1h&from=$([Uri]::EscapeDataString($taskFrom))&to=$([Uri]::EscapeDataString($taskEnd))&limit=1000"}
  )
  if($DiagnosticTradesProbe){$taskRequests=@($taskRequests | Where-Object kind -eq 'trades');if($DiagnosticTradesFrom){$taskRequests[0].fixedWindow=$true}}elseif($DiagnosticBookProbe){$taskRequests=@($taskRequests | Where-Object kind -eq 'book')}elseif($HistoryOnly){$taskRequests=@($taskRequests | Where-Object kind -eq 'history')}elseif($Cycles -gt 1){$taskRequests=@($taskRequests)+@(for($taskCycle=1;$taskCycle -lt $Cycles;$taskCycle++){$taskRequests[0];$taskRequests[1]})}
  if($ExecutionHistoryOnly){$taskRequests=@(@{kind='private_fills';endpoint="/portfolio/fills?tournamentId=$taskTournament&limit=100"})}
  if($LeaderboardOnly){$taskRequests=@(@{kind='leaderboard';endpoint='/tournaments/midterm-elections/leaderboard?period=all&sort=pnl&limit=10'},@{kind='leaderboard';endpoint='/tournaments/midterm-elections/leaderboard?period=1d&sort=pnl&limit=10'})}
  if($CatalogOnly){$taskRequests=@(@{kind='catalog';endpoint='/tournaments/midterm-elections/markets?status=open&limit=100'})}
  if($ScreenPairsFile){
    $taskPairs=@(Get-Content -LiteralPath $ScreenPairsFile -Raw | ConvertFrom-Json -DateKind String)
    if($UniversePairsFile){foreach($taskPair in $taskPairs){if(-not @($taskUniverse | Where-Object {$_.marketId -eq $taskPair.marketId -and $_.exchangeId -eq $taskPair.exchangeId}).Count){throw 'REST_SCOPE_OUTSIDE_UNIVERSE'}}}
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
        # Establish the post-end authoritative book barrier BEFORE its tape
        # coverage. A newer pushed book must not strand historical reservations.
        @{kind='book';marketId=[string]$taskPair.marketId;exchangeId=[string]$taskPair.exchangeId;endpoint="/markets/$($taskPair.marketId)/orderbook?tournamentId=$taskTournament&depth=200"}
        @{kind='trades';fixedWindow=$true;marketId=[string]$taskPair.marketId;exchangeId=[string]$taskPair.exchangeId;endpoint="/exchanges/$($taskPair.exchangeId)/trades?tournamentId=$taskTournament&from=$([Uri]::EscapeDataString($taskRecoveryFromText))&to=$([Uri]::EscapeDataString($taskRecoveryToText))&limit=200"}
      }
    })
    $taskRequests=$taskRecovery+@($taskRequests)
  }
  if($RealtimeCapture){
    if($taskPairs.Count -gt 4){throw 'STREAM_MAXIMUM_FOUR_MARKETS'}
    if($Cycles -gt [Math]::Floor(60/$taskPairs.Count)){throw 'STREAM_DURATION_LIMIT'}
    $taskRequests=@(@{kind='realtime';endpoint='/realtime/token';method='Post'})+@($taskRequests)
  }
  $taskReceipts=0
  for($taskIndex=0;$taskIndex -lt $taskRequests.Count;$taskIndex++){
    if($ScreenPairsFile -and $taskReceipts -ge 120){break}
    $taskRequest=$taskRequests[$taskIndex]
    if($taskStream -and $taskStream.HasExited){throw 'STREAM_PROCESS_EXITED: stop rather than silently downgrade or refresh credentials.'}
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
      if($taskRequest.kind -eq 'realtime' -and $taskBudget.PSObject.Properties.Name -contains 'nextAuthAt'){$taskWaitUntil=[Math]::Max($taskWaitUntil,[long]$taskBudget.nextAuthAt)}
      if($taskBudget.times.Count -ge 5){$taskWaitUntil=[Math]::Max($taskWaitUntil,[long]$taskBudget.times[0]+60001)}
      if($taskNow -ge $taskWaitUntil){break};Start-Sleep -Milliseconds ([int][Math]::Min(250,$taskWaitUntil-$taskNow))
    }
    if(Test-Path -LiteralPath (Join-Path $taskOutput 'STOP')){break}
    $taskBudget.times=@($taskBudget.times)+@([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
    if($taskRequest.kind -eq 'realtime'){$taskBudget | Add-Member -NotePropertyName nextAuthAt -NotePropertyValue ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()+60000) -Force}
    $taskBudget | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $taskBudgetFile -Encoding utf8
    $taskStarted=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
    $taskMethod=if($taskRequest.method){$taskRequest.method}else{'Get'}
    try {$taskResponse=Invoke-WebRequest -Uri ('https://sig.thesuper.market/api/v1'+$taskRequest.endpoint) -Headers $taskHeaders -Method $taskMethod -TimeoutSec 30 -UseBasicParsing}
    catch {
      $taskStatus=if($_.Exception.Response){[int]$_.Exception.Response.StatusCode}else{0}
      $taskDelay=60
      if($_.Exception.Response.Headers.RetryAfter.Delta){$taskDelay=[Math]::Max(60,$_.Exception.Response.Headers.RetryAfter.Delta.TotalSeconds)}
      $taskBudget.blocked=$true;$taskBudget.nextAt=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()+[long]($taskDelay*1000)
      $taskFailure=[ordered]@{at=[DateTime]::UtcNow.ToString('o');status=$taskStatus;endpoint=$taskRequest.endpoint;method=$taskMethod;minimumBackoffSeconds=$taskDelay;orderWrites=0;transport=Get-MolochReadFailure -Exception $_.Exception}
      $taskFailure | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskOutput 'failure.json') -Encoding utf8
      $taskBudget | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $taskBudgetFile -Encoding utf8
      throw "SIG_GET_FAILED_$taskStatus : capture stopped; no blind retry or credential logging."
    }
    $taskReceived=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
    $taskBudget.nextAt=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()+12000
    $taskBudget | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $taskBudgetFile -Encoding utf8
    try{$taskData=$taskResponse.Content | ConvertFrom-Json -DateKind String}catch{throw 'SIG_RESPONSE_JSON_INVALID: raw response omitted from diagnostic output.'}
    if($taskRequest.kind -eq 'realtime'){
      $taskTokenWrites++
      $taskStartInfo=[Diagnostics.ProcessStartInfo]::new()
      $taskStartInfo.FileName=(Get-Command node).Source
      $taskStartInfo.ArgumentList.Add((Join-Path $PSScriptRoot 'realtime/capture.mjs'))
      $taskStartInfo.UseShellExecute=$false;$taskStartInfo.CreateNoWindow=$true
      $taskStartInfo.RedirectStandardInput=$true;$taskStartInfo.RedirectStandardOutput=$true;$taskStartInfo.RedirectStandardError=$true
      $taskStream=[Diagnostics.Process]::new();$taskStream.StartInfo=$taskStartInfo
      if(-not $taskStream.Start()){throw 'STREAM_START_FAILED'}
      $taskStreamOutput=$taskStream.StandardOutput.ReadToEndAsync();$taskStreamErrors=$taskStream.StandardError.ReadToEndAsync()
      $taskInit=@{auth=$taskData;outputDirectory=(Join-Path $taskOutput 'stream');tournamentId=$taskTournament;pairs=@($taskPairs | Select-Object marketId,exchangeId)}
      if($UniversePairsFile){$taskInit.pairs=$taskUniverse;$taskInit.aggregate=$true}
      $taskStream.StandardInput.WriteLine(($taskInit | ConvertTo-Json -Depth 20 -Compress));$taskStream.StandardInput.Flush()
      # This authentication response is never added to receipts, stdout, files or environment variables.
      $taskInit=$null;$taskData=$null;$taskResponse=$null
      Start-Sleep -Seconds 12
      continue
    }
    if($taskRequest.kind -eq 'private_fills'){
      $taskFillOrders=@($taskData.data | Where-Object {$null -ne $_.orderId} | Select-Object orderId -Unique | Select-Object -First 3)
      foreach($taskFillOrder in $taskFillOrders){if([string]$taskFillOrder.orderId -match '^[1-9][0-9]*$'){$taskRequests+=@{kind='private_order';endpoint="/orders/$($taskFillOrder.orderId)"}}}
      $taskData=[ordered]@{data=@($taskData.data | Select-Object id,orderId,exchangeId,marketId,price,quantity,side,filledAt);pagination=$taskData.pagination;coverage=$taskData.coverage;identitiesOmitted=$true;singlePageOnly=$true}
    }elseif($taskRequest.kind -eq 'private_order'){$taskData=$taskData | Select-Object id,exchangeId,marketId,tournamentId,side,action,price,quantity,filledQuantity,remainingQuantity,createdAt,expirationDate,open,status}
    if($taskRequest.kind -eq 'leaderboard'){
      $taskBoard=if($taskData.PSObject.Properties.Name -contains 'leaderboard'){$taskData}elseif($taskData.data -and $taskData.data.PSObject.Properties.Name -contains 'leaderboard'){$taskData.data}else{throw 'LEADERBOARD_SCHEMA_INVALID'}
      $taskData=[ordered]@{period=$taskBoard.period;sort=$taskBoard.sort;total=$taskBoard.total;myRank=$taskBoard.myRank;limit=$taskBoard.limit;offset=$taskBoard.offset;leaderboard=@($taskBoard.leaderboard | Select-Object rank,pnl,tradesCount,volume,winRate,roi);identitiesOmitted=$true;originalResponseSha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($taskResponse.Content))).ToLowerInvariant()}
    }
    $taskReceipt=[ordered]@{schemaVersion=1;source='official_sig_api_v1';kind=$taskRequest.kind;endpoint=$taskRequest.endpoint;tournamentId=$taskTournament;marketId=$taskMarket;exchangeId=$taskExchange;requestStartedAt=$taskStarted;receivedAt=$taskReceived;status=[int]$taskResponse.StatusCode;data=$taskData}
    $taskReceipt | ConvertTo-Json -Depth 60 -Compress | Add-Content -LiteralPath (Join-Path $taskOutput 'receipts.jsonl') -Encoding utf8
    if($taskRequest.kind -eq 'history'){$taskReceipt | ConvertTo-Json -Depth 60 | Set-Content -LiteralPath (Join-Path $taskOutput 'history.json') -Encoding utf8}
    $taskReceipts++
    if($taskRequest.kind -eq 'trades' -and $taskData.pagination.hasMore -and [int]$taskRequest.pageCount -lt 5){
      if(-not $taskData.pagination.nextCursor){throw 'TAPE_PAGINATION_CURSOR_MISSING'}
      $taskTapeBase=$taskRequest.endpoint.Split('&cursor=')[0]
      $taskNext=$taskTapeBase+'&cursor='+[Uri]::EscapeDataString($taskData.pagination.nextCursor)
      if($taskRequests.endpoint -contains $taskNext){throw 'TAPE_CURSOR_LOOP'}
      $taskPageCount=if($taskRequest.pageCount){[int]$taskRequest.pageCount+1}else{2}
      $taskNextRequest=@{kind='trades';fixedWindow=$true;pageCount=$taskPageCount;marketId=$taskMarket;exchangeId=$taskExchange;endpoint=$taskNext}
      $taskRemaining=if($taskIndex+1 -lt $taskRequests.Count){@($taskRequests[($taskIndex+1)..($taskRequests.Count-1)])}else{@()}
      $taskRequests=@($taskRequests[0..$taskIndex])+@($taskNextRequest)+$taskRemaining
    }
    if($taskRequest.kind -eq 'catalog' -and $taskData.pagination.hasMore){
      if(-not $taskData.pagination.nextCursor -or $taskReceipts -ge 10){throw 'CATALOG_PAGINATION_INCOMPLETE'}
      $taskNext='/tournaments/midterm-elections/markets?status=open&limit=100&cursor='+[Uri]::EscapeDataString($taskData.pagination.nextCursor)
      if($taskRequests.endpoint -contains $taskNext){throw 'CATALOG_CURSOR_LOOP'}
      $taskRequests+=@{kind='catalog';endpoint=$taskNext}
    }
    # At most five sequential reads/minute; optional one-shot auth is separately recorded. No orders.
    if($taskReceipts -lt $taskRequests.Count){Start-Sleep -Seconds 12}
  }
  if($taskStream){
    if(-not $taskStream.HasExited){$taskStream.StandardInput.WriteLine('{"stop":true}');$taskStream.StandardInput.Flush();if(-not $taskStream.WaitForExit(5000)){throw 'STREAM_STOP_TIMEOUT'}}
    if($taskStream.ExitCode -ne 0){throw 'STREAM_CAPTURE_FAILED: inspect sanitized stream status; no automatic token refresh.'}
  }
  [pscustomobject]@{Source='official_sig_api_v1';Receipts=$taskReceipts;AccountSharedWithBaal=$true;MaximumLocalReadsPerMinute=5;MaximumLocalAuthWritesPerMinute=1;HumanAuthorizedRecreatedReadCapture=$true;ContestOrders=0;TokenWrites=$taskTokenWrites;RealtimeCapture=[bool]$RealtimeCapture;Output=$taskOutput} | ConvertTo-Json
} finally {
  if($taskStream){
    if(-not $taskStream.HasExited){try{$taskStream.StandardInput.WriteLine('{"stop":true}');$taskStream.StandardInput.Flush()}catch{};if(-not $taskStream.WaitForExit(5000)){$taskStream.Kill();$taskStream.WaitForExit(5000)|Out-Null}}
    $taskStream.Dispose()
  }
  if($taskHeaders){$taskHeaders.Clear()};$taskKey=$null
  [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($taskPointer)
  $taskSecure.Dispose()
  $taskMutex.ReleaseMutex();$taskMutex.Dispose()
}
