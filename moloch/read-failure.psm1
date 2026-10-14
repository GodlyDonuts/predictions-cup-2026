function Get-MolochReadFailure {
  param([Parameter(Mandatory=$true)][System.Exception]$Exception)
  $taskRootType=$Exception.GetType().FullName
  $taskCategory='unclassified';$taskCode=$null;$taskCurrent=$Exception
  for($taskDepth=0;$taskCurrent -and $taskDepth -lt 8;$taskDepth++){
    if($taskCurrent -is [System.TimeoutException] -or $taskCurrent -is [System.Threading.Tasks.TaskCanceledException]){$taskCategory='transport_timeout';$taskCode='Timeout';break}
    if($taskCurrent -is [System.Net.Sockets.SocketException]){
      $taskSocketCode=$taskCurrent.SocketErrorCode.ToString()
      if($taskSocketCode -in @('HostNotFound','TryAgain','NoData')){$taskCategory='transport_dns';$taskCode=$taskSocketCode;break}
      if($taskSocketCode -in @('TimedOut','ConnectionReset','ConnectionAborted','NetworkDown','NetworkUnreachable','HostDown','HostUnreachable')){$taskCategory='transport_connection';$taskCode=$taskSocketCode;break}
    }
    if($taskCurrent -is [System.Net.Http.HttpRequestException] -and $taskCurrent.PSObject.Properties.Name -contains 'HttpRequestError'){
      $taskHttpCode=$taskCurrent.HttpRequestError.ToString()
      if($taskHttpCode -eq 'NameResolutionError'){$taskCategory='transport_dns';$taskCode=$taskHttpCode;break}
      if($taskHttpCode -in @('ConnectionError','ResponseEnded','InvalidResponse')){$taskCategory='transport_connection';$taskCode=$taskHttpCode;break}
    }
    if($taskCurrent -is [System.Net.WebException]){
      $taskWebCode=$taskCurrent.Status.ToString()
      if($taskWebCode -in @('NameResolutionFailure','ProxyNameResolutionFailure')){$taskCategory='transport_dns';$taskCode=$taskWebCode;break}
      if($taskWebCode -in @('ConnectFailure','ReceiveFailure','SendFailure','Timeout','ConnectionClosed','KeepAliveFailure')){$taskCategory='transport_connection';$taskCode=$taskWebCode;break}
    }
    $taskCurrent=$taskCurrent.InnerException
  }
  [ordered]@{category=$taskCategory;exceptionType=if($taskRootType -match '^(System|Microsoft\.PowerShell)\.[A-Za-z0-9_.]+$'){$taskRootType}else{'Other'};transportCode=$taskCode;typedTransportFailure=($taskCategory -ne 'unclassified')}
}
function Test-MolochDnsReadRecovery {
  param([bool]$Enabled,[bool]$Used,[string]$Method,[int]$Status,$Transport)
  $Enabled -and -not $Used -and $Method -eq 'Get' -and $Status -eq 0 -and $Transport.category -eq 'transport_dns' -and $Transport.typedTransportFailure
}
Export-ModuleMember -Function Get-MolochReadFailure,Test-MolochDnsReadRecovery
