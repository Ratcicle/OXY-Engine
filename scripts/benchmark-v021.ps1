param(
    [ValidatePattern('^[a-zA-Z0-9_-]+$')][string]$Label = 'current',
    [switch]$Reference
)
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
$previousLabel = $env:OXY_BENCH_LABEL
$previousRoot = $env:OXY_BENCH_ROOT
$env:OXY_BENCH_LABEL = $Label
$env:OXY_BENCH_ROOT = $workspace
Push-Location $workspace
try {
    New-Item -ItemType Directory -Path 'artifacts' -Force | Out-Null
    if ($Reference) {
        $referenceDirectory = Join-Path $workspace '.tools/v021-baseline'
        $executable = Join-Path $referenceDirectory 'oxy_editor-reference.exe'
        $manifestPath = Join-Path $referenceDirectory 'reference.json'
        if (!(Test-Path -LiteralPath $executable) -or !(Test-Path -LiteralPath $manifestPath)) {
            throw 'A referência local imutável não está disponível. Resultados brutos preservados ficam em benchmarks/results/v021-native-m0-base.json.'
        }
        $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        if ((Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash -ne $manifest.executable_sha256) {
            throw 'O executável de referência não corresponde ao hash registrado.'
        }
        $previousPath = $env:PATH
        try {
            $env:PATH = (Join-Path $env:USERPROFILE '.cargo/oxy-native-toolchain/bin') + ';' + $env:PATH
            # Windows GUI-subsystem binaries must be explicitly waited on.
            $process = Start-Process -FilePath $executable -ArgumentList @('native_component_v021_measurements', '--ignored', '--nocapture') -WindowStyle Hidden -PassThru -Wait -RedirectStandardOutput "artifacts/v021-$Label-stdout.log" -RedirectStandardError "artifacts/v021-$Label-stderr.log"
            if ($process.ExitCode -ne 0) { throw "O benchmark terminou com código $($process.ExitCode). Consulte artifacts/v021-$Label-stderr.log." }
        } finally { $env:PATH = $previousPath }
        # The running checkout can differ from the immutable binary's source. Preserve both.
        $resultPath = Join-Path $workspace "benchmarks/results/v021-native-$Label.json"
        $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
        $result | Add-Member -NotePropertyName runtime_checkout_commit -NotePropertyValue $result.commit -Force
        $result.commit = $manifest.source_commit
        $result | Add-Member -NotePropertyName immutable_executable_sha256 -NotePropertyValue $manifest.executable_sha256 -Force
        $result | ConvertTo-Json -Depth 50 | Set-Content -LiteralPath $resultPath -Encoding utf8
    } else {
        & "$PSScriptRoot/cargo.ps1" @('test', '--release', '--locked', '-p', 'oxy_editor', 'native_component_v021_measurements', '--', '--ignored', '--nocapture') *> "artifacts/v021-$Label.log"
        if ($LASTEXITCODE -ne 0) { throw "O benchmark falhou. Consulte artifacts/v021-$Label.log." }
    }
    Write-Output "Medições CPU: benchmarks/results/v021-native-$Label.json"
    Write-Output "Capturas WGPU reais: qa/v0.2.1/$Label"
    Write-Output 'Limites de duração preservam amostras parciais; não equivalem a completar 101 ciclos. Não execute outros testes/builds durante a medição.'
} finally {
    Pop-Location
    $env:OXY_BENCH_LABEL = $previousLabel
    $env:OXY_BENCH_ROOT = $previousRoot
}
