//! Scratch files: notes and trials that live outside any project, in one
//! folder, saved as they are typed and kept until the user deletes them.

use std::path::{Path, PathBuf};

/// Where scratch files go unless the Preferences say otherwise.
pub fn default_dir() -> PathBuf {
    crate::fsutil::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("simpleedit")
        .join("scratches")
}

/// The folder in use: the configured one, or the default.
pub fn dir_of(setting: &str) -> PathBuf {
    let setting = setting.trim();
    if setting.is_empty() {
        default_dir()
    } else {
        PathBuf::from(setting)
    }
}

/// Whether `path` is one of the scratch files kept in `dir`.
pub fn is_scratch(dir: &Path, path: &Path) -> bool {
    path.parent() == Some(dir)
}

/// The scratch files of a folder, by name.
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    files
}

/// `name` + `.ext`, refusing what is not a plain file name.
pub fn file_name(name: &str, ext: &str) -> Result<String, String> {
    let name = name.trim();
    let ext = ext.trim().trim_start_matches('.');
    let plain = |s: &str| !s.contains(['/', '\\', '\0']) && s != "." && s != "..";
    if name.is_empty() {
        return Err(t!("scratch.err_name").to_string());
    }
    if !plain(name) || !plain(ext) {
        return Err(t!("scratch.err_invalid").to_string());
    }
    Ok(if ext.is_empty() {
        name.to_string()
    } else {
        format!("{name}.{ext}")
    })
}

/// The name given to a file made without one: `scratch1`, `scratch2`...,
/// the first that is free in `dir` with that extension.
pub fn next_name(dir: &Path, ext: &str) -> String {
    (1..)
        .map(|n| format!("scratch{n}"))
        .find(|name| file_name(name, ext).is_ok_and(|file| !dir.join(file).exists()))
        .expect("an unbounded range")
}

/// Creates an empty scratch file; an existing one is never overwritten.
pub fn create(dir: &Path, name: &str, ext: &str) -> Result<PathBuf, String> {
    let file = file_name(name, ext)?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(file);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => t!("scratch.err_exists").to_string(),
            _ => e.to_string(),
        })?;
    Ok(path)
}

/// Moves the scratch files of `from` into `to` (a file whose name is taken
/// there stays where it was). Returns how many moved.
pub fn relocate(from: &Path, to: &Path) -> Result<usize, String> {
    if from == to {
        return Ok(0);
    }
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let mut moved = 0;
    for file in list(from) {
        let Some(name) = file.file_name() else {
            continue;
        };
        let target = to.join(name);
        if target.exists() {
            continue;
        }
        // A rename fails across disks: copy, then remove.
        if std::fs::rename(&file, &target).is_err() {
            std::fs::copy(&file, &target).map_err(|e| e.to_string())?;
            std::fs::remove_file(&file).map_err(|e| e.to_string())?;
        }
        moved += 1;
    }
    Ok(moved)
}

/// The extensions offered for a new file: those of every language the editor
/// colors, then a few common plain ones.
pub const EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "mjs", "jsx", "ts", "tsx", "html", "css", "scss", "less", "json", "toml",
    "yaml", "yml", "md", "sh", "mk", "c", "h", "cpp", "hpp", "cs", "java", "go", "rb", "php", "pl",
    "hs", "scala", "swift", "kt", "xml", "sql", "lua", "r", "tex", "diff", "txt", "csv", "ini",
    "log",
];

/// The small drawn icon of the button: a sheet with a folded corner and two
/// lines of writing.
struct Sheet {
    color: iced::Color,
}

impl<Message> iced::widget::canvas::Program<Message> for Sheet {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path, Stroke};
        use iced::Point;
        let mut frame = Frame::new(renderer, bounds.size());
        let stroke = Stroke::default().with_color(self.color).with_width(1.4);
        let (left, right, top, bottom, fold) = (3.0, 13.0, 1.5, 14.5, 4.0);
        // The outline, with the top right corner cut.
        let outline = Path::new(|p| {
            p.move_to(Point::new(right - fold, top));
            p.line_to(Point::new(left, top));
            p.line_to(Point::new(left, bottom));
            p.line_to(Point::new(right, bottom));
            p.line_to(Point::new(right, top + fold));
            p.close();
        });
        frame.stroke(&outline, stroke);
        // The fold.
        frame.stroke(
            &Path::new(|p| {
                p.move_to(Point::new(right - fold, top));
                p.line_to(Point::new(right - fold, top + fold));
                p.line_to(Point::new(right, top + fold));
            }),
            stroke,
        );
        for y in [8.0, 11.0] {
            frame.stroke(
                &Path::line(Point::new(left + 2.5, y), Point::new(right - 2.5, y)),
                stroke,
            );
        }
        vec![frame.into_geometry()]
    }
}

