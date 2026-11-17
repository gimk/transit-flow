//! The design file (pivot plan §3.4): the player's `Design` as JSON, one file per level in
//! `<data dir>/designs/<level id>.json`. It is written when a run starts, and read when
//! play mode opens a level. The data dir is `data_dir`.

use std::path::PathBuf;

use tf_sim::street::Design;

/// Where designs and stars go: the platform's per-user data folder,
/// `$TRANSIT_FLOW_DATA_DIR` if set (tests).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("TRANSIT_FLOW_DATA_DIR") {
        return dir.into();
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home().map(|home| home.join("Library/Application Support"))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|home| home.join(".local/share")))
    };
    base.unwrap_or_else(|| PathBuf::from("."))
        .join("transit-flow")
}

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

/// The stars of a level (pivot plan §6: a star belongs to a design), next to the design
/// file as `<level id>.stars.json`: the stars the saved design has won, the key of that
/// design (so stars are only believed for the design that won them), and the best count any
/// design reached (which unlocks the stages).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StarsFile {
    pub stars: usize,
    pub best: usize,
    pub key: u64,
}

impl StarsFile {
    pub fn new(design: &Design, stars: usize, best: usize) -> Self {
        Self {
            stars,
            best,
            key: design_key(design),
        }
    }
}

/// A hash of the design's JSON (FNV-1a, so it is the same on every platform and run).
pub fn design_key(design: &Design) -> u64 {
    design
        .to_json()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

pub fn stars_path(level_id: &str) -> PathBuf {
    data_dir()
        .join("designs")
        .join(format!("{level_id}.stars.json"))
}

pub fn save_stars(level_id: &str, record: &StarsFile) -> Result<(), String> {
    let path = stars_path(level_id);
    let folder = path.parent().expect("a stars path has a folder");
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let text = serde_json::to_string_pretty(record).expect("stars always serialize");
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))
}

/// `Ok(None)` when there is no file yet.
pub fn load_stars(level_id: &str) -> Result<Option<StarsFile>, String> {
    let path = stars_path(level_id);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| format!("{}: {error}", path.display()))
}
