--!strict
-- Área de acerto: tira o Dano da Vida de quem entra, uma vez por ativação da área, mesmo que o
-- alvo continue dentro dela. O ataque liga e desliga a área a cada golpe.
local Acerto = {}
Acerto.attributes = { Dano = 15 }

function Acerto.on_area_enter(self: Object, other: Object)
    if other:has_attribute("Vida") and self:first_hit(other) then
        other.attr.Vida = math.max(other.attr.Vida - self.attr.Dano, 0)
    end
end

return Acerto
