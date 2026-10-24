--!strict
-- Área da porta: quem entra com o atributo Chave verdadeiro abre a porta (o pai desta área),
-- que perde o colisor e some. O atributo é lido de quem entrou; sem a chave, a porta fica.
local Porta = {}

function Porta.on_area_enter(self: Object, other: Object)
    local porta = assert(self.parent, "a área fica dentro da porta")
    if other:has_attribute("Chave") and other.attr.Chave == true then
        porta.collider.enabled = false
        porta.visible = false
    else
        print("A porta exige uma chave no objeto que entrou.")
    end
end

return Porta
