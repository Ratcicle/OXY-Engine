--!strict
-- Botões de volume das opções: um arquivo para os quatro. Canal é "Música" ou "Efeitos", Passo
-- é 0.1 ou -0.1, e a Barra mostra o volume em %. A escolha fica salva entre partidas.
local Volume = {}
Volume.attributes = { Canal = "Música", Passo = 0.1, Barra = param.object() }
type Canal = { id: "music" | "effects", variavel: string }
local CANAIS: { [string]: Canal } = {
    ["Música"] = { id = "music", variavel = "Volume da música" },
    Efeitos = { id = "effects", variavel = "Volume dos efeitos" },
}

function Volume.on_click(self: Object)
    audio.play("Clique", { volume = 0.8 })
    local canal = assert(CANAIS[self.attr.Canal], "o Canal é Música ou Efeitos")
    local volume = math.clamp(math.round((vars.get(canal.variavel) + self.attr.Passo) * 10) / 10, 0, 1)
    vars.set(canal.variavel, volume)
    audio.set_volume(canal.id, volume)
    self.attr.Barra.attr.Percentual = math.round(volume * 100)
    save.write()
end

return Volume
