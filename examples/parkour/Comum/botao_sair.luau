--!strict
-- Botão que fecha o jogo (Sair e Sair do jogo).
local Sair = {}

function Sair.on_click(self: Object)
    audio.play("Clique", { volume = 0.8 })
    game.quit()
end

return Sair
