param(
    [string]$Before = 'benchmarks/results/v030-replay-before.json',
    [string]$After = 'benchmarks/results/v030-replay-final-indexed.json'
)
$ErrorActionPreference = 'Stop'
$a = (Get-Content -LiteralPath $Before -Raw | ConvertFrom-Json).runs
$b = (Get-Content -LiteralPath $After -Raw | ConvertFrom-Json).runs
if ($a.Count -ne $b.Count) { throw 'Quantidade de replays diferente' }
$maxError = 0.0
for ($i = 0; $i -lt $a.Count; $i++) {
    if ($a[$i].hz -ne $b[$i].hz -or $a[$i].mode -ne $b[$i].mode) { throw 'Cenários diferentes' }
    if (($a[$i].events | ConvertTo-Json -Depth 10 -Compress) -ne ($b[$i].events | ConvertTo-Json -Depth 10 -Compress)) { throw 'Ordem/tempo dos eventos alterados' }
    if ($a[$i].samples.Count -ne $b[$i].samples.Count) { throw 'Quantidade de amostras diferente' }
    for ($j = 0; $j -lt $a[$i].samples.Count; $j++) {
        $x = $a[$i].samples[$j]; $y = $b[$i].samples[$j]
        foreach ($field in @('tick','grounded','posture','support')) {
            if ($x.$field -ne $y.$field) { throw "Estado $field diferente no passo $($x.tick)" }
        }
        foreach ($field in @('position','velocity','look')) {
            for ($k = 0; $k -lt $x.$field.Count; $k++) {
                $errorValue = [math]::Abs($x.$field[$k] - $y.$field[$k])
                $maxError = [math]::Max($maxError, $errorValue)
                if ($errorValue -gt 0.0002) { throw "Erro de $errorValue em $field / passo $($x.tick)" }
            }
        }
    }
    if (@($b[$i].retained_after_stop | Where-Object { $_ -ne 0 }).Count) { throw 'Stop reteve ativações/tarefas' }
}
[pscustomobject]@{ runs=$a.Count; seconds_per_run=12; fixed_steps_per_run=720; max_numeric_error=$maxError; tolerance=0.0002; ordered_events_equal=$true } | ConvertTo-Json
