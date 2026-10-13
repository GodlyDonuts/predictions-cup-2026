Set-StrictMode -Version Latest

$script:UsnFileTokenNativeSource = @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

public static class UsnFileTokenNativeV1
{
    private const uint GenericRead = 0x80000000;
    private const uint ShareRead = 0x00000001;
    private const uint ShareWrite = 0x00000002;
    private const uint ShareDelete = 0x00000004;
    private const uint OpenExisting = 3;
    private const uint FsctlReadFileUsnData = 0x000900EB;
    private const int FileIdInfoClass = 18;
    private const uint VolumeNameGuid = 0x00000001;

    [StructLayout(LayoutKind.Sequential)]
    private struct FileIdInfo
    {
        public ulong VolumeSerialNumber;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 16)]
        public byte[] FileId;
    }

    public sealed class FileSample
    {
        public string VolumeGuid;
        public string FileSystem;
        public ulong VolumeSerial;
        public string FileId;
        public ulong Length;
        public int MajorVersion;
        public int MinorVersion;
        public string Usn;
        public double ElapsedMs;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true, EntryPoint = "CreateFileW")]
    private static extern SafeFileHandle CreateFileNative(
        string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandleEx(
        SafeFileHandle handle, int informationClass, out FileIdInfo information, uint bufferSize);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileSizeEx(SafeFileHandle handle, out long size);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern uint GetFinalPathNameByHandleW(
        SafeFileHandle handle, StringBuilder path, uint pathLength, uint flags);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true, EntryPoint = "GetVolumeInformationByHandleW")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetVolumeInformationByHandle(
        SafeFileHandle handle, StringBuilder volumeName, uint volumeNameSize,
        out uint serialNumber, out uint maximumComponentLength, out uint fileSystemFlags,
        StringBuilder fileSystemName, uint fileSystemNameSize);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DeviceIoControl(
        SafeFileHandle handle, uint controlCode, byte[] input, uint inputLength,
        byte[] output, uint outputLength, out uint bytesReturned, IntPtr overlapped);

    public static SafeFileHandle OpenReadShared(string path)
    {
        SafeFileHandle handle = CreateFileNative(path, GenericRead,
            ShareRead | ShareWrite | ShareDelete, IntPtr.Zero, OpenExisting, 0, IntPtr.Zero);
        if (handle == null || handle.IsInvalid)
        {
            int error = Marshal.GetLastWin32Error();
            if (handle != null) handle.Dispose();
            throw new Win32Exception(error);
        }
        return handle;
    }

    public static FileSample ReadFileSample(SafeFileHandle handle)
    {
        var timer = System.Diagnostics.Stopwatch.StartNew();
        FileIdInfo identity;
        if (!GetFileInformationByHandleEx(handle, FileIdInfoClass, out identity,
            (uint)Marshal.SizeOf(typeof(FileIdInfo))))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        if (identity.FileId == null || identity.FileId.Length != 16)
            throw new InvalidDataException("File identity was not returned in a supported form.");

        long length;
        if (!GetFileSizeEx(handle, out length) || length < 0)
            throw new Win32Exception(Marshal.GetLastWin32Error());

        var volumeName = new StringBuilder(261);
        var fileSystemName = new StringBuilder(261);
        uint volumeSerial32, maxComponent, fileSystemFlags;
        if (!GetVolumeInformationByHandle(handle, volumeName, (uint)volumeName.Capacity,
            out volumeSerial32, out maxComponent, out fileSystemFlags,
            fileSystemName, (uint)fileSystemName.Capacity))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        if (!String.Equals(fileSystemName.ToString(), "NTFS", StringComparison.OrdinalIgnoreCase))
            throw new NotSupportedException("The target file system is not NTFS.");

        var finalPath = new StringBuilder(32768);
        uint finalPathLength = GetFinalPathNameByHandleW(handle, finalPath,
            (uint)finalPath.Capacity, VolumeNameGuid);
        if (finalPathLength == 0 || finalPathLength >= finalPath.Capacity)
            throw new Win32Exception(Marshal.GetLastWin32Error());
        string volumeGuid = ExtractVolumeGuid(finalPath.ToString());

        // READ_FILE_USN_DATA requests only versions 2 and 3. Unknown versions fail closed.
        byte[] request = new byte[4];
        Buffer.BlockCopy(BitConverter.GetBytes((ushort)2), 0, request, 0, 2);
        Buffer.BlockCopy(BitConverter.GetBytes((ushort)3), 0, request, 2, 2);
        byte[] response = new byte[4096];
        uint bytesReturned;
        if (!DeviceIoControl(handle, FsctlReadFileUsnData, request, (uint)request.Length,
            response, (uint)response.Length, out bytesReturned, IntPtr.Zero))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        if (bytesReturned < 8)
            throw new InvalidDataException("The file USN response was shorter than its version header.");

        uint recordLength = BitConverter.ToUInt32(response, 0);
        ushort major = BitConverter.ToUInt16(response, 4);
        ushort minor = BitConverter.ToUInt16(response, 6);
        if (recordLength > bytesReturned || recordLength < 8 || minor != 0)
            throw new InvalidDataException("The file USN response length or minor version is unsupported.");

        long signedUsn;
        if (major == 2 && recordLength >= 60)
        {
            // The NTFS file-reference number is the first 64 bits of FILE_ID_INFO.FileId.
            ulong recordId = BitConverter.ToUInt64(response, 8);
            ulong handleId = BitConverter.ToUInt64(identity.FileId, 0);
            if (recordId != handleId)
                throw new InvalidDataException("USN record identity did not match the open file handle.");
            signedUsn = BitConverter.ToInt64(response, 24);
        }
        else if (major == 3 && recordLength >= 76)
        {
            for (int i = 0; i < 16; i++)
                if (response[8 + i] != identity.FileId[i])
                    throw new InvalidDataException("USN record identity did not match the open file handle.");
            signedUsn = BitConverter.ToInt64(response, 40);
        }
        else
        {
            throw new NotSupportedException("The file USN record major version is not supported.");
        }

        timer.Stop();
        return new FileSample {
            VolumeGuid = volumeGuid,
            FileSystem = "NTFS",
            VolumeSerial = identity.VolumeSerialNumber,
            FileId = BitConverter.ToString(identity.FileId).Replace("-", ""),
            Length = (ulong)length,
            MajorVersion = major,
            MinorVersion = minor,
            Usn = "0x" + unchecked((ulong)signedUsn).ToString("X16"),
            ElapsedMs = Math.Round(timer.Elapsed.TotalMilliseconds, 3)
        };
    }

    private static string ExtractVolumeGuid(string finalPath)
    {
        const string prefix = "\\\\?\\Volume{";
        if (!finalPath.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
            throw new NotSupportedException("A canonical volume GUID path was unavailable.");
        int close = finalPath.IndexOf('}', prefix.Length);
        if (close < 0 || close + 1 >= finalPath.Length || finalPath[close + 1] != '\\')
            throw new InvalidDataException("The canonical volume GUID path was malformed.");
        return finalPath.Substring(0, close + 2);
    }
}
'@

