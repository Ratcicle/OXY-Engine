param([string]$Before = 'baseline', [string]$After = 'final')
$ErrorActionPreference = 'Stop'
$root = Join-Path (Split-Path $PSScriptRoot -Parent) 'benchmarks/results'
$a = Get-Content -LiteralPath (Join-Path $root "$Before.json") -Raw | ConvertFrom-Json
$b = Get-Content -LiteralPath (Join-Path $root "$After.json") -Raw | ConvertFrom-Json
$rows = foreach ($row in $b.rows) {
    $old = $a.rows | Where-Object { $_.entities -eq $row.entities -and $_.scenario -eq $row.scenario }
    if (!$old) { throw 'Cenário não encontrado na referência.' }
    foreach ($property in $row.PSObject.Properties) {
        $v = $property.Value
        if ($null -eq $v.median_ns) { continue }
        $u = $old.($property.Name)
        if ($null -eq $u.median_ns) { throw 'Operação não encontrada na referência.' }
        [pscustomobject]@{
            scenario=$row.scenario; entities=$row.entities; operation=$property.Name
            before_median_ms=$u.median_ns/1e6; after_median_ms=$v.median_ns/1e6
            ratio_before_over_after=$u.median_ns/[Math]::Max(1,$v.median_ns)
            before_samples=$u.samples; after_samples=$v.samples
            before_p95_ns=$u.p95_ns; after_p95_ns=$v.p95_ns
            before_p99_ns=$u.p99_ns; after_p99_ns=$v.p99_ns
            before_limit=$u.limit_exceeded; after_limit=$v.limit_exceeded
            before_candidates_per_sample=$u.counters.candidates/$u.samples
            after_candidates_per_sample=$v.counters.candidates/$v.samples
        }
    }
}
$rows | Export-Csv -LiteralPath (Join-Path $root "$Before-to-$After.csv") -NoTypeInformation -Encoding utf8
if ($a.equivalence -and $b.equivalence) {
    $old = ConvertTo-Json -InputObject $a.equivalence -Depth 60 -Compress
    $new = ConvertTo-Json -InputObject $b.equivalence -Depth 60 -Compress
    if ($old -cne $new) { throw 'Estados/rastros observáveis diferentes; investigar antes de aceitar os tempos.' }
    Write-Host 'Estados e rastros ordenados exatamente iguais nos replays de 60 passos.'
}
Write-Host "Comparação: $Before-to-$After.csv. Razão > 1 indica menor duração após; não representa FPS."
