//! Presentation labels never change the stable serialized identifiers.
use oxy_core::document::{UiAnchor, UiKind};
pub fn primitive(primitive: oxy_core::document::Primitive) -> &'static str {
    use oxy_core::document::Primitive::*;
    match primitive {
        Rectangle => "Retângulo",
        Circle => "Círculo",
        Sprite => "Sprite",
        Cube => "Cubo",
        Sphere => "Esfera",
        Cylinder => "Cilindro",
        Plane => "Plano",
        Pyramid => "Pirâmide",
        Cone => "Cone",
        Tube => "Tubo",
    }
}
pub fn ui_kind(kind: UiKind) -> &'static str {
    match kind {
        UiKind::Text => "Texto",
        UiKind::Image => "Imagem",
        UiKind::Button => "Botão",
        UiKind::Bar => "Barra",
    }
}
pub fn anchor(anchor: UiAnchor) -> &'static str {
    match anchor {
        UiAnchor::TopLeft => "Superior esquerdo",
        UiAnchor::TopRight => "Superior direito",
        UiAnchor::BottomLeft => "Inferior esquerdo",
        UiAnchor::BottomRight => "Inferior direito",
        UiAnchor::Center => "Centro",
    }
}
pub fn key(key: &str) -> &str {
    match key {
        "Space" => "Espaço",
        "ArrowLeft" => "Seta para a esquerda",
        "ArrowRight" => "Seta para a direita",
        "ArrowUp" => "Seta para cima",
        "ArrowDown" => "Seta para baixo",
        "Escape" => "Esc",
        "Backspace" => "Apagar à esquerda",
        "Delete" => "Excluir",
        "Insert" => "Inserir",
        "Home" => "Início",
        "End" => "Fim",
        "PageUp" => "Página acima",
        "PageDown" => "Página abaixo",
        "Tab" => "Tabulação",
        "Enter" => "Enter",
        "Copy" => "Copiar",
        "Cut" => "Recortar",
        "Paste" => "Colar",
        "Colon" => "Dois-pontos (:)",
        "Comma" => "Vírgula (,)",
        "Backslash" => "Barra invertida (\\)",
        "Slash" => "Barra (/)",
        "Pipe" => "Barra vertical (|)",
        "Questionmark" => "Interrogação (?)",
        "Exclamationmark" => "Exclamação (!)",
        "OpenBracket" => "Abre colchete ([)",
        "CloseBracket" => "Fecha colchete (])",
        "OpenCurlyBracket" => "Abre chave ({)",
        "CloseCurlyBracket" => "Fecha chave (})",
        "Backtick" => "Acento grave (`)",
        "Minus" => "Menos (-)",
        "Period" => "Ponto (.)",
        "Plus" => "Mais (+)",
        "Equals" => "Igual (=)",
        "Semicolon" => "Ponto e vírgula (;)",
        "Quote" => "Aspas simples (')",
        "BrowserBack" => "Voltar no navegador",
        _ => key,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visible_enum_labels_are_portuguese() {
        assert_eq!(ui_kind(UiKind::Bar), "Barra");
        assert_eq!(anchor(UiAnchor::TopLeft), "Superior esquerdo");
        assert_eq!(anchor(UiAnchor::TopRight), "Superior direito");
        assert_eq!(anchor(UiAnchor::BottomLeft), "Inferior esquerdo");
        assert_eq!(anchor(UiAnchor::BottomRight), "Inferior direito");
        assert_eq!(anchor(UiAnchor::Center), "Centro");
        assert_eq!(key("Space"), "Espaço");
    }
}
