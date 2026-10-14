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
Write-Output '4 classified failure and secret-omission checks passed'
