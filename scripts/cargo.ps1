$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
$bundledCompiler = Get-ChildItem -LiteralPath (Join-Path $workspace '.tools') -Directory -Filter 'mingw64' -ErrorAction SilentlyContinue | Select-Object -First 1
if ($bundledCompiler) {
    # GCC's built-in linker specs cannot quote its installation prefix reliably.
    $compilerLink = Join-Path $env:USERPROFILE '.cargo/oxy-native-toolchain'
    if (!(Test-Path -LiteralPath (Join-Path $compilerLink 'bin/gcc.exe'))) { Copy-Item -LiteralPath $bundledCompiler.FullName -Destination $compilerLink -Recurse }
    $env:PATH = "$compilerLink\bin;$env:PATH"
}
$cargoExe = Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe'
if (!(Test-Path -LiteralPath $cargoExe)) { $cargoExe = 'cargo' }
& $cargoExe @args
exit $LASTEXITCODE
