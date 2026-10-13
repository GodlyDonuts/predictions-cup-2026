[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\usn-file-token-v1.psm1') -Force

$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('usn-token-test-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($testRoot)
$passed = 0
$failures = [System.Collections.Generic.List[string]]::new()

function Assert-Test {
    param([bool]$Condition, [string]$Name)
    if ($Condition) { $script:passed++; return }
    $script:failures.Add($Name)
}

function New-TestFile {
    param([string]$Name, [byte[]]$Bytes)
    $path = Join-Path $testRoot $Name
    [IO.File]::WriteAllBytes($path, $Bytes)
    return $path
}

function Write-TestBytes {
    param([string]$Path, [long]$Offset, [byte[]]$Bytes)
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Write,
        [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
    try {
        $stream.Position = $Offset
        $stream.Write($Bytes, 0, $Bytes.Length)
        $stream.Flush($true)
    }
    finally { $stream.Dispose() }
}

$stableGeneration = {
    param($volumeGuid)
    [pscustomobject]@{
        supported = $true
        journalId = '0x0000000000000001'
        firstUsn = '0x0000000000000010'
        lowestValidUsn = '0x0000000000000001'
        queryElapsedMs = 0.01
    }
}

try {
    $path = New-TestFile 'append.bin' ([byte[]](1, 2, 3, 4, 5, 6, 7, 8))
    $baseline = Get-FileUsnTokenSnapshot -LiteralPath $path -JournalGenerationReader $stableGeneration
    $unchanged = Get-FileUsnTokenSnapshot -LiteralPath $path -JournalGenerationReader $stableGeneration
    Assert-Test ($baseline.supported -and $baseline.stable) 'native same-handle NTFS token sampling is supported and stable'
    Assert-Test (Test-UsnCacheReusable -Cached $baseline -Current $unchanged) 'identical stable token permits cache reuse'

    $beforeAppend = Get-FileUsnTokenSnapshot -LiteralPath $path -JournalGenerationReader $stableGeneration
    $appendStream = [IO.File]::Open($path, [IO.FileMode]::Append, [IO.FileAccess]::Write,
        [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
    try { $appendStream.Write([byte[]](9, 10), 0, 2); $appendStream.Flush($true) } finally { $appendStream.Dispose() }
    $afterAppend = Get-FileUsnTokenSnapshot -LiteralPath $path -JournalGenerationReader $stableGeneration
    Assert-Test (-not (Test-UsnCacheReusable -Cached $beforeAppend -Current $afterAppend)) 'append invalidates cache'
    Assert-Test ($afterAppend.file.length -gt $beforeAppend.file.length) 'append is visible through the opened file identity/length sample'
    Assert-Test ($afterAppend.file.usn -ne $beforeAppend.file.usn) 'append advances the per-file USN'

    $rewritePath = New-TestFile 'rewrite.bin' ([byte[]](10, 20, 30, 40, 50, 60, 70, 80))
    $beforeRewrite = Get-FileUsnTokenSnapshot -LiteralPath $rewritePath -JournalGenerationReader $stableGeneration
    Write-TestBytes -Path $rewritePath -Offset 0 -Bytes ([byte[]](11, 21, 31, 41, 51, 61, 71, 81))
    $afterRewrite = Get-FileUsnTokenSnapshot -LiteralPath $rewritePath -JournalGenerationReader $stableGeneration
    Assert-Test ($beforeRewrite.file.length -eq $afterRewrite.file.length) 'same-size rewrite preserves length'
    Assert-Test ($beforeRewrite.file.usn -ne $afterRewrite.file.usn) 'same-size rewrite advances the per-file USN'
    Assert-Test (-not (Test-UsnCacheReusable -Cached $beforeRewrite -Current $afterRewrite)) 'same-size rewrite invalidates cache'

    $truncatePath = New-TestFile 'truncate.bin' ([byte[]](1..32))
    $beforeTruncate = Get-FileUsnTokenSnapshot -LiteralPath $truncatePath -JournalGenerationReader $stableGeneration
    $stream = [IO.File]::Open($truncatePath, [IO.FileMode]::Open, [IO.FileAccess]::Write,
        [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
    try { $stream.SetLength(8); $stream.Flush($true) } finally { $stream.Dispose() }
    $afterTruncate = Get-FileUsnTokenSnapshot -LiteralPath $truncatePath -JournalGenerationReader $stableGeneration
    Assert-Test (-not (Test-UsnCacheReusable -Cached $beforeTruncate -Current $afterTruncate)) 'truncation invalidates cache'

    $replacePath = New-TestFile 'replace.bin' ([byte[]](1, 3, 5, 7))
    $beforeReplace = Get-FileUsnTokenSnapshot -LiteralPath $replacePath -JournalGenerationReader $stableGeneration
    $replacement = New-TestFile 'replacement.bin' ([byte[]](1, 3, 5, 7))
    [IO.File]::Move($replacement, $replacePath, $true)
    $afterReplace = Get-FileUsnTokenSnapshot -LiteralPath $replacePath -JournalGenerationReader $stableGeneration
    Assert-Test (-not (Test-UsnCacheReusable -Cached $beforeReplace -Current $afterReplace)) 'path replacement invalidates cache even when bytes and length match'
    Assert-Test ($beforeReplace.file.fileId -ne $afterReplace.file.fileId) 'path replacement changes stable file identity'

    $changedGeneration = [pscustomobject]@{
        schema = 'usn-file-token-v1'; supported = $true; stable = $true
        file = $baseline.file
        journal = [pscustomobject]@{
            journalId = '0x0000000000000002'
            firstUsn = $baseline.journal.firstUsn
            lowestValidUsn = $baseline.journal.lowestValidUsn
        }
    }
    Assert-Test (-not (Test-UsnCacheReusable -Cached $baseline -Current $changedGeneration)) 'journal generation change invalidates cache'

    $deniedPath = New-TestFile 'denied.bin' ([byte[]](1, 2, 3))
    $deniedGeneration = { param($volumeGuid) throw [UnauthorizedAccessException]::new('simulated denial') }
    $denied = Get-FileUsnTokenSnapshot -LiteralPath $deniedPath -JournalGenerationReader $deniedGeneration
    Assert-Test (-not $denied.supported -and -not $denied.stable) 'denied generation query fails closed'
    Assert-Test (-not (Test-UsnCacheReusable -Cached $baseline -Current $denied)) 'unsupported/denied token cannot reuse a cache'

    $unknown = [pscustomobject]@{
        schema = 'usn-file-token-v1'; supported = $true; stable = $true
        file = [pscustomobject]@{
            volumeGuid = $baseline.file.volumeGuid; fileSystem = 'NTFS'; volumeSerial = $baseline.file.volumeSerial
            fileId = $baseline.file.fileId; length = 8; majorVersion = 99; minorVersion = 0; usn = $baseline.file.usn
        }
        journal = $baseline.journal
    }
    Assert-Test (-not (Test-UsnCacheReusable -Cached $baseline -Current $unknown)) 'unknown USN record version fails closed'

    $racePath = New-TestFile 'race.bin' ([byte[]](1, 2, 3, 4))
    $between = {
        Write-TestBytes -Path $racePath -Offset 0 -Bytes ([byte[]](5, 6, 7, 8))
    }.GetNewClosure()
    $raced = Get-FileUsnTokenSnapshot -LiteralPath $racePath -JournalGenerationReader $stableGeneration -BetweenSamples $between
    Assert-Test ($raced.supported -and -not $raced.stable -and $raced.reason -eq 'token_changed_during_sampling') ('same-handle token mutation during sampling is rejected (supported=' + $raced.supported + ', stable=' + $raced.stable + ', reason=' + $raced.reason + ')')

    if ($failures.Count -gt 0) {
        throw ('FAIL: ' + ($failures -join '; '))
    }
    [pscustomobject]@{ status = 'passed'; passed = $passed; failed = 0 } | ConvertTo-Json -Compress
}
finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force -ErrorAction SilentlyContinue
}

