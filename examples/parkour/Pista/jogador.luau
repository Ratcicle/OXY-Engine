--!strict
-- O corredor: largada, cronômetro, chegada com o recorde da pista, sons de pulo e pouso e a
-- volta ao checkpoint (R). O estado de cada corredor fica nos atributos dele, que os outros
-- scripts da pista leem; o HUD, o resultado e os sons são só do jogador local.
local Jogador = {}
Jogador.attributes = {
    Etapa = 0, -- 0 na largada, 1 correndo, 2 cruzou a chegada, 3 terminou
    Tempo = 0,
    Quedas = 0,
    ["Checkpoint atual"] = 0,
    ["Ponto de retorno"] = vector.create(0, 0, 0),
    ["Ponto de largada"] = vector.create(0, 0, 0),
    ["Jogador local"] = true,
    HUD = param.object(),
    Resultado = param.object(),
}
local function relogio(segundos: number): string
    return time.format(segundos, "mm:ss.cc")
end

function Jogador.on_start(self: Object)
    self.character:block("movement", "largada", true)
    self.attr["Ponto de retorno"] = self.world_position
    self.attr["Ponto de largada"] = self.world_position
    local recorde = vars.get("Melhor tempo", scene.current())
    if self.attr["Jogador local"] and recorde > 0 then
        Jogador.mostrar_recorde(self, recorde)
    end
end

function Jogador.on_step(self: Object, dt: number)
    local etapa = self.attr.Etapa
    if etapa == 0 then
        if vars.get("Largada liberada") then
            self.character:block("movement", "largada", false)
            self.attr.Etapa = 1
        end
    elseif etapa == 1 then
        local tempo = self.attr.Tempo + dt
        self.attr.Tempo = tempo
        if self.attr["Jogador local"] then
            self.attr.HUD:get_child("Tempo").ui.text = "Tempo " .. relogio(tempo)
        end
    elseif etapa == 2 then
        Jogador.terminar(self)
    end
end

-- Cruzou a chegada: o corredor para e, no jogador local, o resultado aparece e o recorde da
-- pista é conferido e salvo.
function Jogador.terminar(self: Object)
    self.attr.Etapa = 3
    self.character:block("movement", "chegada", true)
    if not self.attr["Jogador local"] then
        return
    end
    local resultado = self.attr.Resultado
    game.show_cursor()
    resultado.visible = true
    self.attr.HUD:get_child("Dica dos controles").visible = false
    audio.play("Chegada", { audience = self })
    local tempo = self.attr.Tempo
    local texto = relogio(tempo)
    resultado:get_child("Seu tempo").ui.text = "Seu tempo " .. texto
    local pista = scene.current()
    local recorde = vars.get("Melhor tempo", pista)
    if recorde > 0 and texto == relogio(recorde) then
        resultado:get_child("Recorde igualado").visible = true
    elseif recorde == 0 or tempo < recorde then
        vars.set("Melhor tempo", tempo, pista)
        save.write()
        Jogador.mostrar_recorde(self, tempo)
        resultado:get_child("Novo recorde").visible = true
    else
        local mantido = resultado:get_child("Recorde mantido")
        mantido.ui.text = "Recorde mantido · +" .. relogio(tempo - recorde)
        mantido.visible = true
    end
end

function Jogador.mostrar_recorde(self: Object, segundos: number)
    local texto = "Recorde " .. relogio(segundos)
    self.attr.HUD:get_child("Recorde").ui.text = texto
    self.attr.Resultado:get_child("Recorde do resultado").ui.text = texto
end

function Jogador.on_character(self: Object, event: CharacterEvent)
    if not self.attr["Jogador local"] then
        return
    end
    if event.kind == "jump" then
        audio.play("Pulo", { volume = 0.5, audience = self })
    elseif event.kind == "land" and event.impact >= 3 then
        audio.play("Pouso", { volume = 0.6, audience = self })
    end
end

-- R volta ao último checkpoint, olhando para a frente da pista, fora da pausa e antes da chegada.
function Jogador.on_input(self: Object, action: string, phase: string)
    if action ~= "Voltar ao checkpoint" or phase ~= "pressed" then
        return
    end
    if game.paused() or self.attr.Etapa >= 2 then
        return
    end
    self.character:teleport(
        self.attr["Ponto de retorno"],
        { search = 0.5, yaw = 0, look = vector.create(0, 0, 0) }
    )
end

return Jogador
