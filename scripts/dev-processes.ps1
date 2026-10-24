# Lists the YAOPLM processes and ports that can get in the way of a build or
# a run on this machine: running exes lock target\debug\*.exe (cargo then
# fails with "Access is denied", os error 5), a desktop app holds its
# WebView2 profile (another instance fails with 0x8007139F), and dev ports
# (Vite 5173, servers 7878-7880, CDP 9222) can be taken. Reads only.
#
#   powershell -NoProfile -File scripts/dev-processes.ps1

$ports = 5173, 7878, 7879, 7880, 9222

function Show-Chain([int]$id) {
    $names = @()
    while ($id) {
        $p = Get-CimInstance Win32_Process -Filter "ProcessId=$id" -ErrorAction SilentlyContinue
        if (-not $p) { break }
        $names += "$($p.Name)($($p.ProcessId))"
        $id = $p.ParentProcessId
    }
    $names -join " <- "
}

"== YAOPLM programs"
Get-CimInstance Win32_Process | Where-Object { $_.Name -match '^yaoplm' } | ForEach-Object {
    $kind = if ($_.ExecutablePath -like '*\AppData\Local\YAOPLM\*') { 'installed app (the owner''s: never stop it)' } else { 'dev build' }
    "{0,6}  {1}  {2}`n        {3}`n        started {4}; parents: {5}" -f $_.ProcessId, $_.Name, $kind, $_.ExecutablePath, $_.CreationDate, (Show-Chain $_.ParentProcessId)
}

"== WebView2 profiles of the app"
Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
    Where-Object { $_.CommandLine -match '--embedded-browser-webview' -and $_.CommandLine -match 'yaoplm' } | ForEach-Object {
        $dir = if ($_.CommandLine -match '--user-data-dir="?([^"]+?)"? --') { $Matches[1] } else { '?' }
        "{0,6}  profile {1}; parent {2}" -f $_.ProcessId, $dir, (Show-Chain $_.ParentProcessId)
    }

"== Dev ports"
Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $ports -contains $_.LocalPort } | ForEach-Object {
    $p = Get-CimInstance Win32_Process -Filter "ProcessId=$($_.OwningProcess)"
    "{0,6}  port {1}  {2}: {3}" -f $_.OwningProcess, $_.LocalPort, $p.Name, $p.CommandLine
}
