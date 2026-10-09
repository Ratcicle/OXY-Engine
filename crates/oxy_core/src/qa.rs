//! Where opt-in native QA writes captures and reports. Generated evidence stays out of
//! version control: `target/qa/<relative>` by default, or `$OXY_QA_DIR/<relative>`.
use std::path::{Path, PathBuf};

pub fn output_dir(relative: &str) -> PathBuf {
    std::env::var_os("OXY_QA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/qa"))
        .join(relative)
}
