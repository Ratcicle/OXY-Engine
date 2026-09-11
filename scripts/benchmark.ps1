param([Parameter(Mandatory)][string]$Label, [string]$Sizes = '100,200,400,800,1600', [string]$Executable = '', [ValidateSet('performance','performance_extra','performance_mesh','performance_movement')][string]$Example = 'performance', [string]$ReferenceCommit = '')
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
$results = Join-Path $workspace 'benchmarks/results'
New-Item -ItemType Directory -Path $results -Force | Out-Null
if ($Label -notmatch '^[a-zA-Z0-9_-]+$') { throw 'Rótulo inválido.' }
$result = Join-Path $results "$Label.json"
if (Test-Path -LiteralPath $result) { throw 'Referência existente preservada. Escolha outro rótulo.' }
if (!$Executable) {
    & (Join-Path $PSScriptRoot 'cargo.ps1') build --release --locked -p oxy_render --example $Example --features oxy_core/profiling
    if ($LASTEXITCODE -ne 0) { throw 'Falha no build.' }
    $Executable = Join-Path $workspace "target/release/examples/$Example.exe"
}
$metadata = [ordered]@{
    commit = (& git -c "safe.directory=$($workspace.Replace('\','/'))" rev-parse HEAD)
    executable_reference_commit = $ReferenceCommit
    example = $Example
    dirty = @(& git -c "safe.directory=$($workspace.Replace('\','/'))" status --porcelain)
    cpu = (Get-CimInstance Win32_Processor | Select-Object -ExpandProperty Name)
    logical_cpus = [Environment]::ProcessorCount
    ram_bytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    os = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber)
    rust = (& (Join-Path $env:USERPROFILE '.cargo/bin/rustc.exe') -vV) -join "`n"
    flags = 'release opt-level=2; --locked; oxy_core/profiling; no RUSTFLAGS override'
    utc = [DateTime]::UtcNow.ToString('o')
    executable_sha256 = (Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash
    sizes = $Sizes
    graphics = 'CPU only. No GPU time, resolution or FPS inferred.'
}
$metadata | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $results "$Label-environment.json") -Encoding utf8
# A hard whole-suite timeout supplements the harness per-measurement soft timeout.
$start = [Diagnostics.ProcessStartInfo]::new($Executable, $Sizes)
$start.UseShellExecute = $false; $start.CreateNoWindow = $true; $start.WindowStyle = 'Hidden'
$start.RedirectStandardOutput = $true; $start.WorkingDirectory = $workspace
$process = [Diagnostics.Process]::Start($start)
$read = $process.StandardOutput.ReadToEndAsync()
$deadline = [DateTime]::UtcNow.AddMinutes(15)
$peakWorkingSet = 0L
while (!$process.WaitForExit(100)) {
    $process.Refresh()
    $peakWorkingSet = [Math]::Max($peakWorkingSet, $process.PeakWorkingSet64)
    if ([DateTime]::UtcNow -gt $deadline) { $process.Kill(); throw 'Limite de 15 minutos excedido. Referência anterior preservada.' }
}
if ($process.ExitCode -ne 0) { throw "Benchmark falhou: $($process.ExitCode)" }
$json = $read.GetAwaiter().GetResult()
$null = $json | ConvertFrom-Json
[IO.File]::WriteAllText($result, $json, [Text.UTF8Encoding]::new($false))
$metadata['observed_peak_working_set_bytes'] = $peakWorkingSet
$metadata['memory_method'] = 'Windows PeakWorkingSet64 sampled every 100ms while alive; includes allocator and libraries, not live logical records; zero means unavailable.'
$metadata | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $results "$Label-environment.json") -Encoding utf8
$process.Dispose()
Write-Host "Resultados: $result"
