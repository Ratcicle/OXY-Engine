--!strict
-- A música do jogo, no menu e na pista, com os volumes salvos nas opções.
local Musica = {}

function Musica.on_start(self: Object)
    audio.loop("Música", { channel = "music", volume = 0.7, fade = 0.5 })
    audio.set_volume("music", vars.get("Volume da música"))
    audio.set_volume("effects", vars.get("Volume dos efeitos"))
end

return Musica
