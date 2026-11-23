class TestFontRegistry {
    [hashtable] $values = @{}
    [bool] $denyWrite

    [string[]] GetValueNames() { return [string[]]$this.values.Keys }
    [object] GetValue([string] $name) { return $this.values[$name] }
    [object] GetValue([string] $name, [object] $default, [object] $options) {
        if (!$this.values.ContainsKey($name)) { return $default }
        return $this.values[$name]
    }
    [object] GetValueKind([string] $name) { return [Microsoft.Win32.RegistryValueKind]::String }
    [void] SetValue([string] $name, [object] $value, [object] $kind) {
        if ($this.denyWrite) { throw 'test registration failure' }
        $this.values[$name] = $value
    }
    [void] DeleteValue([string] $name, [bool] $throwOnMissing) { $this.values.Remove($name) }
    [void] Flush() {}
}

function Assert-Equal($actual, $expected) {
    if ($actual -cne $expected) { throw "Expected '$expected', got '$actual'" }
}

$owner = Join-Path $env:TUNDRA_FONT_TEST_ROOT 'owner'
$fonts = Join-Path $env:TUNDRA_FONT_TEST_ROOT 'fonts'
$systemFonts = Join-Path $env:TUNDRA_FONT_TEST_ROOT 'system-fonts'
$target = Get-FontVersionPath $fonts (Get-FontHash $font)
$record = Join-Path $owner 'ownership.json'
$manual = Join-Path $fonts 'manually-installed.ttf'
$registry = [TestFontRegistry]::new()
$empty = [byte[]]::new(0)

function Install-TestFont([byte[]] $version = $font) {
    Invoke-CellIconFont $owner $fonts $systemFonts $registry $null $version $false
}
function Remove-TestFont { Invoke-CellIconFont $owner $fonts $systemFonts $registry $null $empty $false }

Assert-Equal (Remove-TestFont) 'missing'
Assert-Equal (Install-TestFont) 'installed'
Assert-Equal (Install-TestFont) 'available'
Assert-Equal ($registry.GetValue($fontValue)) $target
Assert-Equal (Remove-TestFont) 'removed'
Assert-Equal ([IO.File]::Exists($target)) $false
Assert-Equal ($registry.GetValue($fontValue)) $null
Assert-Equal (Remove-TestFont) 'missing'

$newFont = [Text.Encoding]::ASCII.GetBytes('updated bundled font')
$newTarget = Get-FontVersionPath $fonts (Get-FontHash $newFont)
Assert-Equal (Install-TestFont) 'installed'
$oldFingerprint = Get-FontFingerprint $target
$lock = [IO.File]::Open($target, 'Open', 'Read', 'Read')
$failed = $false
try { Install-TestFont $newFont } catch { $failed = $true } finally { $lock.Dispose() }
Assert-Equal $failed $true
Assert-Equal (Get-FontFingerprint $target) $oldFingerprint
Assert-Equal ($registry.GetValue($fontValue)) $newTarget
Assert-Equal (Install-TestFont $newFont) 'installed'
Assert-Equal ([IO.File]::Exists($target)) $false
Assert-Equal (Get-FontHash ([IO.File]::ReadAllBytes($newTarget))) (Get-FontHash $newFont)
Assert-Equal (Install-TestFont $newFont) 'available'
Assert-Equal (Remove-TestFont) 'removed'

Assert-Equal (Install-TestFont) 'installed'
$ownership = Read-FontOwnership $record $fonts
$legacy = Join-Path $fonts $fontName
[IO.File]::Move($target, $legacy)
$ownership.target = $legacy
$recordLock = [IO.File]::Open($record, 'Open', 'Read', 'ReadWrite')
$script:recordRetryDelay = 0
function Start-Sleep([int] $Milliseconds) {
    $script:recordRetryDelay += $Milliseconds
    $recordLock.Dispose()
    Microsoft.PowerShell.Utility\Start-Sleep -Milliseconds $Milliseconds
}
try {
    Save-FontOwnership $ownership $record
    Assert-Equal ($recordRetryDelay -gt 0) $true
    Assert-Equal (Read-FontOwnership $record $fonts).target $legacy
} finally {
    $recordLock.Dispose()
    Remove-Item -LiteralPath Function:Start-Sleep
}
$registry.values[$fontValue] = $legacy
Assert-Equal (Install-TestFont) 'installed'
Assert-Equal ($registry.GetValue($fontValue)) $target
Assert-Equal ([IO.File]::Exists($legacy)) $false
Assert-Equal (Remove-TestFont) 'removed'

Assert-Equal (Install-TestFont) 'installed'
$ownership = Read-FontOwnership $record $fonts
$ownership | Add-Member NoteProperty upgrade (New-FontStage $owner $newTarget $newFont)
Save-FontOwnership $ownership $record
$upgrade = Join-Path $owner $ownership.upgrade.staged
Assert-Equal (Remove-TestFont) 'removed'
Assert-Equal ([IO.File]::Exists($target)) $false
Assert-Equal ([IO.File]::Exists($upgrade)) $false
Assert-Equal ($registry.GetValue($fontValue)) $null

[IO.File]::WriteAllBytes($manual, $font)
$registry.values[$fontValue] = $manual
Assert-Equal (Install-TestFont) 'available'
Assert-Equal (Install-TestFont $newFont) 'conflict'
Assert-Equal (Remove-TestFont) 'missing'
Assert-Equal ([IO.File]::Exists($manual)) $true
[IO.File]::WriteAllText($manual, 'different version')
Assert-Equal (Install-TestFont) 'conflict'
$registry.values.Clear()

$registry.denyWrite = $true
$failed = $false
try { Install-TestFont } catch { $failed = $true }
Assert-Equal $failed $true
Assert-Equal ([IO.File]::Exists($record)) $true
$registry.denyWrite = $false
Assert-Equal (Install-TestFont) 'installed'

$lock = [IO.File]::Open($target, 'Open', 'Read', 'Read')
$failed = $false
try { Remove-TestFont } catch { $failed = $true } finally { $lock.Dispose() }
Assert-Equal $failed $true
Assert-Equal ([IO.File]::Exists($record)) $true
Assert-Equal (Remove-TestFont) 'removed'

Assert-Equal (Install-TestFont) 'installed'
$stream = [IO.File]::Open($target, 'Open', 'Read', 'Read, Delete')
$mapping = [IO.MemoryMappedFiles.MemoryMappedFile]::CreateFromFile(
    $stream, [NullString]::Value, 0, 'Read', 'None', $false)
$view = $mapping.CreateViewAccessor(0, 0, 'Read')
try {
    Assert-Equal (Remove-TestFont) 'removed'
    Assert-Equal ([IO.File]::Exists($target)) $false
    Assert-Equal ($view.ReadByte(0)) $font[0]
} finally {
    $view.Dispose()
    $mapping.Dispose()
}

Assert-Equal (Install-TestFont) 'installed'
$registry.values[$fontValue] = $manual
Assert-Equal (Remove-TestFont) 'preserved'
Assert-Equal ($registry.GetValue($fontValue)) $manual
Assert-Equal ([IO.File]::Exists($target)) $true

$registry.values[$fontValue] = $target
[IO.File]::Move($target, (Join-Path $fonts 'original.ttf'))
[IO.File]::WriteAllBytes($target, $font)
Assert-Equal (Install-TestFont) 'conflict'
Assert-Equal (Install-TestFont $newFont) 'conflict'
Assert-Equal (Remove-TestFont) 'preserved'
Assert-Equal ([IO.File]::Exists($target)) $true
Assert-Equal ($registry.GetValue($fontValue)) $target
'passed'
