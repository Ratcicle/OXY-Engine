--!strict
-- Alavanca: um clique manda a Porta escolhida no atributo Porta abrir por 3 segundos.
local Alavanca = {}
Alavanca.attributes = { Porta = param.object() }

function Alavanca.on_click(self: Object)
    local porta = self.attr.Porta
    if porta then
        porta:send("abrir", 3)
    end
end

return Alavanca
