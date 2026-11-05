//! The colored icon before a file or a folder's name, taken from the Seti
//! icons of the Nerd Font (bundled by `iced_fonts`).

use iced::widget::{container, text};
use iced::{Color, Element, Length};
use std::path::Path;

/// A glyph of the Nerd Font and its color on a dark background.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Icon {
    pub glyph: char,
    pub color: Color,
}

const fn icon(glyph: u32, rgb: u32) -> Icon {
    Icon {
        glyph: match char::from_u32(glyph) {
            Some(c) => c,
            None => ' ',
        },
        color: Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8),
    }
}

const DEFAULT: Icon = icon(0xf016, 0x8089a0);
const TEXT: Icon = icon(0xe64e, 0x89e051);
const CONFIG: Icon = icon(0xe615, 0x8c9bb0);
const IMAGE: Icon = icon(0xe60d, 0xa074c4);
const ARCHIVE: Icon = icon(0xe6aa, 0xeca517);
const SHELL: Icon = icon(0xe691, 0x89e051);
const LOCK: Icon = icon(0xe672, 0xbbbbbb);
const DOCKER: Icon = icon(0xe650, 0x458ee6);
const NPM: Icon = icon(0xe71e, 0xe8274b);
const MAKE: Icon = icon(0xe673, 0x8c9bb0);

/// The icon of a file, by its whole name first, then by its extension.
pub fn of_file(path: &Path) -> Icon {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    if let Some(icon) = by_name(&name) {
        return icon;
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    by_ext(ext).unwrap_or(DEFAULT)
}

fn by_name(name: &str) -> Option<Icon> {
    Some(match name {
        "package.json" | "package-lock.json" | ".npmrc" | ".npmignore" => NPM,
        "tsconfig.json" | "jsconfig.json" => icon(0xe69d, 0x519aba),
        "cargo.lock" | "yarn.lock" | "composer.lock" | "gemfile.lock" | "poetry.lock" => LOCK,
        "pnpm-lock.yaml" | "pnpm-workspace.yaml" => icon(0xe865, 0xf9ad00),
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitkeep" | ".mailmap" => {
            icon(0xe65d, 0xf54d27)
        }
        ".gitlab-ci.yml" | ".gitlab-ci.yaml" => icon(0xe65c, 0xe24329),
        "dockerfile"
        | "containerfile"
        | ".dockerignore"
        | "compose.yml"
        | "compose.yaml"
        | "docker-compose.yml"
        | "docker-compose.yaml" => DOCKER,
        "makefile" | "gnumakefile" | "justfile" | "cmakelists.txt" => MAKE,
        "license" | "licence" | "copying" | "license.md" | "license.txt" => icon(0xe60a, 0xd0bf41),
        ".editorconfig" => icon(0xe652, 0x8c9bb0),
        ".babelrc" | "babel.config.js" => icon(0xe639, 0xcbcb41),
        "favicon.ico" => icon(0xe623, 0xcbcb41),
        _ if name.starts_with(".env") => icon(0xe615, 0xfaf743),
        _ if name.starts_with(".eslintrc") || name.starts_with("eslint.config.") => {
            icon(0xe655, 0x8c79e8)
        }
        _ if name.starts_with(".prettierrc") || name.starts_with("prettier.config.") => {
            icon(0xe6b4, 0x4285f4)
        }
        _ if name.starts_with("webpack.config.") => icon(0xe6a3, 0x519aba),
        _ if name.ends_with("ignore") && name.starts_with('.') => icon(0xe668, 0x8c9bb0),
        _ => return None,
    })
}

