param([Parameter(Mandatory)][string]$Label, [int[]]$Sizes = @(100,1600), [string]$ReferenceDirectory = '')
$ErrorActionPreference = 'Stop'
if ($Label -notmatch '^[a-zA-Z0-9_-]+$') { throw 'Rótulo inválido.' }
$workspace = Split-Path $PSScriptRoot -Parent
$results = Join-Path $workspace 'benchmarks/results'
$arguments = @('test','--release','--locked','-p','oxy_editor','-p','oxy_player','--no-run','--message-format=json')
if ($ReferenceDirectory) { $arguments += @('--manifest-path', (Join-Path $ReferenceDirectory 'Cargo.toml'),'--target-dir',(Join-Path $workspace 'target/perf-reference')) }
$build = & (Join-Path $PSScriptRoot 'cargo.ps1') @arguments
if ($LASTEXITCODE -ne 0) { throw 'Falha no build nativo.' }
$executables = @{}
foreach ($line in $build) {
    $record = $line | ConvertFrom-Json
    if ($record.executable -and $record.profile.test) { $executables[$record.target.name] = $record.executable }
}
$toolchain = Join-Path $env:USERPROFILE '.cargo/oxy-native-toolchain/bin'
if (Test-Path -LiteralPath $toolchain) { $env:PATH = "$toolchain;$env:PATH" }
$metadata = [ordered]@{
    commit = (& git -c "safe.directory=$($workspace.Replace('\','/'))" rev-parse HEAD)
    reference_directory = $ReferenceDirectory
    cpu = (Get-CimInstance Win32_Processor).Name
    ram_bytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    os = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber)
    graphics = @(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,VideoModeDescription)
    rust = (& (Join-Path $env:USERPROFILE '.cargo/bin/rustc.exe') -vV) -join "`n"
    flags = 'release opt-level=2; --locked; profiling OFF'
    logical_window = '1280x720; CPU App::update only; GPU completion and eframe tessellation not timed'
    sizes = $Sizes
    utc = [DateTime]::UtcNow.ToString('o')
}
$metadataPath = Join-Path $results "$Label-environment.json"
if (Test-Path -LiteralPath $metadataPath) { throw 'Medição anterior preservada; use outro rótulo.' }
$metadata | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $metadataPath -Encoding utf8
foreach ($n in $Sizes) {
    foreach ($hostName in @('editor','player')) {
        $env:OXY_PERF_ENTITIES = "$n"
        $env:OXY_PERF_OUTPUT = Join-Path $results "$Label-$hostName-$n.json"
        if (Test-Path -LiteralPath $env:OXY_PERF_OUTPUT) { throw 'Resultado já existe.' }
        $p = Start-Process -FilePath $executables["oxy_$hostName"] -ArgumentList "native_${hostName}_performance --ignored --nocapture" -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $workspace "artifacts/$Label-$hostName-$n.log") -RedirectStandardError (Join-Path $workspace "artifacts/$Label-$hostName-$n-error.log")
        if (!$p.WaitForExit(150000)) { $p.Kill(); throw 'Teste nativo excedeu 150 segundos.' }
        if ($p.ExitCode -ne 0 -or !(Test-Path -LiteralPath $env:OXY_PERF_OUTPUT)) { throw "Falha no teste $hostName / $n. Consulte artifacts/." }
        Write-Host "$hostName / ${n}: medição e captura concluídas."
    }
}
