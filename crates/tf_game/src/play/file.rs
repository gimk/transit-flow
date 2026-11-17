//! The design file (pivot plan §3.4): the player's `Design` as JSON, one file per level in
//! `<data dir>/designs/<level id>.json`. It is written when a run starts, and read when
//! play mode opens a level. The data dir is the one of the quicksave (`game::data_dir`).

use std::path::PathBuf;

use tf_sim::street::Design;

use crate::game::data_dir;

pub fn design_path(level_id: &str) -> PathBuf {
    data_dir().join("designs").join(format!("{level_id}.json"))
}

pub fn save_design(design: &Design) -> Result<PathBuf, String> {
    let path = design_path(&design.level);
    let folder = path.parent().expect("a design path has a folder");
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    std::fs::write(&path, design.to_json())
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}

/// `Ok(None)` when there is no file yet; `Err` with the reason when it cannot be used
/// (unreadable, bad JSON, wrong version, or made for another level).
pub fn load_design(level_id: &str) -> Result<Option<Design>, String> {
    let path = design_path(level_id);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let design =
        Design::from_json(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    if design.level != level_id {
        return Err(format!(
            "{}: the design is for level '{}'",
            path.display(),
            design.level
        ));
    }
    Ok(Some(design))
}
