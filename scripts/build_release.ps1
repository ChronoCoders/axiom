$ErrorActionPreference = "Stop"

$Root = (Resolve-Path "$PSScriptRoot/..").Path
$OutDir = Join-Path $Root "dist"

if (Test-Path $OutDir) {
    Remove-Item -Recurse -Force $OutDir
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Push-Location $Root
cargo build --release -p axiom-node
if ($LASTEXITCODE -ne 0) { exit 1 }
cargo build --release -p fast-forward
if ($LASTEXITCODE -ne 0) { exit 1 }
cargo build --release -p genesis-tool
if ($LASTEXITCODE -ne 0) { exit 1 }
cargo build --release -p test-vector-gen
if ($LASTEXITCODE -ne 0) { exit 1 }
Pop-Location

Copy-Item "$Root/target/release/axiom-node.exe" $OutDir
Copy-Item "$Root/target/release/fast-forward.exe" $OutDir
Copy-Item "$Root/target/release/genesis-tool.exe" $OutDir
Copy-Item "$Root/target/release/test-vector-gen.exe" $OutDir

Get-ChildItem $OutDir | ForEach-Object {
    $h = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    "$h  $($_.Name)" | Add-Content (Join-Path $OutDir "SHA256SUMS.txt")
}
