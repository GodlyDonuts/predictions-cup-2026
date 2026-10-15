$ErrorActionPreference='Stop'
Import-Module (Join-Path $PSScriptRoot 'exit-book-priority.psm1') -Force
$root=[IO.Path]::GetFullPath($PSScriptRoot)
$doc=[pscustomobject]@{schemaVersion=1;sourceDirectory=$root;generatedAt='1970-01-01T00:00:10.000Z';exitBooks=@([pscustomobject]@{marketId='1';exchangeId='2';activationAt=8000;expiresAt=13000})}
$pairs=@([pscustomobject]@{marketId='1';exchangeId='2'})
$requests=@(@{kind='trades';marketId='1';exchangeId='2'},@{kind='book';marketId='1';exchangeId='2'})
$attempted=[Collections.Generic.HashSet[string]]::new()
$r=Select-MolochExitBook $doc $root $pairs $requests 0 10000 12000 $attempted
if($r.Index -ne 1){throw 'NO_PROMOTION'}
$attempted.Add($r.Key)|Out-Null
if($null -ne (Select-MolochExitBook $doc $root $pairs $requests 0 10000 12000 $attempted)){throw 'DUPLICATE_PROMOTION'}
$attempted.Clear()
if($null -ne (Select-MolochExitBook $doc $root $pairs $requests 0 10000 13000 $attempted)){throw 'DEADLINE_NOT_RESPECTED'}
$requests[0].fixedWindow=$true
if($null -ne (Select-MolochExitBook $doc $root $pairs $requests 0 10000 12000 $attempted)){throw 'CURSOR_PACKET_REORDERED'}
$requests[0].Remove('fixedWindow')
$doc.exitBooks[0].marketId='3'
$rejected=$false;try{Select-MolochExitBook $doc $root $pairs $requests 0 10000 12000 $attempted}catch{$rejected=$true}
if(-not $rejected){throw 'FOREIGN_SCOPE_ACCEPTED'}
$doc.exitBooks[0].marketId='1'
$rejected=$false;try{Select-MolochExitBook $doc $root $pairs $requests 0 50000 50000 $attempted}catch{$rejected=$true}
if(-not $rejected){throw 'STALE_SOURCE_ACCEPTED'}
'6 exit-book priority checks passed'
