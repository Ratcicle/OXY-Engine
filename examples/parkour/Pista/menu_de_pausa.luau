--!strict
-- O menu de pausa (Esc) aparece por cima do resultado e da contagem; ao retomar, os dois voltam
-- se ainda valem.
local Pausa = {}
Pausa.attributes = {
    Jogador = param.object(),
    Corrida = param.object(),
    Resultado = param.object(),
    Contagem = param.object(),
}

function Pausa.on_pause(self: Object, paused: boolean)
    self.visible = paused
    if paused then
        self.attr.Resultado.visible = false
        self.attr.Contagem.visible = false
        return
    end
    if self.attr.Jogador.attr.Etapa == 3 then
        self.attr.Resultado.visible = true
    end
    if self.attr.Corrida.attr.Aviso ~= "" then
        self.attr.Contagem.visible = true
    end
end

return Pausa
