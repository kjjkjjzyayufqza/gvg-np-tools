//! Install a CJK-capable font from common OS locations so system error strings
//! (e.g. localized Windows IO errors) render instead of tofu.

use egui::{
    epaint::text::{FontData, FontFamily, FontInsert, FontPriority, InsertFontFamily},
    Context,
};
use std::path::{Path, PathBuf};

const CJK_FONT_ID: &str = "gvg-cjk-ui";

pub fn install_cjk_fonts(ctx: &Context) {
    let Some((font_bytes, index)) = try_read_first_cjk_font() else {
        eprintln!(
            "[gui] No CJK-capable font file found from known paths; \
             localized status messages may render missing glyphs."
        );
        return;
    };

    let insert = FontInsert::new(
        CJK_FONT_ID,
        FontData {
            font: std::borrow::Cow::Owned(font_bytes),
            index,
            tweak: Default::default(),
        },
        vec![
            InsertFontFamily {
                family: FontFamily::Proportional,
                priority: FontPriority::Highest,
            },
            InsertFontFamily {
                family: FontFamily::Monospace,
                priority: FontPriority::Highest,
            },
        ],
    );
    ctx.add_font(insert);
}

fn first_parseable_face(bytes: &[u8]) -> Option<u32> {
    match ttf_parser::fonts_in_collection(bytes) {
        Some(n) => {
            for i in 0..n.min(64) {
                if ttf_parser::Face::parse(bytes, i).is_ok() {
                    return Some(i);
                }
            }
            None
        }
        None => {
            if ttf_parser::Face::parse(bytes, 0).is_ok() {
                Some(0)
            } else {
                None
            }
        }
    }
}

fn try_read_first_cjk_font() -> Option<(Vec<u8>, u32)> {
    for path in candidate_cjk_paths() {
        if !path.exists() || !path.is_file() {
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[gui] Could not read font {}: {}", path.display(), e);
                continue;
            }
        };
        let Some(face) = first_parseable_face(&bytes) else {
            eprintln!("[gui] Skipping invalid font bytes: {}", path.display());
            continue;
        };
        eprintln!(
            "[gui] Using CJK font: {} (TTC/TTF face index {})",
            path.display(),
            face
        );
        return Some((bytes, face));
    }
    None
}

fn candidate_cjk_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            let fonts = Path::new(&windir).join("Fonts");
            v.push(fonts.join("msyh.ttc"));
            v.push(fonts.join("msyhbd.ttc"));
            v.push(fonts.join("simhei.ttf"));
            v.push(fonts.join("simsun.ttc"));
        }
        v.push(PathBuf::from(r"C:\Windows\Fonts\msyh.ttc"));
    }

    #[cfg(target_os = "macos")]
    {
        v.push(PathBuf::from("/System/Library/Fonts/PingFang.ttc"));
        v.push(PathBuf::from(
            "/System/Library/Fonts/Supplemental/Songti.ttc",
        ));
    }

    #[cfg(target_os = "linux")]
    {
        for p in [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        ] {
            v.push(PathBuf::from(p));
        }
    }

    v
}
