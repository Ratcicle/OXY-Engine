param(
    [string]$Before = 'v032-base-movement',
    [string]$After = 'v032-final-movement-analytic'
)
$ErrorActionPreference = 'Stop'
if ($Before -notmatch '^[a-zA-Z0-9_-]+$' -or $After -notmatch '^[a-zA-Z0-9_-]+$') { throw 'Rótulo inválido.' }
$root = Join-Path (Split-Path $PSScriptRoot -Parent) 'benchmarks/results'
$base = Get-Content -LiteralPath (Join-Path $root "$Before.json") -Raw | ConvertFrom-Json
$final = Get-Content -LiteralPath (Join-Path $root "$After.json") -Raw | ConvertFrom-Json
if ($base.rows.Count -ne $final.rows.Count) { throw 'Quantidade de cenários diferente.' }
$rows = foreach ($row in $final.rows) {
    $referenceRows = @($base.rows | Where-Object { $_.scenario -eq $row.scenario -and $_.controllers -eq $row.controllers -and $_.entities -eq $row.entities })
    if ($referenceRows.Count -ne 1) { throw 'Referência ausente ou ambígua para cenário/total/personagens.' }
    $old = $referenceRows[0]
    foreach ($phase in @('single','four_steps')) {
        $a = $old.$phase; $b = $row.$phase
        if ($a.steps_per_advance -ne $b.steps_per_advance -or $a.advance.samples -ne $b.advance.samples -or $a.advance.limit_exceeded -or $b.advance.limit_exceeded) { throw 'Trabalho diferente ou limite de duração excedido.' }
        if ($a.checksum -ne $b.checksum) { throw "Resultado observável diferente: $($row.scenario)/$($row.controllers)/$phase. Investigue antes de aceitar a comparação." }
        $beforeQueries = $a.physics_after.queries - $a.physics_before.queries
        $afterQueries = $b.physics_after.queries - $b.physics_before.queries
        [pscustomobject]@{
            scenario = $row.scenario
            characters = $row.controllers
            entities = $row.entities
            phase = $phase
            base_median_ms = $a.advance.median_ns / 1e6
            final_median_ms = $b.advance.median_ns / 1e6
            base_p95_ms = $a.advance.p95_ns / 1e6
            final_p95_ms = $b.advance.p95_ns / 1e6
            base_p99_ms = $a.advance.p99_ns / 1e6
            final_p99_ms = $b.advance.p99_ns / 1e6
            change_percent = 100 * ($b.advance.median_ns / $a.advance.median_ns - 1)
            checksum_equal = $true
            query_count_equal = $beforeQueries -eq $afterQueries
        }
    }
}
$destination = Join-Path $root "$Before-to-$After.csv"
$rows | Export-Csv -LiteralPath $destination -NoTypeInformation -Encoding utf8
Write-Host "Comparação CPU: $destination. Checksums iguais; tempos não equivalem a FPS."
