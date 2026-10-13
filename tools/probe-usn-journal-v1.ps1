[CmdletBinding()]
param(
    [string]$OutputPath = (Join-Path $PSScriptRoot '..\research\results\usn-journal-probe-20261013.json'),
    [ValidateRange(4096, 8388608)]
    [int]$ReadBufferBytes = 1048576
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$interopSource = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

public static class UsnJournalProbeNative
{
    public const uint FsctlQueryUsnJournal = 0x000900F4;
    public const uint FsctlReadUsnJournal = 0x000900BB;
    private const uint GenericRead = 0x80000000;
    private const uint ShareRead = 0x00000001;
    private const uint ShareWrite = 0x00000002;
    private const uint ShareDelete = 0x00000004;
    private const uint OpenExisting = 3;

    [StructLayout(LayoutKind.Sequential)]
    private struct ByHandleFileInformation
    {
        public uint FileAttributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME CreationTime;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastAccessTime;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWriteTime;
        public uint VolumeSerialNumber;
        public uint FileSizeHigh;
        public uint FileSizeLow;
        public uint NumberOfLinks;
        public uint FileIndexHigh;
        public uint FileIndexLow;
    }

    public sealed class IoResult
    {
        public bool Ok;
        public int Error;
        public uint Bytes;
        public byte[] Buffer;
    }

    public sealed class FileIdentity
    {
        public uint VolumeSerialNumber;
        public ulong FileReferenceNumber;
        public ulong Length;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true, EntryPoint = "CreateFileW")]
    private static extern SafeFileHandle CreateFileNative(
        string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DeviceIoControl(
        SafeFileHandle handle, uint controlCode, byte[] input, uint inputLength,
        byte[] output, uint outputLength, out uint bytesReturned, IntPtr overlapped);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandle(
        SafeFileHandle handle, out ByHandleFileInformation information);

    public static SafeFileHandle OpenVolumeWithAccess(string path, uint access)
    {
        return CreateFileNative(path, access, ShareRead | ShareWrite,
            IntPtr.Zero, OpenExisting, 0, IntPtr.Zero);
    }

    public static SafeFileHandle OpenFile(string path)
    {
        return CreateFileNative(path, GenericRead, ShareRead | ShareWrite | ShareDelete,
            IntPtr.Zero, OpenExisting, 0, IntPtr.Zero);
    }

    public static IoResult QueryJournal(SafeFileHandle handle, int outputLength)
    {
        byte[] output = new byte[outputLength];
        uint bytes;
        bool ok = DeviceIoControl(handle, FsctlQueryUsnJournal, null, 0,
            output, (uint)output.Length, out bytes, IntPtr.Zero);
        return new IoResult { Ok = ok, Error = ok ? 0 : Marshal.GetLastWin32Error(), Bytes = bytes, Buffer = output };
    }

    public static byte[] BuildReadRequest(long startUsn, ulong journalId)
    {
        byte[] data = new byte[40];
        Buffer.BlockCopy(BitConverter.GetBytes(startUsn), 0, data, 0, 8);
        Buffer.BlockCopy(BitConverter.GetBytes(UInt32.MaxValue), 0, data, 8, 4);
        Buffer.BlockCopy(BitConverter.GetBytes((uint)0), 0, data, 12, 4);
        Buffer.BlockCopy(BitConverter.GetBytes((ulong)0), 0, data, 16, 8);
        Buffer.BlockCopy(BitConverter.GetBytes((ulong)0), 0, data, 24, 8);
        Buffer.BlockCopy(BitConverter.GetBytes(journalId), 0, data, 32, 8);
        return data;
    }

    public static IoResult ReadJournal(SafeFileHandle handle, byte[] request, int outputLength)
    {
        byte[] output = new byte[outputLength];
        uint bytes;
        bool ok = DeviceIoControl(handle, FsctlReadUsnJournal, request, (uint)request.Length,
            output, (uint)output.Length, out bytes, IntPtr.Zero);
        return new IoResult { Ok = ok, Error = ok ? 0 : Marshal.GetLastWin32Error(), Bytes = bytes, Buffer = output };
    }

    public static FileIdentity GetIdentity(SafeFileHandle handle)
    {
        ByHandleFileInformation info;
        if (!GetFileInformationByHandle(handle, out info))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        ulong fileId = ((ulong)info.FileIndexHigh << 32) | info.FileIndexLow;
        ulong length = ((ulong)info.FileSizeHigh << 32) | info.FileSizeLow;
        return new FileIdentity { VolumeSerialNumber = info.VolumeSerialNumber, FileReferenceNumber = fileId, Length = length };
    }

    public static string ErrorMessage(int code)
    {
        return new Win32Exception(code).Message;
    }
}
'@

if (-not ('UsnJournalProbeNative' -as [type])) {
    Add-Type -TypeDefinition $interopSource -Language CSharp
}

function Format-FileReferenceNumber {
    param([UInt64]$Value)
    return ('0x{0:X16}' -f $Value)
}

function Get-ProbeFileIdentity {
    param([Parameter(Mandatory)][string]$Path)
    $handle = [UsnJournalProbeNative]::OpenFile($Path)
    try {
        if ($handle.IsInvalid) {
            $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            throw "Open file handle failed ($errorCode): $([UsnJournalProbeNative]::ErrorMessage($errorCode))"
        }
        $identity = [UsnJournalProbeNative]::GetIdentity($handle)
        [pscustomobject]@{
            volumeSerial = ('0x{0:X8}' -f $identity.VolumeSerialNumber)
            fileReferenceNumber = Format-FileReferenceNumber $identity.FileReferenceNumber
            lengthBytes = [UInt64]$identity.Length
        }
    }
    finally {
        if ($null -ne $handle -and -not $handle.IsInvalid) { $handle.Dispose() }
    }
}

function Get-UsnJournalSnapshot {
    param([Parameter(Mandatory)][Microsoft.Win32.SafeHandles.SafeFileHandle]$VolumeHandle)
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $result = [UsnJournalProbeNative]::QueryJournal($VolumeHandle, 128)
    $timer.Stop()
    if (-not $result.Ok) {
        throw "FSCTL_QUERY_USN_JOURNAL failed ($($result.Error)): $([UsnJournalProbeNative]::ErrorMessage($result.Error))"
    }
    if ($result.Bytes -lt 56) { throw "FSCTL_QUERY_USN_JOURNAL returned only $($result.Bytes) bytes; expected at least 56." }
    [pscustomobject]@{
        journalId = [BitConverter]::ToUInt64($result.Buffer, 0)
        firstUsn = [BitConverter]::ToInt64($result.Buffer, 8)
        nextUsn = [BitConverter]::ToInt64($result.Buffer, 16)
        lowestValidUsn = [BitConverter]::ToInt64($result.Buffer, 24)
        maxUsn = [BitConverter]::ToInt64($result.Buffer, 32)
        maximumSizeBytes = [BitConverter]::ToUInt64($result.Buffer, 40)
        allocationDeltaBytes = [BitConverter]::ToUInt64($result.Buffer, 48)
        elapsedMs = [Math]::Round($timer.Elapsed.TotalMilliseconds, 3)
    }
}

function Convert-UsnReasonFlags {
    param([UInt32]$Reason)
    $known = @(
        [pscustomobject]@{ mask = [UInt32]0x00000001; name = 'DATA_OVERWRITE' },
        [pscustomobject]@{ mask = [UInt32]0x00000002; name = 'DATA_EXTEND' },
        [pscustomobject]@{ mask = [UInt32]0x00000004; name = 'DATA_TRUNCATION' },
        [pscustomobject]@{ mask = [UInt32]0x00000100; name = 'FILE_CREATE' },
        [pscustomobject]@{ mask = [UInt32]0x00000200; name = 'FILE_DELETE' },
        [pscustomobject]@{ mask = [UInt32]0x00000400; name = 'EA_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x00000800; name = 'SECURITY_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x00001000; name = 'RENAME_OLD_NAME' },
        [pscustomobject]@{ mask = [UInt32]0x00002000; name = 'RENAME_NEW_NAME' },
        [pscustomobject]@{ mask = [UInt32]0x00004000; name = 'INDEXABLE_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x00008000; name = 'BASIC_INFO_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x00010000; name = 'HARD_LINK_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x00020000; name = 'COMPRESSION_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x00040000; name = 'ENCRYPTION_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x00080000; name = 'OBJECT_ID_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x00100000; name = 'REPARSE_POINT_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x00200000; name = 'STREAM_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x00400000; name = 'TRANSACTED_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x00800000; name = 'INTEGRITY_CHANGE' },
        [pscustomobject]@{ mask = [UInt32]0x80000000; name = 'CLOSE' }
    )
    @($known | Where-Object { ($Reason -band $_.mask) -ne 0 } | ForEach-Object name)
}

function Read-UsnRecordsSince {
    param(
        [Parameter(Mandatory)][Microsoft.Win32.SafeHandles.SafeFileHandle]$VolumeHandle,
        [Parameter(Mandatory)][UInt64]$JournalId,
        [Parameter(Mandatory)][Int64]$StartUsn,
        [Parameter(Mandatory)][Int64]$EndUsn,
        [Parameter(Mandatory)][int]$BufferBytes
    )

    $timer = [Diagnostics.Stopwatch]::StartNew()
    $cursor = $StartUsn
    $pages = 0
    $parsedRecords = [System.Collections.Generic.List[object]]::new()
    $unknownVersions = [System.Collections.Generic.List[int]]::new()
    $complete = $true
    $errorText = $null
    $maxPages = 64

    while ($cursor -lt $EndUsn -and $pages -lt $maxPages) {
        $request = [UsnJournalProbeNative]::BuildReadRequest($cursor, $JournalId)
        $response = [UsnJournalProbeNative]::ReadJournal($VolumeHandle, $request, $BufferBytes)
        $pages++
        if (-not $response.Ok) {
            $complete = $false
            $errorText = "FSCTL_READ_USN_JOURNAL failed ($($response.Error)): $([UsnJournalProbeNative]::ErrorMessage($response.Error))"
            break
        }
        if ($response.Bytes -lt 8) {
            $complete = $false
            $errorText = "FSCTL_READ_USN_JOURNAL returned only $($response.Bytes) bytes; expected the next-USN header."
            break
        }

        $nextCursor = [BitConverter]::ToInt64($response.Buffer, 0)
        $offset = 8
        while ($offset + 8 -le $response.Bytes) {
            $recordLength = [BitConverter]::ToUInt32($response.Buffer, $offset)
            if ($recordLength -lt 8 -or ($offset + $recordLength) -gt $response.Bytes) {
                $complete = $false
                $errorText = "Malformed USN record length $recordLength at response offset $offset."
                break
            }
            $major = [BitConverter]::ToUInt16($response.Buffer, $offset + 4)
            $minor = [BitConverter]::ToUInt16($response.Buffer, $offset + 6)
            if ($major -eq 2) {
                if ($recordLength -lt 60) {
                    $complete = $false
                    $errorText = "USN_RECORD_V2 length $recordLength is shorter than its 60-byte fixed header."
                    break
                }
                $fileRef = [BitConverter]::ToUInt64($response.Buffer, $offset + 8)
                $parentRef = [BitConverter]::ToUInt64($response.Buffer, $offset + 16)
                $recordUsn = [BitConverter]::ToInt64($response.Buffer, $offset + 24)
                $reason = [BitConverter]::ToUInt32($response.Buffer, $offset + 40)
                $nameLength = [BitConverter]::ToUInt16($response.Buffer, $offset + 56)
                $nameOffset = [BitConverter]::ToUInt16($response.Buffer, $offset + 58)
                $name = ''
                if ($nameLength -gt 0 -and ($nameOffset + $nameLength) -le $recordLength) {
                    $name = [Text.Encoding]::Unicode.GetString($response.Buffer, $offset + $nameOffset, $nameLength)
                }
                if ($recordUsn -ge $StartUsn -and $recordUsn -lt $EndUsn) {
                    $parsedRecords.Add([pscustomobject]@{
                        majorVersion = $major
                        minorVersion = $minor
                        usn = $recordUsn
                        fileReferenceNumber = Format-FileReferenceNumber $fileRef
                        parentFileReferenceNumber = Format-FileReferenceNumber $parentRef
                        reasonMask = ('0x{0:X8}' -f $reason)
                        reasonNames = @(Convert-UsnReasonFlags $reason)
                        fileName = $name
                    })
                }
            }
            else {
                if (-not $unknownVersions.Contains([int]$major)) { $unknownVersions.Add([int]$major) }
            }
            $offset += [int]$recordLength
        }
        if (-not $complete) { break }
        if ($nextCursor -le $cursor) {
            if ($cursor -lt $EndUsn) {
                $complete = $false
                $errorText = "USN reader did not advance from $cursor while the captured end USN is $EndUsn."
            }
            break
        }
        $cursor = $nextCursor
    }

    if ($pages -ge $maxPages -and $cursor -lt $EndUsn) {
        $complete = $false
        $errorText = "USN read reached the $maxPages-page safety limit before end USN $EndUsn."
    }
    $timer.Stop()
    [pscustomobject]@{
        startUsn = $StartUsn
        endUsn = $EndUsn
        nextCursor = $cursor
        pageCount = $pages
        elapsedMs = [Math]::Round($timer.Elapsed.TotalMilliseconds, 3)
        complete = $complete
        error = $errorText
        unknownMajorVersions = @($unknownVersions)
        records = @($parsedRecords)
    }
}

function Invoke-ProbeCase {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string[]]$RelevantFileIds,
        [Parameter(Mandatory)][scriptblock]$Action,
        [Parameter(Mandatory)][Microsoft.Win32.SafeHandles.SafeFileHandle]$VolumeHandle,
        [Parameter(Mandatory)][UInt64]$JournalId,
        [Parameter(Mandatory)][int]$BufferBytes,
        [Parameter(Mandatory)][string]$PrimaryPath
    )

    $beforeQuery = Get-UsnJournalSnapshot -VolumeHandle $VolumeHandle
    $beforeIdentity = $null
    if (Test-Path -LiteralPath $PrimaryPath) { $beforeIdentity = Get-ProbeFileIdentity -Path $PrimaryPath }
    $actionTimer = [Diagnostics.Stopwatch]::StartNew()
    & $Action
    $actionTimer.Stop()
    $afterIdentity = $null
    if (Test-Path -LiteralPath $PrimaryPath) { $afterIdentity = Get-ProbeFileIdentity -Path $PrimaryPath }
    $afterQuery = Get-UsnJournalSnapshot -VolumeHandle $VolumeHandle
    $read = Read-UsnRecordsSince -VolumeHandle $VolumeHandle -JournalId $JournalId `
        -StartUsn $beforeQuery.nextUsn -EndUsn $afterQuery.nextUsn -BufferBytes $BufferBytes
    $matches = @($read.records | Where-Object { $RelevantFileIds -contains $_.fileReferenceNumber })
    [pscustomobject]@{
        case = $Name
        startUsn = $beforeQuery.nextUsn
        endUsn = $afterQuery.nextUsn
        actionElapsedMs = [Math]::Round($actionTimer.Elapsed.TotalMilliseconds, 3)
        journalQueryBeforeMs = $beforeQuery.elapsedMs
        journalQueryAfterMs = $afterQuery.elapsedMs
        journalReadElapsedMs = $read.elapsedMs
        journalReadPages = $read.pageCount
        readComplete = $read.complete
        readError = $read.error
        unknownMajorVersions = $read.unknownMajorVersions
        primaryIdentityBefore = $beforeIdentity
        primaryIdentityAfter = $afterIdentity
        expectedReasonObserved = $false
        expectedIdentityAndRenameObserved = $false
        relevantFileIds = @($RelevantFileIds)
        recordsScanned = @($read.records).Count
        matchingRecords = $matches
    }
}

$startedAt = [DateTime]::UtcNow
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
$isElevatedAdministrator = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$driveRoot = [IO.Path]::GetPathRoot($tempRoot)
$volumeDevicePath = $null
$fileSystem = $null
$probeDirectory = $null
$volumeHandle = $null
$scriptFailure = $null
$cases = [System.Collections.Generic.List[object]]::new()
$result = [ordered]@{
    schemaVersion = 'usn-journal-probe-v1'
    generatedAtUtc = $startedAt.ToString('o')
    host = [ordered]@{
        osVersion = [Environment]::OSVersion.VersionString
        powerShellVersion = $PSVersionTable.PSVersion.ToString()
        elevatedAdministratorToken = $isElevatedAdministrator
    }
    scope = 'Temporary throwaway files only. The script opens the existing volume journal read-only and never creates, deletes, or resizes it.'
    volume = $null
    capability = [ordered]@{
        volumeOpen = $false
        volumeOpenAttempts = @()
        volumeHandleAccess = $null
        queryUsnJournal = $false
        readUsnJournal = $false
        fsutilQueryJournal = [ordered]@{ attempted = $false; succeeded = $false; elapsedMs = $null; journalId = $null; firstUsn = $null; nextUsn = $null; lowestValidUsn = $null; error = $null }
        fsutilReadJournal = [ordered]@{ attempted = $false; succeeded = $false; startUsn = $null; elapsedMs = $null; outputLineCount = $null; sample = @(); error = $null }
        fileIdentityRead = $false
        privilegeAssessment = $null
        status = 'not_started'
        error = $null
    }
    journal = $null
    probe = [ordered]@{
        temporaryDirectory = $null
        temporaryFilesCleaned = $false
        cases = @()
        identitySummary = $null
    }
    latency = $null
    conclusion = [ordered]@{
        allRequestedMutationsDistinguished = $false
        mutationProbeStatus = 'not_started'
        mutationCasesAttempted = 0
        mutationProbeBlocker = $null
        fullHashFallbackMandatory = $true
        fallbackConditions = @(
            'The target volume is not local NTFS, has no readable USN journal, or volume/journal access is denied under the process token.',
            'The journal ID changes, a saved USN falls below the retained FirstUsn/LowestValidUsn boundary, or a read fails or does not advance completely.',
            'The target file identity cannot be read, the volume identity does not match, or USN record versions/IDs are not fully parsed and supported.',
            'Any observed mutation has missing, contradictory, or unknown reason flags; replacement/truncation, a rename, or a hard-link change cannot be proved from stable identities and complete records.',
            'The file changes during verification, a stable end-USN checkpoint cannot be established, or the bytes in the claimed appended suffix cannot be hash-verified against the trusted chain.',
            'A candidate update is not a pure append to the same file identity. USN reason flags are change evidence and do not validate appended content.'
        )
    }
    sources = @(
        'https://learn.microsoft.com/windows/win32/api/winioctl/ni-winioctl-fsctl_query_usn_journal',
        'https://learn.microsoft.com/windows/win32/api/winioctl/ni-winioctl-fsctl_read_usn_journal',
        'https://learn.microsoft.com/windows/desktop/api/winioctl/ns-winioctl-usn_record_v2'
    )
    errors = @()
}

try {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw 'This probe requires Windows NT.'
    }
    if ($driveRoot -notmatch '^[A-Za-z]:\\$') {
        throw "The temporary directory is not on a drive-letter volume: $tempRoot"
    }
    $driveLetter = $driveRoot.Substring(0, 1)
    $volumeDevicePath = ('\\.\{0}:' -f $driveLetter)
    $fileSystem = ([IO.DriveInfo]::new($driveRoot)).DriveFormat
    $result.volume = [ordered]@{
        driveRoot = $driveRoot
        devicePath = $volumeDevicePath
        fileSystem = $fileSystem
        devicePathUsesLocalDriveLetter = $true
    }
    if ($fileSystem -ne 'NTFS') {
        $result.capability.status = 'unsupported_filesystem'
        throw "Temporary directory volume is $fileSystem; FSCTL USN probe requires local NTFS."
    }

    $probeDirectory = Join-Path $tempRoot ('codex-usn-probe-' + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($probeDirectory) | Out-Null
    $result.probe.temporaryDirectory = '<temporary probe directory; cleaned by script>'

    $targetPath = Join-Path $probeDirectory 'target.tmp'
    $replacementPath = Join-Path $probeDirectory 'replacement.tmp'
    [IO.File]::WriteAllBytes($targetPath, [Text.Encoding]::ASCII.GetBytes('original-data-0001'))
    [IO.File]::WriteAllBytes($replacementPath, [Text.Encoding]::ASCII.GetBytes('replacement-data-2'))
    $targetIdentity = Get-ProbeFileIdentity -Path $targetPath
    $replacementIdentity = Get-ProbeFileIdentity -Path $replacementPath
    $result.capability.fileIdentityRead = $true

    $targetFileId = $targetIdentity.fileReferenceNumber
    $replacementFileId = $replacementIdentity.fileReferenceNumber
    if ($targetIdentity.volumeSerial -ne $replacementIdentity.volumeSerial) {
        throw 'The throwaway target and replacement file handles returned different volume identities.'
    }
    if ($targetFileId -eq $replacementFileId) {
        throw 'The two throwaway files unexpectedly share a file reference number.'
    }
    $result.probe.identitySummary = [ordered]@{
        volumeSerial = $targetIdentity.volumeSerial
        originalFileReferenceNumber = $targetFileId
        replacementFileReferenceNumber = $replacementFileId
        originalAndReplacementDistinct = ($targetFileId -ne $replacementFileId)
        replacementPathNowHasReplacementIdentity = $null
    }

    $fsutilPath = Join-Path $env:WINDIR 'System32\fsutil.exe'
    $volumeArgument = '{0}:' -f $driveLetter
    if (Test-Path -LiteralPath $fsutilPath) {
        $fsutilQueryTimer = [Diagnostics.Stopwatch]::StartNew()
        $fsutilQueryLines = @(& $fsutilPath usn queryjournal $volumeArgument 2>&1 | ForEach-Object { $_.ToString() })
        $fsutilQueryTimer.Stop()
        $fsutilQueryExitCode = $LASTEXITCODE
        $result.capability.fsutilQueryJournal.attempted = $true
        $result.capability.fsutilQueryJournal.succeeded = ($fsutilQueryExitCode -eq 0)
        $result.capability.fsutilQueryJournal.elapsedMs = [Math]::Round($fsutilQueryTimer.Elapsed.TotalMilliseconds, 3)
        $nextUsnLine = @($fsutilQueryLines | Where-Object { $_ -match '^\s*Next Usn\s*:' } | Select-Object -First 1)
        $journalIdLine = @($fsutilQueryLines | Where-Object { $_ -match '^\s*Usn Journal ID\s*:' } | Select-Object -First 1)
        $firstUsnLine = @($fsutilQueryLines | Where-Object { $_ -match '^\s*First Usn\s*:' } | Select-Object -First 1)
        $lowestUsnLine = @($fsutilQueryLines | Where-Object { $_ -match '^\s*Lowest Valid Usn\s*:' } | Select-Object -First 1)
        if ($nextUsnLine.Count -gt 0) {
            $nextUsnToken = (($nextUsnLine[0] -split ':', 2)[1].Trim() -split '\s+')[0]
            $result.capability.fsutilQueryJournal.nextUsn = $nextUsnToken
        }
        if ($journalIdLine.Count -gt 0) {
            $result.capability.fsutilQueryJournal.journalId = (($journalIdLine[0] -split ':', 2)[1].Trim() -split '\s+')[0]
        }
        if ($firstUsnLine.Count -gt 0) {
            $result.capability.fsutilQueryJournal.firstUsn = (($firstUsnLine[0] -split ':', 2)[1].Trim() -split '\s+')[0]
        }
        if ($lowestUsnLine.Count -gt 0) {
            $result.capability.fsutilQueryJournal.lowestValidUsn = (($lowestUsnLine[0] -split ':', 2)[1].Trim() -split '\s+')[0]
        }
        if ($fsutilQueryExitCode -eq 0 -and $nextUsnLine.Count -gt 0) {
            $result.journal = [ordered]@{
                source = 'fsutil queryjournal'
                journalId = $result.capability.fsutilQueryJournal.journalId
                firstUsn = $result.capability.fsutilQueryJournal.firstUsn
                nextUsnAtStart = $result.capability.fsutilQueryJournal.nextUsn
                lowestValidUsn = $result.capability.fsutilQueryJournal.lowestValidUsn
                savedUsnIsReadableAtStart = $null
            }
        }
        if ($fsutilQueryExitCode -ne 0 -or $nextUsnLine.Count -eq 0) {
            $result.capability.fsutilQueryJournal.error = (@($fsutilQueryLines | Select-Object -First 5) -join ' ')
        }
        elseif ($null -ne $result.capability.fsutilQueryJournal.nextUsn) {
            $fsutilReadTimer = [Diagnostics.Stopwatch]::StartNew()
            $fsutilReadLines = @(& $fsutilPath usn readjournal $volumeArgument "startusn=$($result.capability.fsutilQueryJournal.nextUsn)" 2>&1 | ForEach-Object { $_.ToString() })
            $fsutilReadTimer.Stop()
            $fsutilReadExitCode = $LASTEXITCODE
            $result.capability.fsutilReadJournal.attempted = $true
            $result.capability.fsutilReadJournal.succeeded = ($fsutilReadExitCode -eq 0)
            $result.capability.fsutilReadJournal.startUsn = $result.capability.fsutilQueryJournal.nextUsn
            $result.capability.fsutilReadJournal.elapsedMs = [Math]::Round($fsutilReadTimer.Elapsed.TotalMilliseconds, 3)
            $result.capability.fsutilReadJournal.outputLineCount = $fsutilReadLines.Count
            $result.capability.fsutilReadJournal.sample = @($fsutilReadLines | Select-Object -First 20)
            if ($fsutilReadExitCode -ne 0) {
                $result.capability.fsutilReadJournal.error = (@($fsutilReadLines | Select-Object -First 5) -join ' ')
            }
        }
    }
    else {
        $result.capability.fsutilQueryJournal.error = 'fsutil.exe was not found at the standard Windows system path.'
    }

    $baseline = $null
    $accessReadElapsedMs = $null
    $accessLabels = @(
        [pscustomobject]@{ label = 'GENERIC_READ'; access = [Convert]::ToUInt32('80000000', 16) },
        [pscustomobject]@{ label = 'FILE_READ_DATA'; access = [UInt32]0x00000001 },
        [pscustomobject]@{ label = 'FILE_READ_ATTRIBUTES'; access = [UInt32]0x00000080 },
        [pscustomobject]@{ label = 'none'; access = [UInt32]0x00000000 }
    )
    foreach ($accessLabel in $accessLabels) {
        $candidateHandle = [UsnJournalProbeNative]::OpenVolumeWithAccess($volumeDevicePath, $accessLabel.access)
        if ($candidateHandle.IsInvalid) {
            $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            $result.capability.volumeOpenAttempts += [pscustomobject]@{
                requestedAccess = $accessLabel.label
                volumeHandleOpened = $false
                queryUsnJournal = $false
                readUsnJournal = $false
                errorCode = $errorCode
                error = [UsnJournalProbeNative]::ErrorMessage($errorCode)
            }
            $candidateHandle.Dispose()
            continue
        }

        $result.capability.volumeOpen = $true
        try {
            $candidateSnapshot = Get-UsnJournalSnapshot -VolumeHandle $candidateHandle
        }
        catch {
            $result.capability.volumeOpenAttempts += [pscustomobject]@{
                requestedAccess = $accessLabel.label
                volumeHandleOpened = $true
                queryUsnJournal = $false
                readUsnJournal = $false
                errorCode = $null
                error = $_.Exception.Message
            }
            $candidateHandle.Dispose()
            continue
        }

        $candidateReadTimer = [Diagnostics.Stopwatch]::StartNew()
        $candidateReadRequest = [UsnJournalProbeNative]::BuildReadRequest($candidateSnapshot.nextUsn, $candidateSnapshot.journalId)
        $candidateRead = [UsnJournalProbeNative]::ReadJournal($candidateHandle, $candidateReadRequest, $ReadBufferBytes)
        $candidateReadTimer.Stop()
        if (-not $candidateRead.Ok -or $candidateRead.Bytes -lt 8) {
            $candidateError = if (-not $candidateRead.Ok) {
                "FSCTL_READ_USN_JOURNAL failed ($($candidateRead.Error)): $([UsnJournalProbeNative]::ErrorMessage($candidateRead.Error))"
            } else { "FSCTL_READ_USN_JOURNAL returned only $($candidateRead.Bytes) bytes for the access probe." }
            $result.capability.volumeOpenAttempts += [pscustomobject]@{
                requestedAccess = $accessLabel.label
                volumeHandleOpened = $true
                queryUsnJournal = $true
                readUsnJournal = $false
                errorCode = if ($candidateRead.Ok) { $null } else { $candidateRead.Error }
                error = $candidateError
            }
            $candidateHandle.Dispose()
            continue
        }

        $result.capability.volumeOpenAttempts += [pscustomobject]@{
            requestedAccess = $accessLabel.label
            volumeHandleOpened = $true
            queryUsnJournal = $true
            readUsnJournal = $true
            errorCode = 0
            error = $null
        }
        $volumeHandle = $candidateHandle
        $baseline = $candidateSnapshot
        $accessReadElapsedMs = [Math]::Round($candidateReadTimer.Elapsed.TotalMilliseconds, 3)
        $result.capability.volumeHandleAccess = $accessLabel.label
        $result.capability.queryUsnJournal = $true
        $result.capability.readUsnJournal = $true
        break
    }
    if ($null -eq $baseline) {
        $attemptDetails = (@($result.capability.volumeOpenAttempts | ForEach-Object {
            "$($_.requestedAccess): $($_.error)"
        }) -join '; ')
        if ((@($result.capability.volumeOpenAttempts | Where-Object { $_.error -match 'Access is denied|Error 5' }).Count -gt 0) -or
            $result.capability.fsutilReadJournal.error -match 'Access is denied|Error 5') {
            $result.capability.status = 'read_access_denied'
        }
        $result.capability.error = "No tested volume-handle access level could query and read the USN journal. $attemptDetails"
        throw $result.capability.error
    }
    $result.capability.status = 'journal_query_and_read_ok'
    $result.journal = [ordered]@{
        journalId = ('0x{0:X16}' -f $baseline.journalId)
        firstUsn = $baseline.firstUsn
        nextUsnAtStart = $baseline.nextUsn
        lowestValidUsn = $baseline.lowestValidUsn
        maxUsn = $baseline.maxUsn
        maximumSizeBytes = $baseline.maximumSizeBytes
        allocationDeltaBytes = $baseline.allocationDeltaBytes
        initialQueryElapsedMs = $baseline.elapsedMs
        accessReadElapsedMs = $accessReadElapsedMs
        savedUsnIsReadableAtStart = ($baseline.nextUsn -ge $baseline.firstUsn -and $baseline.nextUsn -ge $baseline.lowestValidUsn)
    }
    if (-not $result.journal.savedUsnIsReadableAtStart) {
        throw 'The initial saved USN is outside the current journal readability bounds.'
    }

    $appendCase = Invoke-ProbeCase -Name 'append' -RelevantFileIds @($targetFileId) -VolumeHandle $volumeHandle `
        -JournalId $baseline.journalId -BufferBytes $ReadBufferBytes -PrimaryPath $targetPath -Action {
        $stream = [IO.File]::Open($targetPath, [IO.FileMode]::Open, [IO.FileAccess]::Write,
            [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
        try { $stream.Seek(0, [IO.SeekOrigin]::End) | Out-Null; $bytes = [Text.Encoding]::ASCII.GetBytes('APPEND'); $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) }
        finally { $stream.Dispose() }
    }
    $cases.Add($appendCase)

    $overwriteCase = Invoke-ProbeCase -Name 'same_size_overwrite' -RelevantFileIds @($targetFileId) -VolumeHandle $volumeHandle `
        -JournalId $baseline.journalId -BufferBytes $ReadBufferBytes -PrimaryPath $targetPath -Action {
        $stream = [IO.File]::Open($targetPath, [IO.FileMode]::Open, [IO.FileAccess]::Write,
            [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
        try { $bytes = [Text.Encoding]::ASCII.GetBytes('OVERWRITE'); $stream.Position = 0; $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) }
        finally { $stream.Dispose() }
    }
    $cases.Add($overwriteCase)

    $truncateCase = Invoke-ProbeCase -Name 'truncation' -RelevantFileIds @($targetFileId) -VolumeHandle $volumeHandle `
        -JournalId $baseline.journalId -BufferBytes $ReadBufferBytes -PrimaryPath $targetPath -Action {
        $stream = [IO.File]::Open($targetPath, [IO.FileMode]::Open, [IO.FileAccess]::Write,
            [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
        try { $stream.SetLength(5); $stream.Flush($true) }
        finally { $stream.Dispose() }
    }
    $cases.Add($truncateCase)

    $replacementCase = Invoke-ProbeCase -Name 'replacement' -RelevantFileIds @($targetFileId, $replacementFileId) `
        -VolumeHandle $volumeHandle -JournalId $baseline.journalId -BufferBytes $ReadBufferBytes `
        -PrimaryPath $targetPath -Action {
        [IO.File]::Move($replacementPath, $targetPath, $true)
    }
    $cases.Add($replacementCase)

    $expectedByCase = @{
        append = @{ fileId = $targetFileId; reason = 'DATA_EXTEND' }
        same_size_overwrite = @{ fileId = $targetFileId; reason = 'DATA_OVERWRITE' }
        truncation = @{ fileId = $targetFileId; reason = 'DATA_TRUNCATION' }
    }
    foreach ($case in $cases) {
        if (-not $case.readComplete) { continue }
        if ($case.unknownMajorVersions.Count -gt 0) { continue }
        if ($expectedByCase.ContainsKey($case.case)) {
            $expectation = $expectedByCase[$case.case]
            $found = @($case.matchingRecords | Where-Object {
                $_.fileReferenceNumber -eq $expectation.fileId -and $_.reasonNames -contains $expectation.reason
            }).Count -gt 0
            $case.expectedReasonObserved = [bool]$found
        }
    }

    $replacementObserved = $replacementCase.readComplete -and
        $replacementCase.unknownMajorVersions.Count -eq 0 -and
        $replacementCase.primaryIdentityBefore.fileReferenceNumber -eq $targetFileId -and
        $replacementCase.primaryIdentityAfter.fileReferenceNumber -eq $replacementFileId -and
        @($replacementCase.matchingRecords | Where-Object { $_.fileReferenceNumber -eq $targetFileId -and $_.reasonNames -contains 'FILE_DELETE' }).Count -gt 0 -and
        @($replacementCase.matchingRecords | Where-Object { $_.fileReferenceNumber -eq $replacementFileId -and ($_.reasonNames -contains 'RENAME_OLD_NAME' -or $_.reasonNames -contains 'RENAME_NEW_NAME') }).Count -gt 0
    $replacementCase.expectedIdentityAndRenameObserved = [bool]$replacementObserved

    $allCasesPassed = ($cases | Where-Object {
        if ($_.case -eq 'replacement') { -not $_.expectedIdentityAndRenameObserved }
        else { -not $_.expectedReasonObserved }
    }).Count -eq 0
    $allReadsComplete = ($cases | Where-Object { -not $_.readComplete -or $_.unknownMajorVersions.Count -gt 0 }).Count -eq 0
    $result.conclusion.allRequestedMutationsDistinguished = [bool]($allCasesPassed -and $allReadsComplete)
    $result.conclusion.mutationProbeStatus = if ($result.conclusion.allRequestedMutationsDistinguished) { 'completed_all_expected_signals' } else { 'completed_with_ambiguity' }
    $result.conclusion.mutationProbeBlocker = if ($result.conclusion.allRequestedMutationsDistinguished) { $null } else { 'At least one mutation had an incomplete read, unknown record version, or missing expected identity/reason evidence.' }
    $result.conclusion.fullHashFallbackMandatory = $true
    $result.capability.status = if ($result.conclusion.allRequestedMutationsDistinguished) { 'all_throwaway_cases_observed' } else { 'partial_or_ambiguous' }
    $result.probe.identitySummary = [ordered]@{
        volumeSerial = $targetIdentity.volumeSerial
        originalFileReferenceNumber = $targetFileId
        replacementFileReferenceNumber = $replacementFileId
        originalAndReplacementDistinct = ($targetFileId -ne $replacementFileId)
        replacementPathNowHasReplacementIdentity = ($replacementCase.primaryIdentityAfter.fileReferenceNumber -eq $replacementFileId)
    }
}
catch {
    $scriptFailure = $_.Exception.Message
    $result.errors = @($result.errors) + @($scriptFailure)
    if ($result.capability.status -eq 'not_started') { $result.capability.status = 'failed' }
    if (-not $result.capability.error) { $result.capability.error = $scriptFailure }
}
finally {
    if ($null -ne $volumeHandle -and -not $volumeHandle.IsInvalid) { $volumeHandle.Dispose() }
    if ($null -ne $probeDirectory -and (Test-Path -LiteralPath $probeDirectory)) {
        try {
            [IO.Directory]::Delete($probeDirectory, $true)
            $result.probe.temporaryFilesCleaned = $true
        }
        catch {
            $cleanupError = "Temporary probe cleanup failed for its own generated path: $($_.Exception.Message)"
            $result.errors = @($result.errors) + @($cleanupError)
            $result.probe.temporaryFilesCleaned = $false
        }
    }
    elseif ($null -eq $probeDirectory) {
        $result.probe.temporaryFilesCleaned = $true
    }
}

$result.probe.cases = @($cases)
$readLatency = @($cases | ForEach-Object { [double]$_.journalReadElapsedMs } | Sort-Object)
$queryLatency = @($cases | ForEach-Object { [double]$_.journalQueryBeforeMs; [double]$_.journalQueryAfterMs } | Sort-Object)
if ($readLatency.Count -gt 0 -or $queryLatency.Count -gt 0 -or $result.capability.fsutilQueryJournal.elapsedMs -ne $null -or $result.capability.fsutilReadJournal.elapsedMs -ne $null) {
    $result.latency = [ordered]@{
        fsctlReadSampleCount = $readLatency.Count
        fsctlReadMinMs = if ($readLatency.Count) { $readLatency[0] } else { $null }
        fsctlReadMedianMs = if ($readLatency.Count) { $readLatency[[Math]::Floor(($readLatency.Count - 1) / 2)] } else { $null }
        fsctlReadMaxMs = if ($readLatency.Count) { $readLatency[-1] } else { $null }
        fsctlQuerySampleCount = $queryLatency.Count
        fsctlQueryMinMs = if ($queryLatency.Count) { $queryLatency[0] } else { $null }
        fsctlQueryMedianMs = if ($queryLatency.Count) { $queryLatency[[Math]::Floor(($queryLatency.Count - 1) / 2)] } else { $null }
        fsctlQueryMaxMs = if ($queryLatency.Count) { $queryLatency[-1] } else { $null }
        fsutilQueryMs = $result.capability.fsutilQueryJournal.elapsedMs
        fsutilReadMs = $result.capability.fsutilReadJournal.elapsedMs
    }
}

$result.completedAtUtc = [DateTime]::UtcNow.ToString('o')
$result.conclusion.mutationCasesAttempted = $cases.Count
if ($cases.Count -eq 0 -and $result.conclusion.mutationProbeStatus -eq 'not_started') {
    if ($result.capability.status -eq 'read_access_denied') {
        $result.conclusion.mutationProbeStatus = 'not_run_access_denied'
        $result.conclusion.mutationProbeBlocker = $result.capability.error
    }
    elseif ($scriptFailure) {
        $result.conclusion.mutationProbeStatus = 'not_run_setup_failed'
        $result.conclusion.mutationProbeBlocker = $scriptFailure
    }
}
$result.capability.privilegeAssessment = [ordered]@{
    currentProcessElevatedAdministrator = $result.host.elevatedAdministratorToken
    fileIdentityReadableWithCurrentToken = $result.capability.fileIdentityRead
    fsutilQueryReadableWithCurrentToken = $result.capability.fsutilQueryJournal.succeeded
    fsutilRecordReadReadableWithCurrentToken = $result.capability.fsutilReadJournal.succeeded
    nativeJournalReadReadableWithCurrentToken = $result.capability.readUsnJournal
    assessment = if ($result.capability.readUsnJournal -or $result.capability.fsutilReadJournal.succeeded) {
        'The current process token read journal data in this probe; this does not establish the minimum rights required on other volumes or hosts.'
    } elseif ($result.capability.fsutilReadJournal.error -match 'Access is denied|Error 5' -or
        (@($result.capability.volumeOpenAttempts | Where-Object { $_.error -match 'Access is denied' }).Count -gt 0)) {
        'The current non-elevated token was denied journal-record access. Elevation or an explicit volume permission may be required; this run does not isolate which one is sufficient.'
    } else {
        'Journal-record access could not be established under the current token; verify volume permissions and journal availability before relying on this path.'
    }
}
if ($null -ne $result.probe.identitySummary) {
    $result.probe.identitySummary = [ordered]@{
        originalAndReplacementDistinct = [bool]$result.probe.identitySummary.originalAndReplacementDistinct
        replacementPathNowHasReplacementIdentity = $result.probe.identitySummary.replacementPathNowHasReplacementIdentity
    }
}
$result.outputPath = [IO.Path]::GetFileName($OutputPath)
$outputDirectory = Split-Path -Parent $result.outputPath
if (-not (Test-Path -LiteralPath $outputDirectory)) { New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null }
$json = $result | ConvertTo-Json -Depth 12
Set-Content -LiteralPath $result.outputPath -Value $json -Encoding utf8
$result | ConvertTo-Json -Depth 12

if ($scriptFailure) { exit 2 }
if (-not $result.conclusion.allRequestedMutationsDistinguished) { exit 1 }
exit 0

