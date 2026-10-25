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
if ([version]([Diagnostics.FileVersionInfo]::GetVersionInfo($editor.FullName).ProductVersion) -ge [version]'0.5.0') {
    if (!(Test-Path -LiteralPath (Join-Path $package 'oxy-mcp.exe') -PathType Leaf)) { throw 'Arquivo da 0.5.0 ausente: oxy-mcp.exe' }
}
if ([version]([Diagnostics.FileVersionInfo]::GetVersionInfo($editor.FullName).ProductVersion) -ge [version]'0.6.0') {
    foreach ($file in @('licenses/cpal-0.18.2.txt','licenses/kenney-skyboxes-cc0.txt','parkour/project.oxy.json')) {
        if (!(Test-Path -LiteralPath (Join-Path $package $file) -PathType Leaf)) { throw "Arquivo da 0.6.0 ausente: $file" }
    }
    # OXY Parkour, opened from the home screen as a copy, needs every file its project names: in
    # 0.6 (schema 5) the sounds its assets name; from 0.7 (schema 6, in folders) every file of its
    # register, each model in its own file, and the scripts its objects attach.
    $parkour = [IO.Path]::GetFullPath((Join-Path $package 'parkour'))
    $parkourManifest = Get-Content -LiteralPath (Join-Path $parkour 'project.oxy.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($parkourManifest.scenes.Count -lt 2) { throw 'O parkour deve ter o menu e a pista.' }
    function Get-ParkourFile([string]$Relative) {
        $path = [IO.Path]::GetFullPath((Join-Path $parkour $Relative))
        if (!$path.StartsWith($parkour + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or !(Test-Path -LiteralPath $path -PathType Leaf)) { throw "Arquivo do parkour inválido/ausente: $Relative" }
        $path
    }
    if ($parkourManifest.schema_version -eq 6) {
        if ($parkourManifest.layout -ne 'Tree') { throw 'O parkour da 0.7 deve estar em pastas.' }
        foreach ($asset in $parkourManifest.assets) {
            $file = Get-ParkourFile $asset.path
            if ($asset.kind -eq 'Model' -and (Get-Content -LiteralPath $file -Raw -Encoding UTF8 | ConvertFrom-Json).oxy -ne 'modelo') { throw "Modelo do parkour inválido: $($asset.path)" }
        }
        $scripts = @($parkourManifest.scenes.entities | ForEach-Object { $_.scripts } | Where-Object { $_ } | ForEach-Object { $_.path } | Sort-Object -Unique)
        if (!$scripts.Count) { throw 'O parkour da 0.7 roda em scripts, e o pacote não tem nenhum.' }
        foreach ($script in $scripts) { Get-ParkourFile $script | Out-Null }
        # Scripts run on Luau, through mlua: the 0.7 package carries both licenses.
        foreach ($file in @('licenses/mlua-0.12.2.txt','licenses/luau-0.740.txt')) {
            if (!(Test-Path -LiteralPath (Join-Path $package $file) -PathType Leaf)) { throw "Arquivo da 0.7.0 ausente: $file" }
        }
    } elseif ($parkourManifest.schema_version -eq 5) {
        foreach ($asset in $parkourManifest.assets) {
            if ($asset.kind -ne 'Model') { Get-ParkourFile $asset.path | Out-Null }
        }
    } else {
        throw 'O parkour deve estar no schema 5 (0.6) ou 6 (0.7).'
    }
    # Backups an agent or a schema conversion leaves next to a project never ship.
    $backups = @(Get-ChildItem -LiteralPath $package -Recurse -File | Where-Object { $_.Name -like '*.agente.bak' -or $_.Name -like '*.backup.json' })
    if ($backups.Count) { throw "Backups de projeto no pacote: $($backups.Name -join ', ')" }
}
# Nor what stays out of a project, anywhere in the package: entries starting with a dot (OXY's
# .oxy, with the agent's copies and the backups, .vscode, .luaurc).
$outside = @(Get-ChildItem -LiteralPath $package -Recurse -Force | Where-Object { $_.Name.StartsWith('.') } | ForEach-Object { $_.FullName.Substring($package.Length + 1) })
if ($outside.Count) { throw "Pastas e arquivos que ficam fora de um projeto no pacote: $($outside -join ', ')" }
# Luau is C++: the GNU build links its libstdc++ statically (.cargo/config.toml), so GCC's runtime
# DLLs neither ship nor hide, next to the executables, one that would need them.
$gccRuntime = '^(libstdc\+\+|libgcc|libwinpthread)'
$shippedRuntime = @(Get-ChildItem -LiteralPath $package -Recurse -File -Filter '*.dll' | Where-Object { $_.Name -match $gccRuntime })
if ($shippedRuntime.Count) { throw "Runtime do GCC no pacote: $($shippedRuntime.Name -join ', ')" }
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
    function Read-Name([uint32]$Rva) {
        $offset = Convert-Rva $Rva
        $end = $offset
        while ($bytes[$end]) { $end++ }
        [Text.Encoding]::ASCII.GetString($bytes,$offset,$end-$offset)
    }
    $imports = @()
    $importRva = [BitConverter]::ToUInt32($bytes,$optional+120)
    if ($importRva) {
        $descriptor = Convert-Rva $importRva
        while ($nameRva = [BitConverter]::ToUInt32($bytes,$descriptor+12)) {
            $imports += Read-Name $nameRva
            $descriptor += 20
        }
    }
    # A delay-loaded DLL loads at its first call, so it counts as much as an import.
    $delayRva = [BitConverter]::ToUInt32($bytes,$optional+216)
    if ($delayRva) {
        $descriptor = Convert-Rva $delayRva
        while ($nameRva = [BitConverter]::ToUInt32($bytes,$descriptor+4)) {
            if (!([BitConverter]::ToUInt32($bytes,$descriptor) -band 1)) { throw "Tabela de carga atrasada em formato antigo: $Path" }
            $imports += Read-Name $nameRva
            $descriptor += 32
        }
    }
    $file = [IO.Path]::GetFileName($Path)
    $strict = $file -in $windowsOnly
    $names = @($imports | Sort-Object -Unique)
    $gcc = @($names | Where-Object { $_ -match $gccRuntime })
    if ($gcc.Count) { throw "$file importa o runtime do GCC: $($gcc -join ', ') (a libstdc++ do Luau deve ser estática, como pede o .cargo/config.toml)" }
    foreach ($name in $names) {
        $bundled = Test-Path -LiteralPath (Join-Path $package $name)
        $system = $name -match '^api-ms-win-' -or (Test-Path -LiteralPath (Join-Path "$env:SystemRoot/System32" $name) -PathType Leaf)
        if ($name -match '^(vcruntime|msvcp)\d' -and ($strict -or !$bundled)) { throw "$file importa o runtime Visual C++: $name (compile com +crt-static)" }
        if ($strict -and ($bundled -or !$system)) { throw "$file importa uma DLL que não é do Windows: $name" }
        if (!$bundled -and !$system) { throw "DLL não incluída no pacote: $name" }
        Write-Host "$file -> $name"
    }
}
# The editor, the player (an exported game's executable) and the MCP server import only
# Windows' own DLLs: neither GCC's runtime nor Visual C++'s, nor a DLL the package carries.
$windowsOnly = @('OXY Engine.exe', 'oxy_player.exe', 'oxy-mcp.exe')
foreach ($binary in Get-ChildItem -LiteralPath $package -File | Where-Object Extension -In '.exe','.dll') { Assert-PortablePe $binary.FullName }
$manifest = Get-Content -LiteralPath (Join-Path $package 'data/project.oxy.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($manifest.schema_version -notin @(1,2,3,4,5,6) -or $manifest.scenes.Count -lt 3) { throw 'As cenas de exemplo devem estar presentes em formato suportado (schema 1 a 6).' }
foreach ($asset in $manifest.assets) {
    if ($asset.kind -eq 'Model') { continue }
    $data = [IO.Path]::GetFullPath((Join-Path $package 'data'))
    $assetPath = [IO.Path]::GetFullPath((Join-Path $data $asset.path))
    if (!$assetPath.StartsWith($data + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or !(Test-Path -LiteralPath $assetPath -PathType Leaf)) { throw "Asset inválido/ausente: $($asset.path)" }
}
# The player checks the packaged data itself, as an exported game does: no window, so CI runs it too.
if ([version]$version.ProductVersion -ge [version]'0.6.0') {
    $verifyReport = Join-Path $testRoot 'verificar.json'
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = Join-Path $package 'oxy_player.exe'
    $start.Arguments = "--verificar `"$verifyReport`""
    $start.WorkingDirectory = $testRoot
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.EnvironmentVariables['PATH'] = "$env:SystemRoot\System32;$env:SystemRoot"
    $player = [Diagnostics.Process]::Start($start)
    try {
        if (!$player.WaitForExit(120000)) { $player.Kill(); throw 'oxy_player.exe --verificar não terminou em 120 s.' }
        if (!(Test-Path -LiteralPath $verifyReport -PathType Leaf)) { throw "oxy_player.exe --verificar não gravou o relatório (código $($player.ExitCode))." }
        $verified = Get-Content -LiteralPath $verifyReport -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($player.ExitCode -ne 0 -or !$verified.ok) { throw "oxy_player.exe --verificar falhou: $($verified.errors -join ' ')" }
        if ($verified.engine -ne $version.ProductVersion) { throw "oxy_player.exe é da OXY Engine $($verified.engine), e o editor é $($version.ProductVersion)." }
        Write-Host "Player conferiu os dados do pacote: $($verified.game), cena inicial $($verified.start_scene)."
    } finally {
        $player.Dispose()
    }
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
