--!strict
-- Área que conta as visitas: o total é um atributo (um texto da interface pode mostrá-lo), o
-- recorde é a variável do jogo Recorde e os nomes de quem entrou ficam na memória do script.
local Contador = {}
Contador.attributes = { Visitas = 0 }
Contador.state = { nomes = {} :: { string } }

function Contador.on_area_enter(self: Object, other: Object)
    self.attr.Visitas += 1
    table.insert(self.state.nomes, other.name)
    if self.attr.Visitas > vars.get("Recorde") then
        vars.set("Recorde", self.attr.Visitas)
    end
end

return Contador
