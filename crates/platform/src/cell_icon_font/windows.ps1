$fontName = 'TundraCellAtlas.ttf'
$fontValue = 'Tundra Cell Atlas (TrueType)'
$fontKey = 'Software\Microsoft\Windows NT\CurrentVersion\Fonts'

function Initialize-NativeFontApi {
    if ('TundraNativeFont' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using FILETIME = System.Runtime.InteropServices.ComTypes.FILETIME;
using System.Security.Cryptography;
using Microsoft.Win32.SafeHandles;

public static class TundraNativeFont {
    [StructLayout(LayoutKind.Sequential)]
    struct FileInfo {
        public uint Attributes;
        public FILETIME Created, Accessed, Written;
        public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security,
        uint mode, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool GetFileInformationByHandle(SafeFileHandle handle, out FileInfo info);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool SetFileInformationByHandle(SafeFileHandle handle, int infoClass,
        ref byte value, uint size);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool SetFileInformationByHandle(SafeFileHandle handle, int infoClass,
        ref uint value, uint size);

    [DllImport("gdi32.dll", CharSet = CharSet.Unicode)]
    public static extern int AddFontResource(string path);
    [DllImport("gdi32.dll", CharSet = CharSet.Unicode)]
    public static extern bool RemoveFontResource(string path);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern bool SendNotifyMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    public static void Notify() {
        SendNotifyMessage(new IntPtr(0xffff), 0x001d, IntPtr.Zero, IntPtr.Zero);
    }

    public static string Fingerprint(string path, string removeExpected) {
        const uint read = 0x80000000, delete = 0x00010000, shareRead = 1;
        const uint openExisting = 3, openReparsePoint = 0x00200000;
        using (var handle = CreateFile(path, read | (removeExpected == null ? 0 : delete),
            shareRead, IntPtr.Zero, openExisting, openReparsePoint, IntPtr.Zero)) {
            if (handle.IsInvalid) {
                int error = Marshal.GetLastWin32Error();
                if (error == 2 || error == 3) return null;
                throw new IOException("Cannot open the font file: " + new Win32Exception(error).Message);
            }
            FileInfo info;
            if (!GetFileInformationByHandle(handle, out info)) throw new Win32Exception();
            if ((info.Attributes & 0x410) != 0) return "other";
            using (var file = new FileStream(handle, FileAccess.Read))
            using (var sha = SHA256.Create()) {
                string hash = BitConverter.ToString(sha.ComputeHash(file)).Replace("-", "");
                string fingerprint = String.Format("{0:X8}{1:X8}{2:X8}{3:X8}{4:X8}:{5}",
                    info.Volume, info.IndexHigh, info.IndexLow,
                    info.Written.dwHighDateTime, info.Written.dwLowDateTime, hash);
                if (removeExpected != null && fingerprint == removeExpected) {
                    DeleteOwnedFont(handle);
                }
                return fingerprint;
            }
        }
    }

    static void DeleteOwnedFont(SafeFileHandle handle) {
        const int fileDispositionInfo = 4, fileDispositionInfoEx = 21;
        const uint delete = 1, posixSemantics = 2;
        uint flags = delete | posixSemantics;
        // Drop the filename even if a renderer still has the font mapped.
        if (SetFileInformationByHandle(handle, fileDispositionInfoEx, ref flags, 4)) return;
        int error = Marshal.GetLastWin32Error();
        if (error == 1 || error == 50 || error == 87) {
            byte remove = 1;
            if (SetFileInformationByHandle(handle, fileDispositionInfo, ref remove, 1)) return;
            error = Marshal.GetLastWin32Error();
        }
        throw new IOException("Cannot delete the font file: " + new Win32Exception(error).Message);
    }
}
'@
}

function Convert-FontHex([string] $hex) {
    $bytes = [byte[]]::new($hex.Length / 2)
    for ($i = 0; $i -lt $bytes.Length; $i++) {
        $bytes[$i] = [Convert]::ToByte($hex.Substring(2 * $i, 2), 16)
    }
    return ,$bytes
}

function Get-FontHash([byte[]] $bytes) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($sha.ComputeHash($bytes)).Replace('-', '') }
    finally { $sha.Dispose() }
}

function Get-PathAttributes([string] $path) {
    try { return [IO.File]::GetAttributes($path) }
    catch [IO.FileNotFoundException], [IO.DirectoryNotFoundException] { return $null }
}

function Assert-PlainPath([string] $path) {
    for ($part = $path; $part; $part = [IO.Path]::GetDirectoryName($part)) {
        if ((Get-PathAttributes $part) -band [IO.FileAttributes]::ReparsePoint) {
            throw "Cell icon font path uses a reparse point: $part"
        }
    }
}

