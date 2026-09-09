param(
    [string]$Before = 'benchmarks/results/v021-native-m0-base.json',
    [string]$After = 'benchmarks/results/v021-native-final.json',
    [string]$Output = 'benchmarks/results/v021-comparison.csv'
)
$ErrorActionPreference = 'Stop'
$baseline = Get-Content -LiteralPath $Before -Raw | ConvertFrom-Json
$final = Get-Content -LiteralPath $After -Raw | ConvertFrom-Json
$rows = foreach ($oldFixture in $baseline.rows) {
    $newFixture = @($final.rows | Where-Object { $_.fixture -eq $oldFixture.fixture })
    if ($newFixture.Count -ne 1 -or $newFixture[0].triangles -ne $oldFixture.triangles) {
        throw "Carga ausente ou alterada: $($oldFixture.fixture)"
    }
    foreach ($oldPhase in $oldFixture.phases) {
        $newPhase = @($newFixture[0].phases | Where-Object { $_.phase -eq $oldPhase.phase })
        if ($newPhase.Count -ne 1) { throw "Fase ausente: $($oldPhase.phase)" }
        foreach ($measure in $oldPhase.measurements.PSObject.Properties) {
            $old = $measure.Value
            $new = $newPhase[0].measurements.($measure.Name)
            if ($null -eq $new) { throw "Medição ausente: $($measure.Name)" }
            [pscustomobject]@{
                fixture = $oldFixture.fixture
                triangles = $oldFixture.triangles
                phase = $oldPhase.phase
                measure = $measure.Name
                before_samples = $old.samples
                after_samples = $new.samples
                before_median_ms = if ($null -ne $old.median_ns) { [Math]::Round($old.median_ns / 1e6, 6) } else { $null }
                after_median_ms = if ($null -ne $new.median_ns) { [Math]::Round($new.median_ns / 1e6, 6) } else { $null }
                before_p95_ms = if ($null -ne $old.p95_ns) { [Math]::Round($old.p95_ns / 1e6, 6) } else { $null }
                after_p95_ms = if ($null -ne $new.p95_ns) { [Math]::Round($new.p95_ns / 1e6, 6) } else { $null }
                before_p99_ms = if ($null -ne $old.p99_ns) { [Math]::Round($old.p99_ns / 1e6, 6) } else { $null }
                after_p99_ms = if ($null -ne $new.p99_ns) { [Math]::Round($new.p99_ns / 1e6, 6) } else { $null }
                before_limit = $oldPhase.limited
                after_limit = $newPhase[0].limited
                before_selected_components = $oldPhase.last_selected_components
                after_selected_components = $newPhase[0].last_selected_components
            }
        }
    }
}
$rows | Export-Csv -LiteralPath $Output -NoTypeInformation -Encoding utf8
Write-Output $Output
