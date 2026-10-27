--!strict
-- O menu: cursor livre, as barras das opções com os volumes salvos e o melhor tempo da pista.
-- Esc no menu fecha as opções, sem pausar.
local Menu = {}
Menu.attributes = { ["Menu principal"] = param.object(), ["Painel de opções"] = param.object() }
local PISTA = "Pista" -- a cena cujo recorde o menu mostra

function Menu.on_start(self: Object)
    game.show_cursor()
    local opcoes = self.attr["Painel de opções"]
    opcoes:get_child("Barra da música").attr.Percentual = math.round(vars.get("Volume da música") * 100)
    opcoes:get_child("Barra dos efeitos").attr.Percentual = math.round(vars.get("Volume dos efeitos") * 100)
    local recorde = vars.get("Melhor tempo", PISTA)
    if recorde > 0 then
        local texto = "Melhor tempo " .. time.format(recorde, "mm:ss.cc")
        self.attr["Menu principal"]:get_child("Melhor tempo do menu").ui.text = texto
    end
end

function Menu.on_pause(self: Object, paused: boolean)
    if paused then
        self.attr["Painel de opções"].visible = false
        self.attr["Menu principal"].visible = true
        game.resume()
    end
end

return Menu
