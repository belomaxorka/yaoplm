# Finds the test data folder and lists the volumes with their serial
# numbers. The folder is yaoplm-testdata at the root of whichever drive
# holds it, and drive letters change with what is plugged in, so it is found
# by name; a test stick is known by its volume serial, never by its letter.
# Only <letter>:\yaoplm-testdata is looked at: the rest of that drive is the
# owner's and is never listed. Reads only.
#
#   . scripts/testdata.ps1                     # PowerShell: sets YAOPLM_TESTDATA and YAOPLM_OPL_SRC in this shell
#   eval "$(powershell -NoProfile -File scripts/testdata.ps1 -Bash)"   # Git Bash: the same as exports

param([switch]$Bash)

$found = @(Get-PSDrive -PSProvider FileSystem | ForEach-Object { Join-Path $_.Root 'yaoplm-testdata' } |
    Where-Object { Test-Path -LiteralPath $_ -PathType Container })
if ($found.Count -eq 0) { throw 'No yaoplm-testdata folder at the root of any drive: is the test data drive plugged in?' }
if ($found.Count -gt 1) { throw "More than one test data folder: $($found -join ', '). Unplug the extra drive or set YAOPLM_TESTDATA by hand." }

$vars = [ordered]@{ YAOPLM_TESTDATA = $found[0] }
$opl = Join-Path $found[0] 'refs\ps2homebrew__Open-PS2-Loader'
if (Test-Path -LiteralPath $opl) { $vars.YAOPLM_OPL_SRC = $opl }

if ($Bash) {
    foreach ($name in $vars.Keys) { "export $name='$($vars[$name])'" }
    return
}

"== Test data"
foreach ($name in $vars.Keys) {
    Set-Item "env:$name" $vars[$name]
    "{0}={1}" -f $name, $vars[$name]
}

"== Volumes"
$kinds = @{ 2 = 'removable'; 3 = 'fixed'; 4 = 'network'; 5 = 'optical' }
Get-CimInstance Win32_LogicalDisk | ForEach-Object {
    $serial = if ($_.VolumeSerialNumber) { $_.VolumeSerialNumber.Insert(4, '-') } else { '-' }
    $size = if ($_.Size) { '{0:N1} GB' -f ($_.Size / 1GB) } else { '-' }
    "{0}  {1,-9}  {2,-6}  {3,-9}  {4,10}  {5}" -f $_.DeviceID, $serial, $_.FileSystem, $kinds[[int]$_.DriveType], $size, $_.VolumeName
}
