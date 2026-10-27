--!strict
-- A contagem da largada: 3, 2, 1, Já! O HUD mostra o atributo Aviso, e o Painel da contagem
-- some 1 s depois do Já!. É o estado da partida, o mesmo para todos os corredores.
local Corrida = {}
Corrida.attributes = { Aviso = "", Painel = param.object() }

function Corrida.on_start(self: Object)
    vars.set("Largada liberada", false)
    Corrida.mostrar(self, 3)
    self:schedule({
        { 1, Corrida.mostrar, 2 },
        { 1, Corrida.mostrar, 1 },
        { 1, Corrida.mostrar, 0 },
        { 1, Corrida.esconder_painel },
    })
end

function Corrida.mostrar(self: Object, n: number)
    vars.set("Contagem da largada", n)
    if n > 0 then
        self.attr.Aviso = tostring(n)
        audio.play("Bipe", { volume = 0.8 })
    else
        self.attr.Aviso = "Já!"
        vars.set("Largada liberada", true)
        audio.play("Largada")
    end
end

function Corrida.esconder_painel(self: Object)
    self.attr.Painel.visible = false
    self.attr.Aviso = ""
end

return Corrida
