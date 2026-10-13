[CmdletBinding()]
param(
    [ValidateRange(4, 50)]
    [int]$SampleCount = 8,
    [string]$OutputPath = (Join-Path $PSScriptRoot '..\research\results\usn-file-token-probe-20261013.json')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\research\usn-file-token-v1.psm1') -Force
$testRunner = Join-Path $PSScriptRoot '..\research\tests\usn-file-token-v1.test.ps1'
$testInfo = [Diagnostics.ProcessStartInfo]::new()
$testInfo.FileName = Join-Path $PSHOME 'pwsh.exe'
$testInfo.ArgumentList.Add('-NoProfile')
$testInfo.ArgumentList.Add('-File')
$testInfo.ArgumentList.Add($testRunner)
$testInfo.UseShellExecute = $false
$testInfo.CreateNoWindow = $true
$testInfo.RedirectStandardOutput = $true
$testInfo.RedirectStandardError = $true
$testProcess = [Diagnostics.Process]::new()
$testProcess.StartInfo = $testInfo
if (-not $testProcess.Start()) { throw 'Focused token test process did not start.' }
$testStdoutTask = $testProcess.StandardOutput.ReadToEndAsync()
$testStderrTask = $testProcess.StandardError.ReadToEndAsync()
$testProcess.WaitForExit()
$testExitCode = $testProcess.ExitCode
$testOutput = $testStdoutTask.GetAwaiter().GetResult().Trim()
$null = $testStderrTask.GetAwaiter().GetResult()
$testSummary = $testOutput | ConvertFrom-Json
$testProcess.Dispose()
if ($testExitCode -ne 0 -or $testSummary.status -ne 'passed' -or $testSummary.failed -ne 0) { throw 'Focused token tests did not pass.' }

function Get-Percentile {
    param([double[]]$Values, [double]$Percentile)
    if ($Values.Count -eq 0) { return $null }
    $ordered = @($Values | Sort-Object)
    $index = [Math]::Max(0, [Math]::Min($ordered.Count - 1, [Math]::Ceiling($Percentile * $ordered.Count) - 1))
    return [Math]::Round([double]$ordered[$index], 3)
}

function Measure-ChildLaunch {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $psi = [Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $env:ComSpec
    $psi.ArgumentList.Add('/d')
    $psi.ArgumentList.Add('/c')
    $psi.ArgumentList.Add('exit 0')
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $psi
    try {
        if (-not $process.Start()) { return $null }
        $process.WaitForExit()
        $timer.Stop()
        if ($process.ExitCode -ne 0) { return $null }
        return [Math]::Round($timer.Elapsed.TotalMilliseconds, 3)
    }
    finally { $process.Dispose() }
}

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('usn-token-probe-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($tempRoot)
$filePath = Join-Path $tempRoot 'throwaway.bin'
[IO.File]::WriteAllBytes($filePath, [byte[]](1, 2, 3, 4, 5, 6, 7, 8))

try {
    $launchMs = [System.Collections.Generic.List[double]]::new()
    for ($i = 0; $i -lt $SampleCount; $i++) {
        $value = Measure-ChildLaunch
        if ($null -ne $value) { $launchMs.Add([double]$value) }
    }

    $snapshots = [System.Collections.Generic.List[object]]::new()
    for ($i = 0; $i -lt $SampleCount; $i++) {
        $snapshots.Add((Get-FileUsnTokenSnapshot -LiteralPath $filePath))
    }
    $supported = @($snapshots | Where-Object supported).Count
    $stable = @($snapshots | Where-Object { $_.supported -and $_.stable }).Count
    $generationMs = @($snapshots | Where-Object { $_.supported -and $null -ne $_.generationQueryMs } | ForEach-Object { [double]$_.generationQueryMs })
    $fileQueryMs = @($snapshots | Where-Object { $_.supported -and $null -ne $_.fileQueryMs } | ForEach-Object { [double]$_.fileQueryMs })
    $totalMs = @($snapshots | ForEach-Object { [double]$_.elapsedMs })
    $reasons = @($snapshots | Group-Object reason | ForEach-Object { [pscustomobject]@{ reason = $_.Name; count = $_.Count } })

    $result = [ordered]@{
        schema = 'usn-file-token-probe-v1'
        completedAtUtc = [DateTime]::UtcNow.ToString('o')
        host = [ordered]@{
            operatingSystem = 'Windows'
            expectedFileSystem = 'NTFS'
            sampleCount = $SampleCount
            supportedSnapshots = $supported
            stableSnapshots = $stable
            temporaryFilesOnly = $true
            runtimeOrAccountFilesRead = $false
            runtimeOrAccountStateChanged = $false
        }
        measurement = [ordered]@{
            processLaunchBaseline = 'cmd.exe immediate-exit child-process wall time; includes process startup and scheduling'
            fsutilQuery = 'two fsutil usn queryjournal child processes per snapshot; includes launch, query, output capture, and parse'
            nativeFileSample = 'two same-handle FSCTL_READ_FILE_USN_DATA samples plus one post-sample path-binding check'
            processLaunchBaselineMedianMs = Get-Percentile -Values ([double[]]$launchMs.ToArray()) -Percentile 0.5
            processLaunchBaselineP95Ms = Get-Percentile -Values ([double[]]$launchMs.ToArray()) -Percentile 0.95
            twoGenerationQueriesPerSnapshotMedianMs = Get-Percentile -Values ([double[]]$generationMs) -Percentile 0.5
            twoGenerationQueriesPerSnapshotP95Ms = Get-Percentile -Values ([double[]]$generationMs) -Percentile 0.95
            twoNativeFileSamplesMedianMs = Get-Percentile -Values ([double[]]$fileQueryMs) -Percentile 0.5
            twoNativeFileSamplesP95Ms = Get-Percentile -Values ([double[]]$fileQueryMs) -Percentile 0.95
            pathBindingSampleMedianMs = Get-Percentile -Values ([double[]]@($snapshots | Where-Object { $_.supported -and $null -ne $_.pathBindingQueryMs } | ForEach-Object { [double]$_.pathBindingQueryMs })) -Percentile 0.5
            fullSnapshotMedianMs = Get-Percentile -Values ([double[]]$totalMs) -Percentile 0.5
            fullSnapshotP95Ms = Get-Percentile -Values ([double[]]$totalMs) -Percentile 0.95
        }
        result = [ordered]@{
            status = if ($stable -eq $SampleCount) { 'supported_and_stable' } else { 'fail_closed_or_unstable' }
            fullHashFallbackOnUnsupportedOrAnyMismatch = $true
            changedUsnProvesAppend = $false
            suffixOnlyRefreshPermitted = $false
            reasons = $reasons
        }
        correctnessTests = [ordered]@{
            status = $testSummary.status
            passed = $testSummary.passed
            failed = $testSummary.failed
            cases = @('unchanged cache', 'append', 'same-size rewrite', 'truncation', 'file replacement', 'journal-generation change', 'denied/unsupported query', 'unknown record version', 'same-handle token race')
        }
        privacy = [ordered]@{
            machineNameOmitted = $true
            userNameOmitted = $true
            fileAndVolumeIdentifiersOmitted = $true
            pathsOmitted = $true
            rawUsnValuesOmitted = $true
            contentAndHashesOmitted = $true
        }
        sources = @(
            'https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_read_file_usn_data',
            'https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-read_file_usn_data',
            'https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-usn_journal_data_v2'
        )
    }

    $json = $result | ConvertTo-Json -Depth 8
    $resolvedOutput = [IO.Path]::GetFullPath($OutputPath)
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($resolvedOutput))
    [IO.File]::WriteAllText($resolvedOutput, $json + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
    [pscustomobject]@{
        status = $result.result.status
        supportedSnapshots = $supported
        stableSnapshots = $stable
        sampleCount = $SampleCount
        launchP50Ms = $result.measurement.processLaunchBaselineMedianMs
        fullSnapshotP50Ms = $result.measurement.fullSnapshotMedianMs
        reportWritten = $true
    } | ConvertTo-Json -Compress
}
finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

