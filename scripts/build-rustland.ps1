# Builds the Rustland R++ IDE (crates/rustland) for the 486 and packs it into
# dos/rustland/RUSTLAND.EXE, which the emulator installs on drive C:.
#
# Needs the i586 target: rustup target add i586-unknown-linux-gnu
$ErrorActionPreference = "Continue"  # cargo writes progress to stderr
$root = Split-Path -Parent $PSScriptRoot
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
if (-not (Test-Path $cargo)) { $cargo = "cargo" }

Push-Location (Join-Path $root "crates\rustland")
try {
    & $cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "building rustland failed" }
} finally {
    Pop-Location
}

$elf = Join-Path $root "crates\rustland\target\i586-unknown-linux-gnu\release\rustland"
$exe = Join-Path $root "dos\rustland\RUSTLAND.EXE"
Push-Location $root
try {
    # Heap up to 8 MB: programs run from the IDE load at 8 MB.
    & $cargo run --quiet --release -p rusty486-lang --bin pmpack -- $elf $exe 800000
    if ($LASTEXITCODE -ne 0) { throw "pmpack failed" }
} finally {
    Pop-Location
}
