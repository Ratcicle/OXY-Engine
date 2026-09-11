param([Parameter(Mandatory)][string]$Zip, [switch]$ContentOnly)
$ErrorActionPreference = 'Stop'
$zipPath = (Resolve-Path -LiteralPath $Zip).Path
$checksumPath = "$zipPath.sha256"
if (Test-Path -LiteralPath $checksumPath -PathType Leaf) {
    $checksum = (Get-Content -LiteralPath $checksumPath -Raw).Trim()
    if ($checksum -notmatch '^([0-9a-fA-F]{64})\s+\*?(.+)$') { throw 'Checksum SHA-256 inválido.' }
    $expectedHash = $Matches[1]
    $expectedName = $Matches[2]
    if ($expectedName -ne [IO.Path]::GetFileName($zipPath) -or (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash -ne $expectedHash) { throw 'ZIP diferente do checksum publicado; extração cancelada.' }
    Write-Host 'Checksum SHA-256 conferido.'
} else {
    Write-Warning 'Arquivo .zip.sha256 ausente; a integridade do ZIP não será verificada por checksum.'
}
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('OXY portable test ' + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $testRoot | Out-Null
Expand-Archive -LiteralPath $zipPath -DestinationPath $testRoot
$editors = @(Get-ChildItem -LiteralPath $testRoot -Recurse -Filter 'OXY Engine.exe')
if ($editors.Count -ne 1) { throw 'O ZIP deve conter exatamente um OXY Engine.exe.' }
$editor = $editors[0]
$package = $editor.DirectoryName
foreach ($file in @('oxy_player.exe','data/project.oxy.json','LEIA-ME.txt','LICENSE','VERSAO.txt')) {
    if (!(Test-Path -LiteralPath (Join-Path $package $file) -PathType Leaf)) { throw "Arquivo ausente: $file" }
}
if ([version]([Diagnostics.FileVersionInfo]::GetVersionInfo($editor.FullName).ProductVersion) -ge [version]'0.3.0') {
    foreach ($file in @('licenses/rapier-0.35.3.txt','licenses/parry-0.30.2.txt','laboratorio-3d/project.oxy.json')) {
        if (!(Test-Path -LiteralPath (Join-Path $package $file) -PathType Leaf)) { throw "Arquivo da 0.3.0 ausente: $file" }
    }
}
if (Get-ChildItem -LiteralPath $package -Filter '*.cmd') { throw 'O pacote portátil não deve depender de launchers .cmd.' }
$version = [Diagnostics.FileVersionInfo]::GetVersionInfo($editor.FullName)
if ($version.ProductName -ne 'OXY Engine' -or $version.OriginalFilename -ne 'OXY Engine.exe' -or !$version.ProductVersion) { throw 'Metadados do editor inválidos.' }
$versionMarker = (Get-Content -LiteralPath (Join-Path $package 'VERSAO.txt') -Raw).Trim()
if ($versionMarker -ne "OXY Engine $($version.ProductVersion)") { throw 'VERSAO.txt não corresponde ao executável incluído.' }
# Read PE headers/imports with .NET only: the validation script itself needs no Rust/SDK.
function Assert-PortablePe([string]$Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    $pe = [BitConverter]::ToInt32($bytes, 0x3c)
    if ([BitConverter]::ToUInt32($bytes,$pe) -ne 0x4550 -or [BitConverter]::ToUInt16($bytes,$pe+4) -ne 0x8664) { throw "Não é PE Windows x64: $Path" }
    $optional = $pe + 24
    if ([BitConverter]::ToUInt16($bytes,$optional) -ne 0x20b) { throw 'Esperado PE32+.' }
    $sections = [BitConverter]::ToUInt16($bytes,$pe+6)
    $sectionTable = $optional + [BitConverter]::ToUInt16($bytes,$pe+20)
    function Convert-Rva([uint32]$Rva) {
        for ($i=0; $i -lt $sections; $i++) {
            $s = $sectionTable + 40 * $i
            $start = [BitConverter]::ToUInt32($bytes,$s+12)
            $size = [Math]::Max([BitConverter]::ToUInt32($bytes,$s+8),[BitConverter]::ToUInt32($bytes,$s+16))
            if ($Rva -ge $start -and $Rva -lt ($start+$size)) { return $Rva-$start+[BitConverter]::ToUInt32($bytes,$s+20) }
        }
        throw "RVA inválido em $Path"
    }
    if ([IO.Path]::GetFileName($Path) -eq 'OXY Engine.exe') {
        $resource = Convert-Rva ([BitConverter]::ToUInt32($bytes,$optional+128))
        $count = [BitConverter]::ToUInt16($bytes,$resource+12) + [BitConverter]::ToUInt16($bytes,$resource+14)
        $types = for ($i=0; $i -lt $count; $i++) { [BitConverter]::ToUInt32($bytes,$resource+16+8*$i) }
        if (14 -notin $types -or 16 -notin $types) { throw 'Ícone ou metadados não foram incorporados ao PE.' }
    }
    $importRva = [BitConverter]::ToUInt32($bytes,$optional+120)
    if (!$importRva) { return }
    $descriptor = Convert-Rva $importRva
    while ($true) {
        $nameRva = [BitConverter]::ToUInt32($bytes,$descriptor+12)
        if (!$nameRva) { break }
        $offset = Convert-Rva $nameRva
        $end = $offset
        while ($bytes[$end]) { $end++ }
        $name = [Text.Encoding]::ASCII.GetString($bytes,$offset,$end-$offset)
        $bundled = Test-Path -LiteralPath (Join-Path $package $name)
        if ($name -match '^(vcruntime|msvcp)\d' -and !$bundled) { throw "Runtime Visual C++ externo: $name" }
        if (!$bundled -and $name -notmatch '^api-ms-win-' -and !(Test-Path -LiteralPath (Join-Path "$env:SystemRoot/System32" $name))) { throw "DLL não incluída no pacote: $name" }
        Write-Host "$([IO.Path]::GetFileName($Path)) -> $name"
        $descriptor += 20
    }
}
Get-ChildItem -LiteralPath $package -File | Where-Object Extension -In '.exe','.dll' | ForEach-Object { Assert-PortablePe $_.FullName }
$manifest = Get-Content -LiteralPath (Join-Path $package 'data/project.oxy.json') -Raw | ConvertFrom-Json
if ($manifest.schema_version -notin @(1,2,3) -or $manifest.scenes.Count -lt 3) { throw 'As cenas de exemplo devem estar presentes em formato suportado (schema 1, 2 ou 3).' }
foreach ($asset in $manifest.assets) {
    if ($asset.kind -eq 'Model') { continue }
    $data = [IO.Path]::GetFullPath((Join-Path $package 'data'))
    $assetPath = [IO.Path]::GetFullPath((Join-Path $data $asset.path))
    if (!$assetPath.StartsWith($data + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or !(Test-Path -LiteralPath $assetPath -PathType Leaf)) { throw "Asset inválido/ausente: $($asset.path)" }
}
if (!$ContentOnly) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $editor.FullName
    $start.WorkingDirectory = $testRoot
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WindowStyle = 'Hidden'
    $start.EnvironmentVariables['PATH'] = "$env:SystemRoot\System32;$env:SystemRoot"
    $start.EnvironmentVariables['CARGO_HOME'] = Join-Path $testRoot 'no-cargo'
    $start.EnvironmentVariables['RUSTUP_HOME'] = Join-Path $testRoot 'no-rustup'
    $start.EnvironmentVariables['LOCALAPPDATA'] = Join-Path $testRoot 'settings'
    $process = [Diagnostics.Process]::Start($start)
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds(25)
        do {
            Start-Sleep -Milliseconds 250
            $process.Refresh()
            if ($process.HasExited) { throw "Editor encerrou durante a abertura: $($process.ExitCode)" }
        } while ((!$process.MainWindowHandle -or $process.MainWindowTitle -notlike 'OXY Engine*') -and [DateTime]::UtcNow -lt $deadline)
        if (!$process.MainWindowHandle -or $process.MainWindowTitle -notlike 'OXY Engine*' -or !$process.Responding) { throw 'Janela nativa OXY Engine não ficou disponível.' }
        Write-Host "Janela aberta sem argumentos, fora do repositório e sem Rust/Cargo no PATH: $($process.MainWindowTitle)"
    } finally {
        if (!$process.HasExited) {
            $null = $process.CloseMainWindow()
            if (!$process.WaitForExit(5000)) { $process.Kill() }
        }
        $process.Dispose()
    }
}
Write-Host "Validado: OXY Engine $($version.ProductVersion). Extração preservada em $testRoot"
Write-Host 'A checagem de PATH não substitui o teste em outra máquina limpa/Windows Sandbox.'
