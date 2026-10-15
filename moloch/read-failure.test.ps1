$ErrorActionPreference='Stop'
Import-Module (Join-Path $PSScriptRoot 'read-failure.psm1') -Force
$socket=[System.Net.Sockets.SocketException]::new([int][System.Net.Sockets.SocketError]::HostNotFound)
$wrapped=[System.Net.Http.HttpRequestException]::new('SECRET_SENTINEL_DO_NOT_LOG',$socket)
$dns=Get-MolochReadFailure -Exception $wrapped
if($dns.category -ne 'transport_dns' -or -not $dns.typedTransportFailure){throw 'DNS_CLASSIFICATION'}
$timeout=Get-MolochReadFailure -Exception ([System.TimeoutException]::new('SECRET_SENTINEL_DO_NOT_LOG'))
if($timeout.category -ne 'transport_timeout'){throw 'TIMEOUT_CLASSIFICATION'}
$io=Get-MolochReadFailure -Exception ([System.IO.IOException]::new('SECRET_SENTINEL_DO_NOT_LOG'))
if($io.typedTransportFailure -or $io.category -ne 'unclassified'){throw 'DURABLE_FAILURE_MUST_NOT_BECOME_RECOVERABLE'}
if((@($dns,$timeout,$io) | ConvertTo-Json) -match 'SECRET_SENTINEL_DO_NOT_LOG'){throw 'MESSAGE_LEAK'}
$cases=@(
  @{enabled=$true;used=$false;method='Get';status=0;transport=$dns;expected=$true},
  @{enabled=$false;used=$false;method='Get';status=0;transport=$dns;expected=$false},
  @{enabled=$true;used=$true;method='Get';status=0;transport=$dns;expected=$false},
  @{enabled=$true;used=$false;method='Post';status=0;transport=$dns;expected=$false},
  @{enabled=$true;used=$false;method='Get';status=429;transport=$dns;expected=$false},
  @{enabled=$true;used=$false;method='Get';status=0;transport=$timeout;expected=$false}
)
foreach($case in $cases){$actual=Test-MolochDnsReadRecovery -Enabled $case.enabled -Used $case.used -Method $case.method -Status $case.status -Transport $case.transport;if($actual -ne $case.expected){throw 'DNS_READ_RECOVERY_GUARD'}}
Write-Output '10 classified failure, secret-omission and bounded DNS guard checks passed'
