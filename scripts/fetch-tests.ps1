# Downloads the SingleStepTests 80386 real-mode CPU test suite (~576 MB)
# into testdata/386. The test runner skips gracefully if this hasn't been run.
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-tests.ps1

$ErrorActionPreference = 'Stop'
$repo = 'SingleStepTests/80386'
$raw = "https://raw.githubusercontent.com/$repo/main"
New-Item -ItemType Directory -Force (Join-Path $PSScriptRoot '..\testdata\386\real_mode') | Out-Null
# curl config files treat backslashes as escapes, so use forward slashes.
$dest = (Resolve-Path (Join-Path $PSScriptRoot '..\testdata\386')).Path -replace '\\', '/'

Write-Host 'Listing test files...'
$files = Invoke-RestMethod "https://api.github.com/repos/$repo/contents/v1_ex_real_mode"

# Build a curl config so all files download in parallel and existing ones are skipped.
$cfg = New-TemporaryFile
$lines = @(
    "url = `"$raw/80386.csv`"", "output = `"$dest/80386.csv`"",
    "url = `"$raw/revocation_list.txt`"", "output = `"$dest/revocation_list.txt`""
)
$todo = 0
foreach ($f in $files) {
    $out = "$dest/real_mode/$($f.name)"
    if ((Test-Path $out) -and (Get-Item $out).Length -eq $f.size) { continue }
    $lines += "url = `"$raw/v1_ex_real_mode/$($f.name)`""
    $lines += "output = `"$out`""
    $todo++
}
Set-Content -Path $cfg -Value $lines -Encoding ascii
Write-Host "Downloading $todo of $($files.Count) test files..."
curl.exe --fail --silent --show-error --location --parallel --parallel-max 16 --config $cfg
$code = $LASTEXITCODE
Remove-Item $cfg
$missing = @($files | Where-Object { -not (Test-Path "$dest/real_mode/$($_.name)") }).Count
if ($code -ne 0 -or $missing -gt 0) { throw "Download incomplete: curl exit $code, $missing files missing. Re-run to resume." }
Write-Host "Done. Run: cargo test -p rusty486-core --release --test singlestep -- --nocapture"
