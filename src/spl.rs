//! Import of Windows Soundpad sound lists (`.spl`, XML with `<Sound url="…" title="…"/>`).
//! Windows paths rarely exist on Linux, so files are looked up by name next to the list.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Imported {
    pub title: String,
    pub path: Option<PathBuf>,
    pub original: String,
}

pub fn parse(spl: &Path) -> Result<Vec<Imported>, String> {
    let text = std::fs::read_to_string(spl).map_err(|e| e.to_string())?;
    let base = spl.parent().unwrap_or(Path::new("."));
    let index = index_files(base, 6);
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("<Sound ") {
        rest = &rest[i + 7..];
        let tag = &rest[..rest.find('>').unwrap_or(rest.len())];
        let Some(url) = attr(tag, "url") else { continue };
        let file_name = url.rsplit(['\\', '/']).next().unwrap_or(&url).to_string();
        let title = attr(tag, "title")
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| file_name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(file_name.clone()));
        out.push(Imported { title, path: resolve(&url, &file_name, &index), original: url });
    }
    if out.is_empty() {
        return Err(tr!("no sounds found in the file", "в файле не найдено звуков").into());
    }
    Ok(out)
}

fn resolve(url: &str, file_name: &str, index: &HashMap<String, PathBuf>) -> Option<PathBuf> {
    let direct = PathBuf::from(url);
    if direct.is_file() {
        return Some(direct);
    }
    // Wine/Proton: Z:\home\user\… is the Linux root.
    if let Some(rest) = url.strip_prefix("Z:\\").or_else(|| url.strip_prefix("z:\\")) {
        let p = PathBuf::from(format!("/{}", rest.replace('\\', "/")));
        if p.is_file() {
            return Some(p);
        }
    }
    index.get(&file_name.to_lowercase()).cloned()
}

/// Lower-cased file name → path for every file under `dir` (limited depth).
fn index_files(dir: &Path, depth: usize) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, level)) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                if level < depth {
                    stack.push((p, level + 1));
                }
            } else if let Some(name) = p.file_name() {
                map.entry(name.to_string_lossy().to_lowercase()).or_insert(p);
            }
        }
    }
    map
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let mut search = tag;
    loop {
        let i = search.find(&key)?;
        // Make sure we matched a whole attribute name (e.g. not "xurl=").
        if i == 0 || search.as_bytes()[i - 1].is_ascii_whitespace() {
            let v = &search[i + key.len()..];
            return Some(unescape(&v[..v.find('"')?]));
        }
        search = &search[i + key.len()..];
    }
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else { break };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_attributes_and_entities() {
        let tag = r#"url="C:\Sounds\Tom &amp; Jerry.mp3" title="Tom &amp; Jerry &#1055;" artist="""#;
        assert_eq!(attr(tag, "url").unwrap(), r"C:\Sounds\Tom & Jerry.mp3");
        assert_eq!(attr(tag, "title").unwrap(), "Tom & Jerry П");
        assert_eq!(attr(tag, "missing"), None);
    }
}
