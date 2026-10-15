function Select-MolochExitBook {
  param($Document,[string]$SourceDirectory,$Pairs,$Requests,[int]$Index,[long]$Now,[long]$NextAdmission,$Attempted)
  if($Document.schemaVersion -ne 1 -or [IO.Path]::GetFullPath($Document.sourceDirectory) -ne [IO.Path]::GetFullPath($SourceDirectory)){throw 'EXIT_BOOK_SOURCE_PROOF'}
  $generated=[DateTimeOffset]::Parse($Document.generatedAt).ToUnixTimeMilliseconds()
  if($Now-$generated -gt 30000 -or $generated-$Now -gt 1000){throw 'EXIT_BOOK_SOURCE_AGE'}
  $intents=@($Document.exitBooks)
  if($intents.Count -gt 64){throw 'EXIT_BOOK_COUNT'}
  foreach($intent in ($intents | Sort-Object expiresAt)) {
    $scope=@($Pairs | Where-Object {$_.marketId -eq $intent.marketId -and $_.exchangeId -eq $intent.exchangeId})
    if($scope.Count -ne 1 -or ($intent.activationAt -isnot [long] -and $intent.activationAt -isnot [int]) -or ($intent.expiresAt -isnot [long] -and $intent.expiresAt -isnot [int]) -or $intent.expiresAt -le $intent.activationAt){throw 'EXIT_BOOK_INTENT_PROOF'}
    $key="$($intent.marketId):$($intent.exchangeId):$($intent.activationAt):$($intent.expiresAt)"
    if($Attempted.Contains($key) -or $NextAdmission -lt $intent.activationAt -or $NextAdmission -ge $intent.expiresAt){continue}
    if($Requests[$Index].kind -eq 'book' -or $Requests[$Index].fixedWindow -or $Requests[$Index].dnsRetry){return $null}
    for($slot=$Index+1;$slot -lt $Requests.Count;$slot++){
      $r=$Requests[$slot]
      if($r.kind -eq 'book' -and $r.marketId -eq $intent.marketId -and $r.exchangeId -eq $intent.exchangeId -and -not $r.dnsRetry){return [pscustomobject]@{Index=$slot;Key=$key;MarketId=$intent.marketId;ExchangeId=$intent.exchangeId}}
    }
  }
  return $null
}
Export-ModuleMember -Function Select-MolochExitBook
