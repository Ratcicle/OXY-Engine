//! An immutable evaluation phase. Its API is retained by the performance harness.
use crate::document::*;
pub struct SceneView<'a> {
    pub scene: &'a Scene,
}
impl<'a> SceneView<'a> {
    pub fn new(scene: &'a Scene) -> Self {
        Self { scene }
    }
    pub fn entity(&self, id: &str) -> Option<&'a Entity> {
        self.scene.entity(id)
    }
    pub fn world_matrix(&self, id: &str) -> Result<glam::Mat4, String> {
        self.scene.world_matrix(id)
    }
    pub fn descendants(&self, id: &str) -> Vec<Id> {
        self.scene.descendants(id)
    }
}