fn by_ext(ext: &str) -> Option<Icon> {
    Some(match ext {
        "rs" => icon(0xe68b, 0xdea584),
        "toml" => icon(0xe6b2, 0xc8743f),
        "json" | "jsonc" | "json5" => icon(0xf0626, 0xcbcb41),
        "ts" | "mts" | "cts" => icon(0xe628, 0x519aba),
        "tsx" => icon(0xe625, 0x3a7be0),
        "jsx" => icon(0xe625, 0x20c2e3),
        "js" | "mjs" | "cjs" => icon(0xe60c, 0xcbcb41),
        "py" | "pyi" | "pyw" => icon(0xe606, 0xffbc03),
        "go" => icon(0xe627, 0x00add8),
        "c" => icon(0xe61e, 0x599eff),
        "h" => icon(0xe61e, 0xa074c4),
        "cpp" | "cc" | "cxx" | "c++" => icon(0xe61d, 0x519aba),
        "hpp" | "hh" | "hxx" => icon(0xe61d, 0xa074c4),
        "cs" => icon(0xe648, 0x8dc149),
        "java" => icon(0xe66d, 0xcc3e44),
        "kt" | "kts" => icon(0xe634, 0x7f52ff),
        "swift" => icon(0xe699, 0xe37933),
        "dart" => icon(0xe64c, 0x3fa9e0),
        "scala" | "sc" => icon(0xe68e, 0xcc3e44),
        "php" => icon(0xe608, 0xa074c4),
        "rb" | "erb" => icon(0xe605, 0xcc3e44),
        "lua" => icon(0xe620, 0x51a0cf),
        "zig" => icon(0xe6a9, 0xf69a1b),
        "hs" => icon(0xe61f, 0xa074c4),
        "ex" | "exs" => icon(0xe62d, 0xa074c4),
        "clj" | "cljs" | "edn" => icon(0xe642, 0x8dc149),
        "r" => icon(0xe68a, 0x3d7fd6),
        "jl" => icon(0xe624, 0xa270ba),
        "ml" | "mli" => icon(0xe67a, 0xe37933),
        "elm" => icon(0xe62c, 0x519aba),
        "vim" => icon(0xe62b, 0x2fa84f),
        "pl" | "pm" => icon(0xe67e, 0x519aba),
        "gradle" | "groovy" => icon(0xe660, 0x5a9baf),
        "html" | "htm" => icon(0xe60e, 0xe44d26),
        "xml" | "xsd" | "xsl" | "plist" => icon(0xe619, 0xe37933),
        "svg" => icon(0xe698, 0xffb13b),
        "css" => icon(0xe614, 0x42a5f5),
        "scss" | "sass" => icon(0xe603, 0xf55385),
        "less" => icon(0xe60b, 0x8e6cc4),
        "styl" => icon(0xe600, 0x8dc149),
        "vue" => icon(0xe6a0, 0x8dc149),
        "svelte" => icon(0xe697, 0xff3e00),
        "astro" => icon(0xe6b3, 0xe23f67),
        "md" | "markdown" | "mdx" => icon(0xe609, 0x519aba),
        "txt" | "text" | "rst" => TEXT,
        "log" => icon(0xe64e, 0x8c9bb0),
        "yml" | "yaml" => icon(0xe6a8, 0xd06a8a),
        "ini" | "cfg" | "conf" | "properties" | "env" => CONFIG,
        "lock" => LOCK,
        "dockerfile" => DOCKER,
        "mk" => MAKE,
        "sh" | "bash" | "zsh" | "fish" | "ksh" => SHELL,
        "ps1" | "psm1" => icon(0xe683, 0x4273ca),
        "bat" | "cmd" => icon(0xe629, 0xc1f12e),
        "sql" | "db" | "sqlite" | "sqlite3" => icon(0xe64d, 0xdad8d8),
        "csv" | "tsv" => icon(0xe64a, 0x89e051),
        "xls" | "xlsx" | "ods" => icon(0xe6a6, 0x2e9e5a),
        "doc" | "docx" | "odt" => icon(0xe6a5, 0x4a86e8),
        "pdf" => icon(0xe67d, 0xe04e39),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "avif" => IMAGE,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => icon(0xe638, 0x66d8ef),
        "mp4" | "mkv" | "webm" | "mov" | "avi" => icon(0xe69f, 0xfd971f),
        "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "xz" | "bz2" | "zst" | "deb" => ARCHIVE,
        "ttf" | "otf" | "woff" | "woff2" => icon(0xe659, 0xd0d0d0),
        "graphql" | "gql" => icon(0xe662, 0xe535ab),
        "tf" | "tfvars" => icon(0xe69a, 0x7b63f0),
        "wasm" | "wat" => icon(0xe6a1, 0x7c6fe8),
        "tex" | "bib" => icon(0xe69b, 0x6ba539),
        "ipynb" => icon(0xe678, 0xf57d01),
        "prisma" => icon(0xe684, 0x5a67d8),
        "desktop" => CONFIG,
        _ => return None,
    })
}

/// The icon of a folder, open or not: a thin outline of the Lucide font, in
/// grey.
pub fn of_folder(open: bool) -> Icon {
    if open {
        icon(0xe247, 0x8089a0)
    } else {
        icon(0xe0db, 0x8089a0)
    }
}

/// The color to draw it with: darker on a light background, to stay legible.
pub fn color(icon: Icon, dark: bool) -> Color {
    if dark {
        icon.color
    } else {
        let c = icon.color;
        Color::from_rgb(c.r * 0.68, c.g * 0.68, c.b * 0.68)
    }
}

/// The icon, in a box 16 px wide, drawn with `font`.
fn view<'a, M: 'a>(icon: Icon, font: iced::Font, dark: bool) -> Element<'a, M> {
    container(
        text(icon.glyph)
            .font(font)
            .size(14)
            .color(color(icon, dark)),
    )
    .width(Length::Fixed(16.0))
    .center_x(Length::Fixed(16.0))
    .into()
}

/// The icon of a file, ready to put before its name.
pub fn file<'a, M: 'a>(path: &Path, dark: bool) -> Element<'a, M> {
    view(of_file(path), iced_fonts::NERD_FONT, dark)
}

