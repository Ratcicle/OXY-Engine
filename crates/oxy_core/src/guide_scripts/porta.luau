--!strict
-- Porta: a mensagem "abrir" libera a passagem pelos segundos que vieram com ela, e um
-- temporizador a fecha de novo.
local Porta = {}

function Porta.on_message(self: Object, name: string, value: any, from: Object)
    if name == "abrir" then
        self.collider.enabled = false
        self:after(value or 2, Porta.fechar)
    end
end

function Porta.fechar(self: Object)
    self.collider.enabled = true
end

return Porta
