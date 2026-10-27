--!strict
-- Quem cruza a linha correndo passa à etapa 2; o script do jogador termina a corrida.
local Chegada = {}

function Chegada.on_area_enter(self: Object, other: Object)
    if other.attr.Etapa == 1 then
        other.attr.Etapa = 2
    end
end

return Chegada
