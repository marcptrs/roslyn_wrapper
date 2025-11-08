use std::path::{Path, PathBuf};

pub fn url_to_path(uri: &str) -> Result<PathBuf, ()> {
    if let Some(rest) = uri.strip_prefix("file://") {
        let trimmed = rest.trim_start_matches('/');
        let decoded = percent_decode(trimmed);
        #[cfg(windows)]
        {
            let s = decoded.replace('/', "\\");
            return Ok(PathBuf::from(s));
        }
        #[cfg(not(windows))]
        {
            return Ok(PathBuf::from(format!("/{}", decoded)));
        }
    }
    Err(())
}

pub fn path_to_file_uri(p: &Path) -> String {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy().replace('\\', "/");
        format!("file:///{}", s)
    }
    #[cfg(not(windows))]
    {
        let s = p.to_string_lossy();
        format!("file://{}", s)
    }
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

pub fn try_find_solution_or_project(root: &Path) -> Option<String> {
    // Recursive scan for *.sln first, then *.csproj. Limit depth to avoid huge walks.
    fn scan_dir(dir: &Path, depth: usize, max_depth: usize, slns: &mut Vec<PathBuf>, projs: &mut Vec<PathBuf>) {
        if depth > max_depth { return; }
        let entries = match std::fs::read_dir(dir) { Ok(it) => it, Err(_) => return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_file() {
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("sln") { slns.push(p.clone()); }
                    else if ext.eq_ignore_ascii_case("csproj") { projs.push(p.clone()); }
                }
            } else if p.is_dir() {
                scan_dir(&p, depth + 1, max_depth, slns, projs);
            }
        }
    }

    let mut slns = Vec::new();
    let mut projs = Vec::new();
    scan_dir(root, 0, 4, &mut slns, &mut projs); // depth limit 4 for safety

    if slns.len() == 1 {
        return Some(path_to_file_uri(&slns[0]));
    } else if slns.len() > 1 {
        // choose deterministically: shortest path, then lexicographically
        slns.sort_by_key(|p| (p.components().count(), p.to_string_lossy().to_string()));
        return Some(path_to_file_uri(&slns[0]));
    }

    if projs.len() == 1 {
        return Some(path_to_file_uri(&projs[0]));
    } else if projs.len() > 1 {
        projs.sort_by_key(|p| (p.components().count(), p.to_string_lossy().to_string()));
        return Some(path_to_file_uri(&projs[0]));
    }

    None
}
