use std::fs;
use std::path::Path;
use std::process::ExitCode;

struct Split {
    index: String,
    parts: Vec<(String, String)>,
}

fn heading(line: &str) -> Option<&str> {
    line.strip_prefix("## ")
}

fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(48);
    let out = out.trim_matches('-');
    if out.is_empty() { "part" } else { out }.to_string()
}

fn deeper(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("](") {
        let (head, tail) = rest.split_at(at + 2);
        out.push_str(head);
        let target = tail.split(')').next().unwrap_or("");
        let local = !target.is_empty()
            && !target.starts_with(['#', '/'])
            && !target.contains(':')
            && !target.contains(char::is_whitespace);
        if local {
            out.push_str("../");
        }
        rest = tail;
    }
    out + rest
}

fn split(stem: &str, text: &str) -> Option<Split> {
    let mut preamble = String::new();
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let marker = ["```", "~~~"]
            .into_iter()
            .find(|m| line.trim_start().starts_with(m));
        match (fence, marker) {
            (None, Some(m)) => fence = Some(m),
            (Some(open), Some(m)) if open == m => fence = None,
            _ => {}
        }
        match heading(line).filter(|_| fence.is_none()) {
            Some(title) => sections.push((title.trim().to_string(), String::new())),
            None => {
                let into = sections.last_mut().map_or(&mut preamble, |s| &mut s.1);
                into.push_str(line);
                into.push('\n');
            }
        }
    }
    if sections.is_empty() {
        return None;
    }
    let mut index = format!("{}\n", preamble.trim_end());
    let mut parts: Vec<(String, String)> = Vec::new();
    let titles: Vec<String> = sections.iter().map(|(t, _)| t.clone()).collect();
    for (title, body) in sections {
        let mut name = slug(&title);
        let taken = |name: &str| titles.iter().any(|t| slug(t) == name);
        if parts.iter().any(|(n, _)| *n == format!("{name}.md")) {
            let n = (2..).find(|n| {
                let tried = format!("{name}-{n}");
                !parts.iter().any(|(p, _)| *p == format!("{tried}.md")) && !taken(&tried)
            });
            name = format!("{name}-{}", n.unwrap());
        }
        let file = format!("{name}.md");
        let shown = title.replace('[', "\\[").replace(']', "\\]");
        let sep = if parts.is_empty() { "\n" } else { "" };
        index.push_str(&format!("{sep}- [{shown}]({stem}/{file})\n"));
        parts.push((file, format!("# {title}\n\n{}\n", deeper(body.trim()))));
    }
    Some(Split { index, parts })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [file] = args.as_slice() else {
        eprintln!("usage: scripts/split-agents docs/agents/<file>.md");
        return ExitCode::FAILURE;
    };
    let path = Path::new(file);
    let (Some(stem), Some(dir)) = (path.file_stem(), path.parent()) else {
        eprintln!("{file}: not a Markdown file");
        return ExitCode::FAILURE;
    };
    let stem = stem.to_string_lossy();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(split) = split(&stem, &text) else {
        println!("{file}: no second-level heading to split by, nothing changed");
        return ExitCode::SUCCESS;
    };
    let parts_dir = dir.join(&*stem);
    for (name, part) in &split.parts {
        let existing = fs::read_to_string(parts_dir.join(name));
        if existing.is_ok_and(|e| e != *part) {
            eprintln!(
                "{}: exists with other text, nothing written",
                parts_dir.join(name).display()
            );
            return ExitCode::FAILURE;
        }
    }
    let written = fs::create_dir_all(&parts_dir)
        .and_then(|()| {
            (split.parts.iter()).try_for_each(|(name, part)| fs::write(parts_dir.join(name), part))
        })
        .and_then(|()| fs::write(path, &split.index));
    if let Err(e) = written {
        eprintln!("{file}: {e}");
        return ExitCode::FAILURE;
    }
    println!(
        "{file}: {} parts in {}/",
        split.parts.len(),
        parts_dir.display()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# Title\n\nIntro, see [map](../architecture.md) and [site](https://x.y/z).\n\n\
        ## First part (#1)\n\nBody with [a](a.md#top) and [self](#here).\n\n```sh\n## not a heading\n```\n\n\
        ## Second\n\nMore.\n\n## First part (#1)\n\nSame title again.\n";

    #[test]
    fn each_second_level_section_becomes_a_part_and_the_index_lists_them() {
        let split = split("doc", TEXT).unwrap();
        let names: Vec<&str> = split.parts.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["first-part-1.md", "second.md", "first-part-1-2.md"]);
        assert_eq!(
            split.index,
            "# Title\n\nIntro, see [map](../architecture.md) and [site](https://x.y/z).\n\n\
             - [First part (#1)](doc/first-part-1.md)\n- [Second](doc/second.md)\n\
             - [First part (#1)](doc/first-part-1-2.md)\n"
        );
        let first = &split.parts[0].1;
        assert!(
            first.starts_with("# First part (#1)\n\nBody with [a](../a.md#top) and [self](#here).")
        );
        assert!(first.contains("```sh\n## not a heading\n```"));
        assert!(first.ends_with('\n') && !first.ends_with("\n\n"));
    }

    #[test]
    fn running_on_its_own_output_changes_nothing() {
        let split = split("doc", TEXT).unwrap();
        assert!(super::split("doc", &split.index).is_none());
        for (_, part) in &split.parts {
            assert!(super::split("doc", part).is_none());
        }
    }

    #[test]
    fn a_fence_closes_on_its_own_marker_and_a_listed_preamble_keeps_its_blank_line() {
        let text = "# T\n\n- [old](x.md)\n\n## A\n\n~~~\n```\n## inside\n~~~\n\n## B\n\nb\n";
        let split = split("doc", text).unwrap();
        assert_eq!(split.parts.len(), 2);
        assert!(split.parts[0].1.contains("## inside"));
        assert_eq!(
            split.index,
            "# T\n\n- [old](x.md)\n\n- [A](doc/a.md)\n- [B](doc/b.md)\n"
        );
    }

    #[test]
    fn a_file_without_second_level_headings_is_left_alone() {
        assert!(split("doc", "# Only\n\n### Deeper\n\ntext\n").is_none());
    }

    #[test]
    fn a_slug_is_short_ascii() {
        assert_eq!(
            slug("Named workspaces: acceptance (#347)"),
            "named-workspaces-acceptance-347"
        );
        assert_eq!(slug("???"), "part");
        assert!(slug(&"word ".repeat(40)).len() <= 48);
    }
}
