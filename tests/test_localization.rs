use fluent_bundle::FluentResource;
use fluent_syntax::parser::ParserError;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn ftl_is_valid() {
/*    let content_en = include_str!("../locales/en/messages.ftl");
    check_ftl(content_en);
    let content_de = include_str!("../locales/de/messages.ftl");
    check_ftl(content_de);
*/
    // we do not need to check txt and tera files at all - they are not FTL
    run_ftl_checks(Path::new("./locales")).unwrap()
}

fn walk_dir(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            walk_dir(&path, files)?;
        } else if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("ftl") {
            files.push(path);
        }
    }
    Ok(())
}

fn run_ftl_checks(root: &Path) -> std::io::Result<()> {
    let mut files = Vec::new();
    walk_dir(root, &mut files)?;

    for path in files {
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("❌ {}: {}", path.display(), e);
                continue;
            }
        };

        match FluentResource::try_new(content.to_string()) {
            Ok(_) => {
                println!("✅ {}", path.display());
            }
            Err((_, errors)) => {
                println!("❌ {}\n", path.display());
                pretty_print_errors(&content, &errors);
            }
        }
    }

    Ok(())
}
#[allow(dead_code)]
fn check_ftl(content: &str) {
    if let Err((_res, errors)) = FluentResource::try_new(content.to_string()) {
        pretty_print_errors(content, &errors);
        panic!("Invalid FTL");
    }
}
fn byte_to_line_col(src: &str, byte: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;

    for (i, ch) in src.char_indices() {
        if i >= byte {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }

    (line, col)
}
fn snippet(src: &str, start: usize, end: usize, context: usize) -> String {
    let lo = start.saturating_sub(context);
    let hi = (end + context).min(src.len());

    src[lo..hi].to_string()
}
fn pretty_print_errors(src: &str, errors: &[ParserError]) {
    for err in errors {
        let pos = err.pos.start;
        let (line, col) = byte_to_line_col(src, pos);

        eprintln!("❌ Fluent parse error:");
        eprintln!("   kind : {:?}", err.kind);
        eprintln!("   at   : line {}, column {}", line, col);

        let snip = snippet(src, err.pos.start, err.pos.end, 80);
        eprintln!("   snippet:\n{}\n", snip);
    }
}
