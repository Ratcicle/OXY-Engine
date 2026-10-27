--!strict
-- Área de checkpoint: um arquivo para os 6 checkpoints da pista e o modelo Checkpoint. Quem
-- passa por um checkpoint mais adiante guarda a Ordem dele e volta ali, nos pés, ao cair.
local Checkpoint = {}
Checkpoint.attributes = { Ordem = 1 }
local PES = vector.create(0, 1.5, 0) -- do centro da área até o chão

function Checkpoint.on_area_enter(self: Object, other: Object)
    if not other:has_attribute("Checkpoint atual") then
        return -- uma caixa de física também entra na área
    end
    local ordem = self.attr.Ordem
    if other.attr["Checkpoint atual"] < ordem then
        other.attr["Ponto de retorno"] = self.world_position - PES
        other.attr["Checkpoint atual"] = ordem
        if other.attr["Jogador local"] then
            audio.play("Checkpoint", { volume = 0.9, audience = other })
        end
    end
end

return Checkpoint
