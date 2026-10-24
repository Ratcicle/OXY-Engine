--!strict
-- Modelo que ataca: a ação Atacar toca o clipe Ataque, e o marcador Impacto liga a Área de
-- acerto por 0,15 s. Cada vez que ela liga é uma ativação nova, com um acerto por alvo.
local Ataque = {}
local ABERTA = 0.15 -- segundos com a área ligada

function Ataque.on_input(self: Object, action: string, phase: string)
    if action == "Atacar" and phase == "pressed" then
        self.animation:play("Ataque")
    end
end

function Ataque.on_marker(self: Object, marker: string)
    if marker == "Impacto" then
        self:get_child("Área de acerto").collider.enabled = true
        self:after(ABERTA, Ataque.fechar)
    end
end

function Ataque.fechar(self: Object)
    self:get_child("Área de acerto").collider.enabled = false
end

return Ataque