function Open-FontLock([string] $owner) {
    Assert-PlainPath $owner
    [void][IO.Directory]::CreateDirectory($owner)
    $lockPath = Join-Path $owner 'install.lock'
    Assert-PlainPath $lockPath
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while ($true) {
        try { return [IO.File]::Open($lockPath, 'OpenOrCreate', 'ReadWrite', 'None') }
        catch [IO.IOException] {
            if (($_.Exception.InnerException.HResult -band 0xffff) -notin @(32, 33) -or
                [DateTime]::UtcNow -gt $deadline) { throw }
            Start-Sleep -Milliseconds 50
        }
    }
}

function Write-FontFile([string] $path, [byte[]] $bytes) {
    $file = [IO.File]::Open($path, 'CreateNew', 'Write', 'None')
    try {
        $file.Write($bytes, 0, $bytes.Length)
        $file.Flush($true)
    } finally { $file.Dispose() }
}

function Save-FontOwnership($record, [string] $path) {
    $temporary = "$path.next"
    Assert-PlainPath $temporary
    [IO.File]::Delete($temporary)
    Write-FontFile $temporary ([Text.Encoding]::UTF8.GetBytes(($record | ConvertTo-Json -Compress)))
    if ([IO.File]::Exists($path)) { [IO.File]::Replace($temporary, $path, [NullString]::Value) }
    else { [IO.File]::Move($temporary, $path) }
}

function Get-FontFingerprint([string] $path) {
    $attributes = Get-PathAttributes $path
    if ($null -eq $attributes) { return $null }
    if ($attributes -band ([IO.FileAttributes]::ReparsePoint -bor [IO.FileAttributes]::Directory)) {
        return 'other'
    }
    return [TundraNativeFont]::Fingerprint($path, [NullString]::Value)
}

function Read-FontOwnership([string] $recordPath, [string] $target) {
    Assert-PlainPath $recordPath
    if ($null -eq (Get-PathAttributes $recordPath)) { return $null }
    $record = [IO.File]::ReadAllText($recordPath) | ConvertFrom-Json
    if ($record.target -ine $target -or $record.staged -cnotmatch '^stage-[0-9a-f]{32}$' -or
        $record.fingerprint -cnotmatch '^[0-9A-F]{40}:[0-9A-F]{64}$' -or $record.ready -isnot [bool]) {
        throw 'Invalid cell icon font ownership record'
    }
    return $record
}

function Get-FontRegistryEntries($userKey, $machineKey, [string] $fonts, [string] $systemFonts, [string] $target) {
    foreach ($source in @(@{ key = $userKey; dir = $fonts; user = $true },
                          @{ key = $machineKey; dir = $systemFonts; user = $false })) {
        if ($null -eq $source.key) { continue }
        foreach ($name in $source.key.GetValueNames()) {
            $raw = $source.key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            $path = $null
            if ($raw -is [string] -and $raw) {
                $path = [Environment]::ExpandEnvironmentVariables($raw)
                try {
                    if (![IO.Path]::IsPathRooted($path)) { $path = Join-Path $source.dir $path }
                    $path = [IO.Path]::GetFullPath($path)
                } catch [ArgumentException], [NotSupportedException] { $path = $null }
            }
            if ($path -ieq $target -or $name -match '^Tundra Cell Atlas(?:$|[ (])') {
                [PSCustomObject]@{ name = $name; path = $path; user = $source.user; raw = $raw;
                    kind = $source.key.GetValueKind($name) }
            }
        }
    }
}