if (-not ('UsnFileTokenNativeV1' -as [type])) {
    Add-Type -TypeDefinition $script:UsnFileTokenNativeSource -Language CSharp
}

function Get-UsnJournalGeneration {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$VolumeGuid)

    $fsutil = Join-Path $env:SystemRoot 'System32\fsutil.exe'
    if (-not (Test-Path -LiteralPath $fsutil -PathType Leaf)) {
        throw [NotSupportedException]::new('fsutil is unavailable.')
    }
    $processInfo = [Diagnostics.ProcessStartInfo]::new()
    $processInfo.FileName = $fsutil
    $processInfo.ArgumentList.Add('usn')
    $processInfo.ArgumentList.Add('queryjournal')
    $processInfo.ArgumentList.Add($VolumeGuid)
    $processInfo.UseShellExecute = $false
    $processInfo.CreateNoWindow = $true
    $processInfo.RedirectStandardOutput = $true
    $processInfo.RedirectStandardError = $true

    $timer = [Diagnostics.Stopwatch]::StartNew()
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $processInfo
    try {
        if (-not $process.Start()) { throw [NotSupportedException]::new('USN query process failed to start.') }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $null = $stderrTask.GetAwaiter().GetResult()
        $timer.Stop()
        if ($process.ExitCode -ne 0) { throw [NotSupportedException]::new('USN journal query was denied or unsupported.') }

        $patterns = [ordered]@{
            JournalId = '^\s*Usn Journal ID\s*:\s*(0x[0-9A-F]+)\s*$'
            FirstUsn = '^\s*First Usn\s*:\s*(0x[0-9A-F]+)\s*$'
            LowestValidUsn = '^\s*Lowest Valid Usn\s*:\s*(0x[0-9A-F]+)\s*$'
        }
        $values = [ordered]@{}
        foreach ($name in $patterns.Keys) {
            $match = [regex]::Match($stdout, $patterns[$name], [Text.RegularExpressions.RegexOptions]::IgnoreCase -bor [Text.RegularExpressions.RegexOptions]::Multiline)
            if (-not $match.Success) { throw [NotSupportedException]::new('USN journal generation fields were incomplete or unrecognized.') }
            $values[$name] = $match.Groups[1].Value.ToUpperInvariant()
        }
        [pscustomobject]@{
            supported = $true
            journalId = $values.JournalId
            firstUsn = $values.FirstUsn
            lowestValidUsn = $values.LowestValidUsn
            queryElapsedMs = [Math]::Round($timer.Elapsed.TotalMilliseconds, 3)
        }
    }
    finally {
        $process.Dispose()
    }
}

