--!strict
-- Quem cai volta ao ponto de retorno; sem espaço ali, procura mais longe e depois tenta o ponto
-- de largada. Cada volta conta uma queda.
local Queda = {}
local PERTO: TeleportOptions = { search = 0.5, yaw = 0, look = vector.create(0, 0, 0) }
local LONGE: TeleportOptions = { search = 2, yaw = 0, look = vector.create(0, 0, 0) }

function Queda.on_area_enter(self: Object, other: Object)
    if not other:has_attribute("Ponto de retorno") then
        return
    end
    local personagem = other.character
    local ok, motivo = personagem:teleport(other.attr["Ponto de retorno"], PERTO)
    if not ok then
        ok, motivo = personagem:teleport(other.attr["Ponto de retorno"], LONGE)
    end
    if not ok then
        ok, motivo = personagem:teleport(other.attr["Ponto de largada"], LONGE)
    end
    if not ok then
        print("Sem espaço para devolver quem caiu, nem no ponto de retorno nem no ponto de largada:", other)
        print("Motivo da recusa do último teleporte:", motivo)
        return
    end
    other.attr.Quedas += 1
    if other.attr["Jogador local"] then
        audio.play("Queda", { audience = other })
    end
end

return Queda
