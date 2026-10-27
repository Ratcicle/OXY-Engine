--!strict
-- Botão que abre a cena de nome Cena (Jogar, Menu).
local Botao = {}
Botao.attributes = { Cena = "Menu" }

function Botao.on_click(self: Object)
    audio.play("Clique", { volume = 0.8 })
    scene.change(self.attr.Cena)
end

return Botao
