--!strict
-- Botão Continuar do menu de pausa.
local Continuar = {}

function Continuar.on_click(self: Object)
    audio.play("Clique", { volume = 0.8 })
    game.resume()
end

return Continuar