/// The icon, 16 px wide.
pub fn icon<'a, Message: 'a>(color: iced::Color) -> iced::Element<'a, Message> {
    iced::widget::canvas::Canvas::new(Sheet { color })
        .width(iced::Length::Fixed(16.0))
        .height(iced::Length::Fixed(16.0))
        .into()
}

/// An entry of the extension list: one of `EXTENSIONS`, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ext(pub Option<&'static str>);

impl Ext {
    /// "No extension" first, then every extension in alphabetical order.
    pub fn all() -> Vec<Ext> {
        let mut sorted = EXTENSIONS.to_vec();
        sorted.sort_unstable();
        std::iter::once(Ext(None))
            .chain(sorted.into_iter().map(|e| Ext(Some(e))))
            .collect()
    }

    pub fn as_str(self) -> &'static str {
        self.0.unwrap_or("")
    }
}

impl std::fmt::Display for Ext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(ext) => write!(f, ".{ext}"),
            None => f.write_str(&t!("scratch.no_ext")),
        }
    }
}

/// The panel under the editor: a form to make a file, and the files.
#[derive(Debug)]
pub struct Panel {
    pub open: bool,
    pub name: String,
    pub ext: Ext,
    pub error: Option<String>,
    pub files: Vec<PathBuf>,
    pub dir: PathBuf,
}

impl Panel {
    pub fn new(setting: &str) -> Self {
        let dir = dir_of(setting);
        Panel {
            open: false,
            name: String::new(),
            ext: Ext(Some("txt")),
            error: None,
            files: list(&dir),
            dir,
        }
    }

    pub fn refresh(&mut self) {
        self.files = list(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_of_the_editor_has_its_extension_in_the_list() {
        for ext in [
            "rs", "py", "js", "jsx", "ts", "tsx", "html", "css", "scss", "less", "json", "toml",
            "yaml", "md", "sh", "mk", "c", "cpp", "cs", "java", "go", "rb", "php", "pl", "hs",
            "scala", "swift", "kt", "xml", "sql", "lua", "r", "tex", "diff",
        ] {
            assert!(EXTENSIONS.contains(&ext), "{ext} is missing");
        }
        // Each one is known to the editor, and none comes twice.
        for ext in EXTENSIONS {
            let known = crate::editor::extension_to_token(ext).is_some()
                || ["txt", "csv", "ini", "log"].contains(ext);
            assert!(known, "{ext} is not recognized");
        }
        let mut sorted = EXTENSIONS.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), EXTENSIONS.len());
        assert_eq!(Ext::all()[0], Ext(None));
        assert_eq!(Ext::all().len(), EXTENSIONS.len() + 1);
        let listed: Vec<&str> = Ext::all().iter().skip(1).map(|e| e.as_str()).collect();
        assert!(listed.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn a_blank_setting_means_the_default_folder() {
        assert_eq!(dir_of("  "), default_dir());
        assert_eq!(dir_of("/x/notes"), PathBuf::from("/x/notes"));
    }

    #[test]
    fn names_and_extensions_make_a_plain_file_name() {
        assert_eq!(file_name("todo", "md").unwrap(), "todo.md");
        assert_eq!(file_name(" todo ", ".md").unwrap(), "todo.md");
        assert_eq!(file_name("Makefile", "").unwrap(), "Makefile");
        assert!(file_name("", "md").is_err());
        assert!(file_name("a/b", "md").is_err());
        assert!(file_name("a", "m/d").is_err());
        assert!(file_name("..", "").is_err());
    }

    #[test]
    fn created_files_are_listed_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("scratches");
        let a = create(&root, "b", "txt").unwrap();
        let b = create(&root, "A", "rs").unwrap();
        std::fs::write(&a, "keep").unwrap();
        assert!(create(&root, "b", "txt").is_err());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "keep");
        assert_eq!(list(&root), vec![b.clone(), a.clone()]);
        assert!(is_scratch(&root, &a));
        assert!(!is_scratch(&root, &dir.path().join("x.txt")));
    }

    #[test]
    fn relocating_moves_the_files_and_spares_taken_names() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("from"), dir.path().join("to"));
        let a = create(&from, "a", "txt").unwrap();
        create(&from, "b", "txt").unwrap();
        std::fs::write(&a, "text").unwrap();
        create(&to, "b", "txt").unwrap();
        assert_eq!(relocate(&from, &to).unwrap(), 1);
        assert_eq!(std::fs::read_to_string(to.join("a.txt")).unwrap(), "text");
        assert!(!a.exists());
        assert!(from.join("b.txt").exists());
        assert_eq!(relocate(&to, &to).unwrap(), 0);
    }
}