/// The icon of a folder, ready to put before its name.
pub fn folder<'a, M: 'a>(open: bool, dark: bool) -> Element<'a, M> {
    view(of_folder(open), iced_fonts::LUCIDE_FONT, dark)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(name: &str) -> char {
        of_file(Path::new(name)).glyph
    }

    #[test]
    fn a_file_is_known_by_its_extension() {
        assert_eq!(glyph("src/main.rs"), '\u{e68b}');
        assert_eq!(glyph("App.TSX"), '\u{e625}');
        assert_eq!(glyph("index.ts"), '\u{e628}');
        assert_eq!(glyph("data.json"), '\u{f0626}');
        assert_eq!(glyph("install.sh"), '\u{e691}');
        assert_eq!(glyph("README.md"), '\u{e609}');
    }

    #[test]
    fn a_whole_name_wins_over_the_extension() {
        assert_eq!(glyph("package.json"), NPM.glyph);
        assert_eq!(glyph("pnpm-lock.yaml"), '\u{e865}');
        assert_eq!(glyph("compose.yml"), DOCKER.glyph);
        assert_eq!(glyph("Dockerfile"), DOCKER.glyph);
        assert_eq!(glyph(".gitlab-ci.yml"), '\u{e65c}');
        assert_eq!(glyph("Cargo.lock"), LOCK.glyph);
        assert_eq!(glyph(".env.local"), '\u{e615}');
        assert_eq!(glyph(".prettierignore"), '\u{e668}');
        assert_eq!(glyph(".gitignore"), '\u{e65d}');
        assert_eq!(glyph("LICENSE"), '\u{e60a}');
        assert_eq!(glyph("Makefile"), MAKE.glyph);
    }

    #[test]
    fn an_unknown_file_has_the_plain_icon() {
        assert_eq!(of_file(Path::new("notes.unknown")), DEFAULT);
        assert_eq!(of_file(Path::new("scratch1")), DEFAULT);
        assert_eq!(of_file(Path::new("")), DEFAULT);
    }

    #[test]
    fn every_folder_has_the_same_plain_icon_open_or_closed() {
        assert_ne!(of_folder(false).glyph, of_folder(true).glyph);
        assert_eq!(of_folder(false).color, of_folder(true).color);
        let face = ttf_parser::Face::parse(iced_fonts::LUCIDE_FONT_BYTES, 0)
            .expect("the Lucide font parses");
        for open in [false, true] {
            assert!(face.glyph_index(of_folder(open).glyph).is_some());
        }
    }

    #[test]
    fn every_glyph_is_in_the_font() {
        // Each codepoint used must exist in the bundled Nerd Font.
        let font = iced_fonts::NERD_FONT_BYTES;
        let face = ttf_parser::Face::parse(font, 0).expect("the Nerd Font parses");
        let names = [
            "a.rs",
            "a.toml",
            "a.json",
            "a.ts",
            "a.tsx",
            "a.jsx",
            "a.js",
            "a.py",
            "a.go",
            "a.c",
            "a.h",
            "a.cpp",
            "a.hpp",
            "a.cs",
            "a.java",
            "a.kt",
            "a.swift",
            "a.dart",
            "a.scala",
            "a.php",
            "a.rb",
            "a.lua",
            "a.zig",
            "a.hs",
            "a.ex",
            "a.clj",
            "a.r",
            "a.jl",
            "a.ml",
            "a.elm",
            "a.vim",
            "a.pl",
            "a.gradle",
            "a.html",
            "a.xml",
            "a.svg",
            "a.css",
            "a.scss",
            "a.less",
            "a.styl",
            "a.vue",
            "a.svelte",
            "a.astro",
            "a.md",
            "a.txt",
            "a.log",
            "a.yml",
            "a.ini",
            "a.lock",
            "a.mk",
            "a.sh",
            "a.ps1",
            "a.bat",
            "a.sql",
            "a.csv",
            "a.xlsx",
            "a.docx",
            "a.pdf",
            "a.png",
            "a.mp3",
            "a.mp4",
            "a.zip",
            "a.ttf",
            "a.gql",
            "a.tf",
            "a.wasm",
            "a.tex",
            "a.ipynb",
            "a.prisma",
            "a.x",
            "package.json",
            "tsconfig.json",
            "pnpm-lock.yaml",
            ".gitignore",
            ".gitlab-ci.yml",
            "Dockerfile",
            "Makefile",
            "LICENSE",
            ".editorconfig",
            ".babelrc",
            "favicon.ico",
            ".env",
            ".eslintrc",
            ".prettierrc",
            "webpack.config.js",
            ".npmignore",
            ".fooignore",
        ];
        let icons: Vec<Icon> = names.iter().map(|n| of_file(Path::new(n))).collect();
        for icon in icons {
            assert!(
                face.glyph_index(icon.glyph).is_some(),
                "missing glyph {:x}",
                icon.glyph as u32
            );
        }
    }

    #[test]
    fn a_light_background_darkens_the_color() {
        let icon = of_file(Path::new("a.js"));
        assert_eq!(color(icon, true), icon.color);
        assert!(color(icon, false).r < icon.color.r);
    }
}
