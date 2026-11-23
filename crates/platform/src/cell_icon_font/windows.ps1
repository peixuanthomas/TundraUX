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
    [DllImport("dwrite.dll")]
    static extern int DWriteCreateFactory(uint type, ref Guid iid, out IntPtr factory);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    delegate int GetSystemFontCollection(IntPtr factory, out IntPtr collection,
        [MarshalAs(UnmanagedType.Bool)] bool checkForUpdates);

    public static void Notify() {
        SendNotifyMessage(new IntPtr(0xffff), 0x001d, IntPtr.Zero, IntPtr.Zero);
    }

    public static void RefreshSystemFonts() {
        var iid = new Guid("b859ee5a-d838-4b5b-a2e8-1adc7d93db48");
        IntPtr factory;
        Marshal.ThrowExceptionForHR(DWriteCreateFactory(0, ref iid, out factory));
        try {
            // GetSystemFontCollection follows the three IUnknown methods.
            IntPtr method = Marshal.ReadIntPtr(Marshal.ReadIntPtr(factory), 3 * IntPtr.Size);
            var getFonts = (GetSystemFontCollection)Marshal.GetDelegateForFunctionPointer(
                method, typeof(GetSystemFontCollection));
            IntPtr collection;
            Marshal.ThrowExceptionForHR(getFonts(factory, out collection, true));
            Marshal.Release(collection);
        } finally {
            Marshal.Release(factory);
        }
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

function Get-FontVersionPath([string] $fonts, [string] $hash) {
    $suffix = $hash.Substring(0, 16).ToLowerInvariant()
    return Join-Path $fonts "TundraCellAtlas-$suffix.ttf"
}

function Test-FontReceipt($copy, [string] $fonts, [bool] $allowLegacy) {
    if ($copy.staged -cnotmatch '^stage-[0-9a-f]{32}$' -or
        $copy.fingerprint -cnotmatch '^[0-9A-F]{40}:[0-9A-F]{64}$' -or $copy.ready -isnot [bool]) {
        return $false
    }
    $versioned = Get-FontVersionPath $fonts $copy.fingerprint.Split(':')[1]
    return $copy.target -ieq $versioned -or ($allowLegacy -and $copy.target -ieq (Join-Path $fonts $fontName))
}

function Read-FontOwnership([string] $recordPath, [string] $fonts) {
    Assert-PlainPath $recordPath
    if ($null -eq (Get-PathAttributes $recordPath)) { return $null }
    $record = [IO.File]::ReadAllText($recordPath) | ConvertFrom-Json
    if (!(Test-FontReceipt $record $fonts $true)) {
        throw 'Invalid cell icon font ownership record'
    }
    if ($null -ne $record.upgrade -and (!(Test-FontReceipt $record.upgrade $fonts $false) -or
        $record.upgrade.staged -ieq $record.staged -or $record.upgrade.target -ieq $record.target)) {
        throw 'Invalid cell icon font upgrade record'
    }
    return $record
}

function New-FontStage([string] $owner, [string] $target, [byte[]] $font) {
    $name = 'stage-' + [Guid]::NewGuid().ToString('N')
    $path = Join-Path $owner $name
    Write-FontFile $path $font
    return [PSCustomObject]@{ target = $target; staged = $name;
        fingerprint = (Get-FontFingerprint $path); ready = $false }
}

function Remove-RecordedFontFile([string] $path, [string] $fingerprint) {
    if ($null -eq (Get-FontFingerprint $path)) { return $true }
    return [TundraNativeFont]::Fingerprint($path, $fingerprint) -ceq $fingerprint
}

function Remove-LoadedFont([string] $target, [string] $fingerprint, [bool] $notify) {
    if ($notify) {
        while ([TundraNativeFont]::RemoveFontResource($target)) {}
        [TundraNativeFont]::Notify()
    }
    return Remove-RecordedFontFile $target $fingerprint
}

function Get-FontRegistryEntries($userKey, $machineKey, [string] $fonts, [string] $systemFonts, [string[]] $targets) {
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
            if ($path -in $targets -or $name -match '^Tundra Cell Atlas(?:$|[ (])') {
                [PSCustomObject]@{ name = $name; path = $path; user = $source.user; raw = $raw;
                    kind = $source.key.GetValueKind($name) }
            }
        }
    }
}

function Read-FontCopyState($copy, [string] $owner) {
    $stage = Join-Path $owner $copy.staged
    $published = Get-FontFingerprint $copy.target
    $prepared = Get-FontFingerprint $stage
    return [PSCustomObject]@{
        receipt = $copy; stage = $stage; published = $null -ne $published; prepared = $null -ne $prepared
        changed = ($null -ne $published -and $published -cne $copy.fingerprint) -or
            ($null -ne $prepared -and $prepared -cne $copy.fingerprint)
    }
}

