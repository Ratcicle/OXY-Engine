--!strict
-- Botão que recomeça a corrida (Reiniciar e Jogar de novo).
local Reiniciar = {}

function Reiniciar.on_click(self: Object)
    audio.play("Clique", { volume = 0.8 })
    scene.restart()
end

return Reiniciar
