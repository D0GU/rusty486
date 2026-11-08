# Fetches and assembles the test386.asm CPU test ROM into testdata/test386
# (needs git and NASM). Then: cargo test -p rusty486-core --release --test test386
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-test386.ps1

$ErrorActionPreference = 'Stop'
$dir = Join-Path $PSScriptRoot '..\testdata\test386'
if (-not (Test-Path $dir)) {
    git clone -q --depth 1 https://github.com/barotto/test386.asm $dir
}
$cfg = Join-Path $dir 'src\configuration.asm'
# Send the arithmetic results to rusty486's debug port.
(Get-Content $cfg) -replace '^OUT_PORT equ 0$', 'OUT_PORT equ 0xE9' | Set-Content $cfg -Encoding ascii
Push-Location $dir
nasm -i./src/ -f bin src/test386.asm -l test386.lst -o test386.bin
Pop-Location
Write-Host "Built $dir\test386.bin"
