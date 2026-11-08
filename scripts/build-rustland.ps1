# Builds the Rustland R++ IDE (crates/rustland) for the 486 and packs it into
# dos/rustland/RUSTLAND.EXE, compiles R++ Write into dos/write/WRITE.EXE and Text into
# dos/text/TEXT.EXE. The emulator installs them on drive C:.
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
    # Heap up to 32 MB: programs run from the IDE load at 32 MB.
    & $cargo run --quiet --release -p rusty486-lang --bin pmpack -- $elf $exe 2000000
    if ($LASTEXITCODE -ne 0) { throw "pmpack failed" }
    # R++ Write, the word processor in C:\WRITE.
    & $cargo run --quiet --release -p rusty486-lang --bin rppc -- dos/rustland/examples/WRITE.RS dos/write/WRITE.EXE
    if ($LASTEXITCODE -ne 0) { throw "compiling WRITE.RS failed" }
    # Text, the plain text editor in C:\TEXT.
    & $cargo run --quiet --release -p rusty486-lang --bin rppc -- dos/rustland/examples/TEXT.RS dos/text/TEXT.EXE
    if ($LASTEXITCODE -ne 0) { throw "compiling TEXT.RS failed" }
} finally {
    Pop-Location
}