function Test-UsnFieldEqual {
    param([object]$Left, [object]$Right)
    if ($null -eq $Left -or $null -eq $Right) { return $false }
    return [string]::Equals([string]$Left, [string]$Right, [StringComparison]::OrdinalIgnoreCase)
}

function Get-UsnPropertyValue {
    param([object]$InputObject, [string]$Name)
    if ($null -eq $InputObject) { return $null }
    $property = $InputObject.PSObject.Properties[$Name]
    if ($null -eq $property) { return $null }
    return $property.Value
}

function Test-UsnFileSampleEqual {
    param([object]$Left, [object]$Right)
    foreach ($field in @('volumeGuid', 'fileSystem', 'volumeSerial', 'fileId', 'length', 'majorVersion', 'minorVersion', 'usn')) {
        if (-not (Test-UsnFieldEqual (Get-UsnPropertyValue $Left $field) (Get-UsnPropertyValue $Right $field))) { return $false }
    }
    return $true
}

function Test-UsnJournalGenerationEqual {
    param([object]$Left, [object]$Right)
    foreach ($field in @('journalId', 'firstUsn', 'lowestValidUsn')) {
        if (-not (Test-UsnFieldEqual (Get-UsnPropertyValue $Left $field) (Get-UsnPropertyValue $Right $field))) { return $false }
    }
    return $true
}

function Test-UsnTokenShape {
    param([object]$Snapshot)
    if ((Get-UsnPropertyValue $Snapshot 'schema') -ne 'usn-file-token-v1') { return $false }
    $file = Get-UsnPropertyValue $Snapshot 'file'
    $journal = Get-UsnPropertyValue $Snapshot 'journal'
    if ($null -eq $file -or $null -eq $journal) { return $false }
    $major = Get-UsnPropertyValue $file 'majorVersion'
    $minor = Get-UsnPropertyValue $file 'minorVersion'
    if ($major -notin @(2, 3) -or $minor -ne 0) { return $false }
    if ((Get-UsnPropertyValue $file 'fileSystem') -ne 'NTFS') { return $false }
    $volumeGuid = [string](Get-UsnPropertyValue $file 'volumeGuid')
    $volumeSerial = [string](Get-UsnPropertyValue $file 'volumeSerial')
    $fileId = [string](Get-UsnPropertyValue $file 'fileId')
    $usn = [string](Get-UsnPropertyValue $file 'usn')
    $length = Get-UsnPropertyValue $file 'length'
    if ($volumeGuid -notmatch '^\\\\\?\\Volume\{[0-9A-Fa-f-]+\}\\$') { return $false }
    if ($volumeSerial -notmatch '^0x[0-9A-F]{16}$' -or $fileId -notmatch '^[0-9A-F]{32}$' -or $usn -notmatch '^0x[0-9A-F]{16}$') { return $false }
    try { $null = [UInt64]::Parse([string]$length) } catch { return $false }
    foreach ($field in @('journalId', 'firstUsn', 'lowestValidUsn')) {
        $value = [string](Get-UsnPropertyValue $journal $field)
        if ($value -notmatch '^0x[0-9A-F]{1,16}$') { return $false }
    }
    return $true
}

function Test-UsnCacheReusable {
    <#
      Returns true only for identical supported, stable tokens. False means the
      caller must perform the complete frozen hash-chain verification or reject.
      A changed per-file USN is invalidation only; this module never proves append.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][object]$Cached,
        [Parameter(Mandatory)][object]$Current
    )
    if ((Get-UsnPropertyValue $Cached 'supported') -ne $true -or
        (Get-UsnPropertyValue $Cached 'stable') -ne $true -or
        (Get-UsnPropertyValue $Current 'supported') -ne $true -or
        (Get-UsnPropertyValue $Current 'stable') -ne $true) { return $false }
    if (-not (Test-UsnTokenShape $Cached) -or -not (Test-UsnTokenShape $Current)) { return $false }
    if (-not (Test-UsnFileSampleEqual (Get-UsnPropertyValue $Cached 'file') (Get-UsnPropertyValue $Current 'file'))) { return $false }
    if (-not (Test-UsnJournalGenerationEqual (Get-UsnPropertyValue $Cached 'journal') (Get-UsnPropertyValue $Current 'journal'))) { return $false }
    return $true
}

