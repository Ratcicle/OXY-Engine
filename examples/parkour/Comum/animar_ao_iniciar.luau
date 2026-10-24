--!strict
-- Toca o clipe de nome Clipe desde o começo da cena: os discos que giram, a balsa e o elevador.
local Animar = {}
Animar.attributes = { Clipe = "Giro" }

function Animar.on_start(self: Object)
    self.animation:play(self.attr.Clipe, true)
end

return Animar
