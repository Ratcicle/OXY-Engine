param(
    [string]$Destination = '',
    [string]$BinaryDirectory = '',
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
$version = [regex]::Match((Get-Content -LiteralPath (Join-Path $workspace 'Cargo.toml') -Raw), '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
if (!$Destination) { $Destination = "dist/OXY-Engine-$version-windows-x64" }
$destinationPath = [IO.Path]::GetFullPath($(if ([IO.Path]::IsPathRooted($Destination)) { $Destination } else { Join-Path $workspace $Destination }))
& (Join-Path $PSScriptRoot 'package.ps1') -Project 'examples/validacao' -Destination $destinationPath -IncludeEditor -SkipBuild:$SkipBuild -BinaryDirectory $BinaryDirectory
$editor = Join-Path $destinationPath 'OXY Engine.exe'
$metadata = [Diagnostics.FileVersionInfo]::GetVersionInfo($editor)
if ($metadata.ProductName -ne 'OXY Engine' -or $metadata.ProductVersion -ne $version) {
    throw "Executável incorreto ou antigo: $($metadata.ProductName) $($metadata.ProductVersion); esperado OXY Engine $version."
}
Copy-Item -LiteralPath (Join-Path $workspace 'LICENSE') -Destination $destinationPath
$instructions = @"
OXY Engine $version — Windows x64 portátil

Extraia o ZIP inteiro em uma pasta gravável e abra OXY Engine.exe.
Não é necessário instalar Rust, Cargo ou usar um arquivo .cmd.
Windows 10/11 x64 e um driver gráfico compatível com DirectX 12 ou Vulkan.

Novo projeto: escolha a primeira cena 2D/3D e use Salvar para escolher sua pasta.
Abrir projeto: escolha project.oxy.json de um projeto existente.
Projetos recentes: lista local em %LOCALAPPDATA%\OXY Engine.
Projeto de exemplo: sala 2D, oficina 3D e cartas; Salvar cria sua cópia.
Ajuda > Guia de lógica visual: consulte os nós e abra cópias de cinco receitas
editáveis, disponíveis offline dentro do executável.
Estúdio > Modelagem: crie formas, edite faces/arestas/vértices, pinte e anime
peças rígidas. Soltar a alça aplica a edição; Esc cancela o gesto inteiro.
Última operação ajusta o mesmo comando. Shift+U cria borda interna; Shift+E
extruda. Ctrl+caixa soma componentes, inclusive ocultos. F1 abre ajuda contextual.
Interface: ajuste a escala pendente e clique Aplicar; Cancelar mantém a atual.
Mantenha a pasta data e quaisquer DLLs ao lado dos executáveis.

oxy_player.exe é o runtime separado de jogos exportados. Neste pacote,
abre os dados de exemplo; os painéis de edição pertencem apenas ao editor.
Projetos antigos (schema 1) são convertidos em memória, sem alterar o original
ao abrir. Ao salvar convertido, a OXY preserva um backup do documento original.
O novo schema 2 requer OXY Engine 0.2.0 ou leitor compatível; não abra esses
documentos em versões antigas que desconheçam ações de entrada e malhas editáveis.
Sem instalador, atualização automática ou assinatura digital.
"@
[IO.File]::WriteAllText((Join-Path $destinationPath 'LEIA-ME.txt'), $instructions, [Text.Encoding]::UTF8)
$zip = "$destinationPath.zip"
$temporaryZip = "$destinationPath.$([Guid]::NewGuid()).zip"
Compress-Archive -LiteralPath $destinationPath -DestinationPath $temporaryZip -CompressionLevel Optimal
# File replacement preserves the prior ZIP if compression or replacement fails.
if (Test-Path -LiteralPath $zip) { [IO.File]::Replace($temporaryZip, $zip, "$zip.previous", $true) }
else { [IO.File]::Move($temporaryZip, $zip) }
$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$zip.sha256", "$hash  $([IO.Path]::GetFileName($zip))`n", [Text.Encoding]::ASCII)
Write-Host "ZIP portátil: $zip"
