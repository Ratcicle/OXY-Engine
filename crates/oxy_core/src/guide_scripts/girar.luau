--!strict
local Girar = {}
Girar.attributes = { Velocidade = 90 } -- graus por segundo

function Girar.on_step(self: Object, dt: number)
    self.rotation += vector.create(0, self.attr.Velocidade * dt, 0)
end

return Girar