function Test-FontRegistryValue($userKey, [string] $expected) {
    if (!$userKey) { return !$expected }
    $actual = $userKey.GetValue($fontValue, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    if (!$expected) { return $null -eq $actual }
    return $actual -ieq $expected -and $userKey.GetValueKind($fontValue) -eq [Microsoft.Win32.RegistryValueKind]::String
}

function Remove-FontCopy($copy, [bool] $notify) {
    if ($copy.published -and !(Remove-LoadedFont $copy.receipt.target $copy.receipt.fingerprint $notify)) {
        return $false
    }
    return Remove-RecordedFontFile $copy.stage $copy.receipt.fingerprint
}

function Invoke-CellIconFont([string] $owner, [string] $fonts, [string] $systemFonts,
                            $userKey, $machineKey, [byte[]] $font, [bool] $notify) {
    $recordPath = Join-Path $owner 'ownership.json'
    $legacy = Join-Path $fonts $fontName
    $install = ($font.Length -gt 0)
    Assert-PlainPath $owner
    if (!$install -and ![IO.Directory]::Exists($owner)) { return 'missing' }
    $lock = Open-FontLock $owner
    try {
        $record = Read-FontOwnership $recordPath $fonts
        if (!$install -and $null -eq $record) { return 'missing' }
        Assert-PlainPath $fonts
        Initialize-NativeFontApi
        $active = if ($record) { Read-FontCopyState $record $owner }
        $pending = if ($record.upgrade) { Read-FontCopyState $record.upgrade $owner }
        $copies = @($active, $pending) | Where-Object { $null -ne $_ }
        if ($copies | Where-Object changed) {
            if ($install) { return 'conflict' }
            return 'preserved'
        }

        $copyTargets = @($copies | ForEach-Object { $_.receipt.target })
        $watched = @($legacy) + $copyTargets
        if ($install) {
            $sha = Get-FontHash $font
            $target = Get-FontVersionPath $fonts $sha
            $watched += $target
        }

        $slot = $null
        $other = @()
        foreach ($entry in @(Get-FontRegistryEntries $userKey $machineKey $fonts $systemFonts $watched)) {
            if ($entry.user -and $entry.name -ieq $fontValue) { $slot = $entry }
            else { $other += $entry }
        }
        $registered = if ($slot.kind -eq [Microsoft.Win32.RegistryValueKind]::String) {
            $copies | Where-Object { $_.receipt.target -ieq $slot.raw } | Select-Object -First 1
        }
        $slotPath = if ($registered) { $registered.receipt.target }
        $aliases = $other | Where-Object { $_.path -in $copyTargets }

        if (!$install) {
            if (($slot -and !$registered.published) -or $aliases) { return 'preserved' }
            if ($registered) {
                if (!(Test-FontRegistryValue $userKey $slotPath)) { return 'preserved' }
                $userKey.DeleteValue($fontValue, $false)
                $userKey.Flush()
            }
            foreach ($copy in $copies) {
                if (!(Remove-FontCopy $copy $notify)) { return 'preserved' }
            }
            [IO.File]::Delete($recordPath)
            return 'removed'
        }

        $manualAvailable = $false
        if ($slot -and !$registered) { $other += $slot }
        foreach ($entry in $other) {
            $manual = if ($entry.path) { Get-FontFingerprint $entry.path }
            if (!$manual -or !$manual.EndsWith(":$sha")) { return 'conflict' }
            $manualAvailable = $true
        }
        if (($slot -and !$registered) -or ($manualAvailable -and !$record)) { return 'available' }
        if (($registered -and !$registered.published) -or $aliases) { return 'conflict' }
        if ($pending -and $pending.receipt.target -ine $target) { return 'conflict' }
        if ($target -notin $copyTargets -and $null -ne (Get-PathAttributes $target)) { return 'conflict' }
        if (!$record -and $null -ne (Get-PathAttributes $legacy)) { return 'conflict' }

        $candidate = if ($pending) { $pending } elseif ($record.target -ieq $target) { $active }
        if (!$pending -and $candidate.published -and $candidate.receipt.ready -and $slotPath -ieq $target) {
            return 'available'
        }
        $migrating = $record -and $record.target -ine $target
        if (!$candidate.published -and !$candidate.prepared) {
            $next = New-FontStage $owner $target $font
            if ($migrating) { $record | Add-Member NoteProperty upgrade $next -Force }
            else { $record = $next }
            Save-FontOwnership $record $recordPath
            $candidate = Read-FontCopyState $next $owner
        }
        $next = $candidate.receipt
        if (!$candidate.published) {
            [void][IO.Directory]::CreateDirectory($fonts)
            [IO.File]::Move($candidate.stage, $target)
        }
        if ((Get-FontFingerprint $target) -cne $next.fingerprint -or
            !(Test-FontRegistryValue $userKey $slotPath)) { return 'conflict' }
        if ($slotPath -ine $target) {
            $userKey.SetValue($fontValue, $target, [Microsoft.Win32.RegistryValueKind]::String)
            $userKey.Flush()
        }
        if ($notify -and (!$next.ready -or $slotPath -ine $target)) {
            if ($migrating -and $active.published -and
                (Get-FontFingerprint $active.receipt.target) -ceq $active.receipt.fingerprint) {
                while ([TundraNativeFont]::RemoveFontResource($active.receipt.target)) {}
            }
            if ([TundraNativeFont]::AddFontResource($target) -eq 0) { throw 'Windows could not load the registered font' }
            [TundraNativeFont]::RefreshSystemFonts()
            [TundraNativeFont]::Notify()
        }
        $next.ready = $true
        Save-FontOwnership $record $recordPath
        if ($migrating) {
            if (!(Remove-FontCopy $active $notify)) { return 'conflict' }
            Save-FontOwnership $next $recordPath
        }
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