function Invoke-CellIconFont([string] $owner, [string] $fonts, [string] $systemFonts,
                            $userKey, $machineKey, [byte[]] $font, [bool] $notify) {
    $target = Join-Path $fonts $fontName
    $recordPath = Join-Path $owner 'ownership.json'
    $install = ($font.Length -gt 0)
    Assert-PlainPath $owner
    if (!$install -and ![IO.Directory]::Exists($owner)) { return 'missing' }
    $lock = Open-FontLock $owner
    try {
        $record = Read-FontOwnership $recordPath $target
        if (!$install -and $null -eq $record) { return 'missing' }
        Assert-PlainPath $fonts
        Initialize-NativeFontApi
        $current = Get-FontFingerprint $target
        $slot = $null
        $other = @()
        foreach ($entry in @(Get-FontRegistryEntries $userKey $machineKey $fonts $systemFonts $target)) {
            if ($entry.user -and $entry.name -ieq $fontValue) { $slot = $entry }
            else { $other += $entry }
        }
        $ownedSlot = $null -ne $record -and $null -ne $slot -and
            $slot.kind -eq [Microsoft.Win32.RegistryValueKind]::String -and $slot.raw -ieq $target

        if (!$install) {
            if (($null -ne $current -and $current -cne $record.fingerprint) -or
                ($null -ne $slot -and (!$ownedSlot -or $null -eq $current)) -or
                ($other | Where-Object { $_.path -ieq $target })) { return 'preserved' }

            $stage = Join-Path $owner $record.staged
            $staged = Get-FontFingerprint $stage
            if ($null -ne $staged -and $staged -cne $record.fingerprint) { return 'preserved' }

            if ($ownedSlot) {
                if ($userKey.GetValueKind($fontValue) -ne [Microsoft.Win32.RegistryValueKind]::String -or
                    $userKey.GetValue($fontValue) -ine $target) { return 'preserved' }
                $userKey.DeleteValue($fontValue, $false)
                $userKey.Flush()
            }
            if ($null -ne $current) {
                if ($notify) {
                    while ([TundraNativeFont]::RemoveFontResource($target)) {}
                    [TundraNativeFont]::Notify()
                }
                if ([TundraNativeFont]::Fingerprint($target, $record.fingerprint) -cne $record.fingerprint) {
                    return 'preserved'
                }
            }
            if ($null -ne $staged -and
                [TundraNativeFont]::Fingerprint($stage, $record.fingerprint) -cne $record.fingerprint) {
                return 'preserved'
            }
            [IO.File]::Delete($recordPath)
            return 'removed'
        }

        $sha = Get-FontHash $font
        if ($null -ne $record -and $null -ne $current -and $current -cne $record.fingerprint) {
            return 'conflict'
        }
        $available = $false
        if ($null -ne $slot -and !$ownedSlot) { $other += $slot }
        foreach ($entry in $other) {
            $manual = if ($entry.path) { Get-FontFingerprint $entry.path }
            if (!$manual -or !$manual.EndsWith(":$sha")) { return 'conflict' }
            $available = $true
        }
        if ($available) { return 'available' }
        if ($null -ne $current -and ($null -eq $record -or !$current.EndsWith(":$sha"))) { return 'conflict' }
        if ($ownedSlot -and $null -eq $current) { return 'conflict' }
        if ($ownedSlot -and $record.ready) { return 'available' }

        if ($null -eq $current) {
            if ($null -ne $record) {
                $stage = Join-Path $owner $record.staged
                $staged = Get-FontFingerprint $stage
                if ($null -ne $staged -and ($staged -cne $record.fingerprint -or !$staged.EndsWith(":$sha"))) {
                    return 'conflict'
                }
                if ($null -eq $staged) { $record = $null }
            }
            if ($null -eq $record) {
                $staged = 'stage-' + [Guid]::NewGuid().ToString('N')
                $stage = Join-Path $owner $staged
                Write-FontFile $stage $font
                $record = [PSCustomObject]@{ target = $target; staged = $staged;
                    fingerprint = (Get-FontFingerprint $stage); ready = $false }
                Save-FontOwnership $record $recordPath
            }
            [void][IO.Directory]::CreateDirectory($fonts)
            [IO.File]::Move($stage, $target)
        }
        if ((Get-FontFingerprint $target) -cne $record.fingerprint) { return 'conflict' }
        if (!$ownedSlot) {
            if ($null -ne $userKey.GetValue($fontValue)) { return 'conflict' }
            $userKey.SetValue($fontValue, $target, [Microsoft.Win32.RegistryValueKind]::String)
            $userKey.Flush()
        }
        if ($notify) {
            if ([TundraNativeFont]::AddFontResource($target) -eq 0) { throw 'Windows could not load the registered font' }
            [TundraNativeFont]::Notify()
        }
        $record.ready = $true
        Save-FontOwnership $record $recordPath
        return 'installed'
    } finally {
        $lock.Dispose()
    }
}

function Invoke-CurrentUserFont([byte[]] $font) {
    $local = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    $system = [Environment]::GetFolderPath([Environment+SpecialFolder]::Fonts)
    if (!$local -or !$system) { throw 'Cannot locate Windows font directories' }
    $owner = Join-Path $local 'TundraUX\cell-icon-font'
    $fonts = Join-Path $local 'Microsoft\Windows\Fonts'
    if ($font.Length -gt 0) { $user = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($fontKey) }
    else { $user = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($fontKey, $true) }
    $machine = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($fontKey)
    try { Invoke-CellIconFont $owner $fonts $system $user $machine $font $true }
    finally {
        if ($user) { $user.Dispose() }
        if ($machine) { $machine.Dispose() }
    }
}

function Install-CurrentUserFont([byte[]] $font) { Invoke-CurrentUserFont $font }
function Remove-CurrentUserFont { Invoke-CurrentUserFont ([byte[]]::new(0)) }
