param(
    # The `build` folder of the profile that compiled mlua-sys, as target/debug/build.
    [Parameter(Mandatory = $true)][string]$BuildDirectory
)
# Luau (mlua-sys) raises errors as C++ exceptions, and cc passes no /EH flag to MSVC: the CI sets
# LUAU_CXXFLAGS=/EHsc. luau0-src compiles with -W0, which hides the C4530 warning a build without
# /EH would give, so the compiler commands cc writes to the build script output (with
# CC_ENABLE_DEBUG_OUTPUT=1) are the only visible check. Each C++ file luau0-src compiles (its src
# folders, without CodeGen, which mlua builds only for the JIT) must appear with /EHsc.
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
$libraries = @('VM', 'Ast', 'Common', 'Bytecode', 'Inliner', 'Compiler', 'Config', 'Custom', 'Require')
# luau0-src 0.22.0+luau740 compiles 75 files; another version must be checked again by hand.
$expectedCount = 75

$lock = Get-Content -LiteralPath (Join-Path $workspace 'Cargo.lock') -Raw
$match = [regex]::Match($lock, 'name = "luau0-src"\r?\nversion = "([^"]+)"')
if (!$match.Success) { throw 'luau0-src não está no Cargo.lock.' }
$package = "luau0-src-$($match.Groups[1].Value)"
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$source = Get-ChildItem -LiteralPath (Join-Path $cargoHome 'registry/src') -Directory |
    ForEach-Object { Join-Path $_.FullName $package } |
    Where-Object { Test-Path -LiteralPath $_ } |
    Select-Object -First 1
if (!$source) { throw "Fontes de $package não encontradas no registro do cargo." }

$expected = foreach ($library in $libraries) {
    Get-ChildItem -LiteralPath (Join-Path $source "luau/$library/src") -Filter '*.cpp' |
        ForEach-Object { "\luau\$library\src\$($_.Name)" }
}
if ($expected.Count -ne $expectedCount) {
    throw "luau0-src tem $($expected.Count) arquivos C++, não $expectedCount; confira a lista."
}

$outputs = Get-ChildItem -LiteralPath $BuildDirectory -Directory -Filter 'mlua-sys-*' |
    ForEach-Object { Join-Path $_.FullName 'output' } |
    Where-Object { Test-Path -LiteralPath $_ }
if (!$outputs) { throw "Nenhuma saída do script de build do mlua-sys em $BuildDirectory." }
# cc writes each command quoted, with doubled backslashes.
$commands = $outputs | ForEach-Object { Get-Content -LiteralPath $_ } | Where-Object { $_ -match '"-c"' }

$missing = @()
$without = @()
foreach ($file in $expected) {
    $compiled = @($commands | Where-Object { $_.Replace('\\', '\').Replace('/', '\').Contains("$file`"") })
    if ($compiled.Count -eq 0) { $missing += $file }
    elseif (@($compiled | Where-Object { $_ -notmatch '(^|\s|")/EHsc("|\s|$)' }).Count -gt 0) { $without += $file }
}
$good = $expected.Count - $missing.Count - $without.Count
Write-Output "Luau: $($expected.Count) arquivos C++ esperados, $good compilados com /EHsc."
if ($missing) { Write-Output "Sem comando de compilação: $($missing -join ', ')" }
if ($without) { Write-Output "Compilados sem /EHsc: $($without -join ', ')" }
if ($missing -or $without) { throw 'Luau não foi compilado inteiro com /EHsc.' }
