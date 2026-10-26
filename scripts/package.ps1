param(
    [string]$Project = 'examples/validacao',
    [string]$Destination = 'dist/OXY-Player',
    [switch]$SkipBuild,
    [switch]$DebugBuild,
    [switch]$IncludeEditor,
    [string]$BinaryDirectory = ''
)
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
$projectCandidate = if ([IO.Path]::IsPathRooted($Project)) { $Project } else { Join-Path $workspace $Project }
$projectResolved = (Resolve-Path -LiteralPath $projectCandidate).Path
$manifestPath = if (Test-Path -LiteralPath $projectResolved -PathType Container) { Join-Path $projectResolved 'project.oxy.json' } else { $projectResolved }
$dataRoot = Split-Path $manifestPath -Parent
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($manifest.schema_version -notin @(1,2,3,4,5,6)) { throw 'Versão incompatível: o pacote aceita schema_version 1 a 6.' }
$destinationPath = [IO.Path]::GetFullPath($(if ([IO.Path]::IsPathRooted($Destination)) { $Destination } else { Join-Path $workspace $Destination }))
if ($destinationPath.TrimEnd('\','/') -eq $workspace.TrimEnd('\','/') -or $destinationPath.TrimEnd('\','/') -eq $dataRoot.TrimEnd('\','/')) { throw 'Escolha uma pasta de pacote separada do projeto e do repositório.' }
if ($destinationPath.TrimEnd('\','/') -eq [IO.Path]::GetPathRoot($destinationPath).TrimEnd('\','/')) { throw 'A raiz de um disco não pode ser pasta de pacote.' }
if ($workspace.StartsWith($destinationPath.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or $dataRoot.StartsWith($destinationPath.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'A pasta de pacote não pode conter o repositório ou os dados originais.' }
$files = @()
foreach ($asset in $manifest.assets) {
    # A model's pieces are in the project file, except in the tree layout, where each model is a
    # file of its own that the player reads.
    if ($asset.kind -eq 'Model' -and ($manifest.layout -ne 'Tree' -or $asset.model)) { continue }
    $relative = [string]$asset.path
    if ([string]::IsNullOrWhiteSpace($relative) -or [IO.Path]::IsPathRooted($relative) -or $relative.Contains(':') -or ($relative -split '[/\\]' -contains '..')) { throw "Caminho de asset inseguro: $relative" }
    $source = (Resolve-Path -LiteralPath (Join-Path $dataRoot $relative)).Path
    if (!$source.StartsWith($dataRoot.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Asset sai da pasta de dados: $relative" }
    $files += [PSCustomObject]@{ Source = $source; Relative = $relative }
}
# Every .luau of the project's tree goes, as the export takes them: objects name their files, in
# any folder, and a script may require any other. What stays out of a project stays out here:
# entries starting with a dot (.oxy, .vscode, .luaurc), links (never followed) and subfolders
# that are another project or an exported game.
$scriptFolders = [Collections.Generic.Queue[string]]::new()
$scriptFolders.Enqueue($dataRoot)
while ($scriptFolders.Count) {
    foreach ($entry in Get-ChildItem -LiteralPath $scriptFolders.Dequeue() -Force) {
        if ($entry.Name.StartsWith('.') -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint)) { continue }
        if ($entry.PSIsContainer) {
            if (!(Test-Path -LiteralPath (Join-Path $entry.FullName 'project.oxy.json')) -and !(Test-Path -LiteralPath (Join-Path $entry.FullName 'data/export.oxy.json'))) { $scriptFolders.Enqueue($entry.FullName) }
        }
        elseif ($entry.Name -cmatch '.\.luau$') { $files += [PSCustomObject]@{ Source = $entry.FullName; Relative = $entry.FullName.Substring($dataRoot.TrimEnd('\','/').Length + 1) } }
    }
}
if (!$SkipBuild) {
    $buildArgs = @('build','-p','oxy_player','--locked')
    if ($IncludeEditor) { $buildArgs += @('-p','oxy_editor') }
    if (!$DebugBuild) { $buildArgs += '--release' }
    & (Join-Path $PSScriptRoot 'cargo.ps1') @buildArgs
    if ($LASTEXITCODE -ne 0) { throw 'Build do pacote falhou; pacote anterior preservado.' }
}
$profile = if ($DebugBuild) { 'debug' } else { 'release' }
$binaryRoot = if ($BinaryDirectory) { [IO.Path]::GetFullPath($(if ([IO.Path]::IsPathRooted($BinaryDirectory)) { $BinaryDirectory } else { Join-Path $workspace $BinaryDirectory })) } else { Join-Path $workspace "target/$profile" }
$executable = Join-Path $binaryRoot 'oxy_player.exe'
if (!(Test-Path -LiteralPath $executable -PathType Leaf)) { throw "Executável não encontrado: $executable" }
$editorExecutable = Join-Path $binaryRoot 'oxy_editor.exe'
if ($IncludeEditor -and !(Test-Path -LiteralPath $editorExecutable -PathType Leaf)) { throw "Editor não encontrado: $editorExecutable" }
$destinationParent = Split-Path $destinationPath -Parent
New-Item -ItemType Directory -Path $destinationParent -Force | Out-Null
$stagingPath = Join-Path $destinationParent ('.oxy-package-' + [Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $stagingPath | Out-Null
$packageVersion = [regex]::Match((Get-Content -LiteralPath (Join-Path $workspace 'Cargo.toml') -Raw), '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value.Replace("+patch.", ".")
[IO.File]::WriteAllText((Join-Path $stagingPath 'VERSAO.txt'), "OXY Engine $packageVersion`r`n", [Text.Encoding]::UTF8)
$packageData = Join-Path $stagingPath 'data'
New-Item -ItemType Directory -Path $packageData -Force | Out-Null
foreach ($file in $files) {
    $target = [IO.Path]::GetFullPath((Join-Path $packageData $file.Relative))
    if (!$target.StartsWith($packageData.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw "Destino de asset inseguro: $target" }
    New-Item -ItemType Directory -Path (Split-Path $target -Parent) -Force | Out-Null
    Copy-Item -LiteralPath $file.Source -Destination $target -Force
}
Copy-Item -LiteralPath $executable -Destination (Join-Path $stagingPath 'oxy_player.exe') -Force
Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $packageData 'project.oxy.json') -Force
$licenseTarget = Join-Path $stagingPath 'licenses'
New-Item -ItemType Directory -Path $licenseTarget -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $workspace 'licenses') -File | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $licenseTarget
}
if ($IncludeEditor) {
    Copy-Item -LiteralPath $editorExecutable -Destination (Join-Path $stagingPath 'OXY Engine.exe') -Force
}
# No runtime DLL goes along: the GNU build links GCC's runtime and Luau's libstdc++ statically
# (.cargo/config.toml) and the MSVC one the C runtime (+crt-static), as an exported game needs:
# the export copies the player alone. test-portable.ps1 checks what each executable imports.
# Backups an agent leaves next to a project (project.oxy.json.agente.bak) never ship.
Get-ChildItem -LiteralPath $stagingPath -Recurse -File -Filter '*.agente.bak' | Remove-Item -Force
$previousPath = $null
if (Test-Path -LiteralPath $destinationPath) {
    if (!(Test-Path -LiteralPath $destinationPath -PathType Container)) { throw 'O destino existente deve ser uma pasta.' }
    $previousPath = Join-Path $destinationParent ('.oxy-package-previous-' + [Guid]::NewGuid().ToString())
}
# Every directory move stays inside the explicitly selected destination parent.
foreach ($candidate in @($destinationPath,$stagingPath,$previousPath)) {
    if ($null -eq $candidate) { continue }
    $absolute = [IO.Path]::GetFullPath($candidate)
    if ((Split-Path $absolute -Parent) -ne $destinationParent) { throw "Destino de movimentação fora da pasta de pacote: $absolute" }
}
if ($previousPath) { Move-Item -LiteralPath $destinationPath -Destination $previousPath }
try { Move-Item -LiteralPath $stagingPath -Destination $destinationPath }
catch {
    if ($previousPath) { Move-Item -LiteralPath $previousPath -Destination $destinationPath }
    throw
}
Write-Host "Pacote desktop pronto: $destinationPath"
Write-Host 'Abra oxy_player.exe. A pasta data precisa permanecer ao lado do executável.'
if ($IncludeEditor) { Write-Host 'Abra OXY Engine.exe diretamente e escolha Projeto de exemplo para editar uma cópia dos dados incluídos.' }
if ($previousPath) { Write-Host "Pacote anterior preservado: $previousPath" }
