//! Whole-object vertex alignment, independent of editable topology.
use crate::{
    document::{Id, Scene},
    editing,
};
use glam::{Mat4, Vec3};
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Method {
    Move,
    Scale { axis: usize, pivot: Vec3 },
}
pub fn delta(source: Vec3, target: Vec3, method: Method) -> Result<Mat4, String> {
    if !source.is_finite() || !target.is_finite() {
        return Err("Os pontos precisam ser finitos.".into());
    }
    match method {
        Method::Move => Ok(Mat4::from_translation(target - source)),
        Method::Scale { axis, pivot } => {
            if axis > 2 || !pivot.is_finite() {
                return Err("Escolha o eixo e um pivô válido.".into());
            }
            let a = source - pivot;
            let b = target - pivot;
            let tolerance = 1e-5 * (1. + a.length().max(b.length()));
            if (0..3).any(|i| i != axis && (a[i] - b[i]).abs() > tolerance) {
                return Err(
                    "O destino não está no eixo escolhido. Use Mover ou escolha um ponto alinhado."
                        .into(),
                );
            }
            if a[axis].abs() < tolerance {
                return Err("O ponto de origem está sobre o pivô nesse eixo; não há fator de escala definido.".into());
            }
            let factor = b[axis] / a[axis];
            if !factor.is_finite() || factor.abs() < 1e-5 {
                return Err("O alinhamento produziria escala nula.".into());
            }
            let mut scale = Vec3::ONE;
            scale[axis] = factor;
            Ok(Mat4::from_translation(pivot)
                * Mat4::from_scale(scale)
                * Mat4::from_translation(-pivot))
        }
    }
}
pub fn apply(
    scene: &mut Scene,
    selection: &[Id],
    source: Vec3,
    target: Vec3,
    method: Method,
) -> Result<(), String> {
    editing::transform_selection(scene, selection, delta(source, target, method)?)
}
