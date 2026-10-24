--!strict
-- Botão que troca um painel por outro (Opções e Voltar).
local Painel = {}
Painel.attributes = { Esconder = param.object(), Mostrar = param.object() }

function Painel.on_click(self: Object)
    audio.play("Clique", { volume = 0.8 })
    self.attr.Esconder.visible = false
    self.attr.Mostrar.visible = true
end

return Painel
