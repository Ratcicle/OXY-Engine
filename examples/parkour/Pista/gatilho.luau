--!strict
-- Ladrilho que cai: um arquivo para os 12 gatilhos, cada um dentro do seu ladrilho e no modelo
-- Ladrilho que cai. Pisou, o ladrilho some 0,45 s depois e volta 3 s mais tarde.
local Gatilho = {}

function Gatilho.on_area_enter(self: Object)
    self.collider.enabled = false
    self:after(0.45, Gatilho.cair)
end

function Gatilho.cair(self: Object)
    local ladrilho = assert(self.parent, "o gatilho fica dentro do ladrilho")
    ladrilho.collider.enabled = false
    ladrilho.visible = false
    self:after(3, Gatilho.voltar)
end

function Gatilho.voltar(self: Object)
    local ladrilho = assert(self.parent, "o gatilho fica dentro do ladrilho")
    ladrilho.collider.enabled = true
    ladrilho.visible = true
    self.collider.enabled = true
end

return Gatilho
