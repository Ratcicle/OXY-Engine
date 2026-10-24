--!strict
-- Carta de interface: o clique gasta o Custo da Energia do Estado e tira o Dano da Vida do
-- Alvo, só quando a energia basta. Sem energia, nada muda e o Console explica.
local Carta = {}
Carta.attributes = { Custo = 2, Dano = 20, Estado = param.object(), Alvo = param.object() }

function Carta.on_click(self: Object)
    local estado = self.attr.Estado
    if estado.attr.Energia < self.attr.Custo then
        print("Energia insuficiente: a carta não alterou energia nem vida.")
        return
    end
    estado.attr.Energia -= self.attr.Custo
    local alvo = self.attr.Alvo
    alvo.attr.Vida = math.max(alvo.attr.Vida - self.attr.Dano, 0)
end

return Carta