function Get-FileUsnTokenSnapshot {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$LiteralPath,
        [scriptblock]$JournalGenerationReader,
        [scriptblock]$BetweenSamples
    )
    if ($null -eq $JournalGenerationReader) {
        $JournalGenerationReader = { param($volumeGuid) Get-UsnJournalGeneration -VolumeGuid $volumeGuid }
    }

    $handle = $null
    $pathCheckHandle = $null
    $sampleTimer = [Diagnostics.Stopwatch]::StartNew()
    try {
        $fullPath = (Get-Item -LiteralPath $LiteralPath -ErrorAction Stop).FullName
        $handle = [UsnFileTokenNativeV1]::OpenReadShared($fullPath)
        $before = [UsnFileTokenNativeV1]::ReadFileSample($handle)
        $journalBefore = & $JournalGenerationReader $before.VolumeGuid
        if ($null -ne $BetweenSamples) { & $BetweenSamples }
        $after = [UsnFileTokenNativeV1]::ReadFileSample($handle)
        $journalAfter = & $JournalGenerationReader $after.VolumeGuid
        # Re-open the name after the same-handle samples to fail closed if it was
        # replaced or redirected while the primary handle remained pinned.
        $pathCheckHandle = [UsnFileTokenNativeV1]::OpenReadShared($fullPath)
        $pathCheck = [UsnFileTokenNativeV1]::ReadFileSample($pathCheckHandle)
        $sampleTimer.Stop()

        $stable = (Test-UsnFileSampleEqual $before $after) -and
            (Test-UsnJournalGenerationEqual $journalBefore $journalAfter) -and
            (Test-UsnFileSampleEqual $after $pathCheck)
        [pscustomobject]@{
            schema = 'usn-file-token-v1'
            supported = [bool]($journalBefore.supported -and $journalAfter.supported)
            stable = [bool]$stable
            reason = if (-not $journalBefore.supported -or -not $journalAfter.supported) { 'journal_generation_unavailable' } elseif (-not $stable) { 'token_changed_during_sampling' } else { 'stable' }
            file = [pscustomobject]@{
                volumeGuid = $after.VolumeGuid
                fileSystem = $after.FileSystem
                volumeSerial = ('0x{0:X16}' -f $after.VolumeSerial)
                fileId = $after.FileId
                length = [UInt64]$after.Length
                majorVersion = [int]$after.MajorVersion
                minorVersion = [int]$after.MinorVersion
                usn = $after.Usn
            }
            journal = [pscustomobject]@{
                journalId = $journalAfter.journalId
                firstUsn = $journalAfter.firstUsn
                lowestValidUsn = $journalAfter.lowestValidUsn
            }
            elapsedMs = [Math]::Round($sampleTimer.Elapsed.TotalMilliseconds, 3)
            fileQueryMs = [Math]::Round($before.ElapsedMs + $after.ElapsedMs, 3)
            pathBindingQueryMs = [Math]::Round($pathCheck.ElapsedMs, 3)
            generationQueryMs = [Math]::Round($journalBefore.queryElapsedMs + $journalAfter.queryElapsedMs, 3)
        }
    }
    catch {
        $sampleTimer.Stop()
        [pscustomobject]@{
            schema = 'usn-file-token-v1'
            supported = $false
            stable = $false
            reason = if ($_.Exception -is [UnauthorizedAccessException] -or $_.Exception -is [System.ComponentModel.Win32Exception]) { 'access_denied_or_native_query_failed' } elseif ($_.Exception -is [NotSupportedException]) { 'unsupported_filesystem_or_generation_query' } else { 'token_query_failed' }
            file = $null
            journal = $null
            elapsedMs = [Math]::Round($sampleTimer.Elapsed.TotalMilliseconds, 3)
            fileQueryMs = $null
            pathBindingQueryMs = $null
            generationQueryMs = $null
        }
    }
    finally {
        if ($null -ne $pathCheckHandle) { $pathCheckHandle.Dispose() }
        if ($null -ne $handle) { $handle.Dispose() }
    }
}

Export-ModuleMember -Function Get-UsnJournalGeneration, Get-FileUsnTokenSnapshot, Test-UsnCacheReusable

