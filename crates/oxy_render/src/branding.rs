//! The OXY icon, for the editor's window and the windows of exported games.

const OXY_PNG: &[u8] = include_bytes!("../../../assets/branding/oxy.png");

/// The OXY icon as window icons take it.
pub fn oxy_icon() -> egui::IconData {
    icon_from_png(OXY_PNG).expect("Ícone OXY incluído no executável")
}

/// A window icon from the bytes of a PNG image.
pub fn icon_from_png(bytes: &[u8]) -> Result<egui::IconData, String> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|e| format!("PNG inválido: {e}"))?
        .into_rgba8();
    Ok(egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

#[test]
fn the_oxy_icon_is_a_square_rgba_image() {
    let icon = oxy_icon();
    assert!(icon.width >= 32 && icon.width == icon.height);
    assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
    assert!(
        icon_from_png(b"texto")
            .unwrap_err()
            .contains("PNG inválido")
    );
}
