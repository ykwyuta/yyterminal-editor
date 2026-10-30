//! Bounded, advisory save names. Never changes document bytes or a supplied save path.
use std::path::{Path, PathBuf};

pub const SAMPLE_BYTES: u64 = 64 * 1024;

/// Keep the whole directory, so Save as starts beside the current file.
pub fn suggest(
    current: Option<&Path>,
    mode: Option<&str>,
    manual: Option<&str>,
    head: &[u8],
) -> PathBuf {
    let outline = mode == Some("outline");
    if !outline && let Some(path) = current {
        let special = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
            || matches!(
                crate::syntax::registry().detect(Some(path), b"").as_deref(),
                Some("makefile" | "dockerfile" | "gitignore")
            );
        if path.extension().is_some_and(|ext| !ext.is_empty()) || special {
            return path.to_owned();
        }
    }
    let ext = if outline {
        "json"
    } else {
        mode.or_else(|| manual.and_then(syntax_extension))
            .unwrap_or_else(|| infer(head))
    };
    let mut path = current.unwrap_or(Path::new("Untitled")).to_owned();
    path.set_extension(ext);
    path
}

fn syntax_extension(name: &str) -> Option<&'static str> {
    let id = crate::syntax::language(name)?.id.clone();
    Some(match id.as_str() {
        "rust" => "rs",
        "python" => "py",
        "javascript" => "js",
        "typescript" => "ts",
        "csharp" => "cs",
        "ruby" => "rb",
        "perl" => "pl",
        "vb" => "vb",
        "kotlin" => "kt",
        "shell" => "sh",
        "powershell" => "ps1",
        "batch" => "bat",
        "markdown" => "md",
        "cobol" | "cobol-free" => "cbl",
        "c" => "c",
        "cpp" => "cpp",
        "java" => "java",
        "go" => "go",
        "php" => "php",
        "swift" => "swift",
        "json" => "json",
        "html" => "html",
        "xml" => "xml",
        "css" => "css",
        "yaml" => "yaml",
        "toml" => "toml",
        "ini" => "ini",
        "sql" => "sql",
        "jcl" => "jcl",
        "rhai" => "rhai",
        "pli" => "pli",
        "rpg" => "rpg",
        "log" => "log",
        "diff" => "diff",
        _ => return None,
    })
}

fn infer(bytes: &[u8]) -> &'static str {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(SAMPLE_BYTES as usize)]);
    let text = text.trim_start_matches('\u{feff}').trim();
    let lower = text.to_ascii_lowercase();
    let root = lower
        .strip_prefix("<?xml")
        .and_then(|s| s.split_once("?>"))
        .map_or(lower.as_str(), |(_, s)| s.trim_start());
    if root.starts_with("<svg") {
        return "svg";
    }
    if let Some(ext) = crate::syntax::registry()
        .detect(None, text.as_bytes())
        .and_then(|id| syntax_extension(&id))
    {
        return ext;
    }
    if text.is_empty() {
        return "txt";
    }
    if text.starts_with(['{', '['])
        && match serde_json::from_str::<serde_json::Value>(text) {
            Ok(_) => true,
            Err(e) => e.is_eof(),
        }
    {
        return "json";
    }
    let lines: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(128)
        .collect();
    if [
        "<!doctype html",
        "<html",
        "<head>",
        "<body",
        "<div",
        "<p>",
        "<section",
    ]
    .iter()
    .any(|s| lower.starts_with(s))
    {
        return "html";
    }
    if text.starts_with("<?php") {
        return "php";
    }
    if text.starts_with("<?xml")
        || text.starts_with("<!--")
        || text.starts_with('<')
            && (text.contains("</")
                || text.strip_prefix('<').is_some_and(|s| {
                    s.starts_with(|c: char| c.is_alphabetic() || c == '_') && s.contains("/>")
                }))
    {
        return "xml";
    }
    if lines.len() > 1
        && lines
            .iter()
            .all(|s| s.starts_with('{') && serde_json::from_str::<serde_json::Value>(s).is_ok())
    {
        return "jsonl";
    }
    if lines
        .iter()
        .any(|s| s.starts_with("```") || s.starts_with("~~~"))
    {
        return "md";
    }
    for line in &lines {
        let starts = |prefixes: &[&str]| prefixes.iter().any(|p| line.starts_with(p));
        if starts(&["<?php"]) {
            return "php";
        }
        if line.eq_ignore_ascii_case("@echo off") {
            return "bat";
        }
        if starts(&["using System"]) || starts(&["namespace "]) && text.contains("public class ") {
            return "cs";
        }
        if starts(&["#include ", "#include<"]) {
            return if text.contains("std::") || text.contains("iostream") {
                "cpp"
            } else {
                "c"
            };
        }
        if starts(&["fn ", "pub fn ", "use std::", "use crate::"]) {
            return "rs";
        }
        if starts(&["package main", "func "]) {
            return "go";
        }
        if starts(&["def "]) && line.ends_with(':')
            || starts(&["from "]) && line.contains(" import ")
            || starts(&["if __name__ ==", "print("])
            || starts(&["import "]) && !line.ends_with(';') && !line.contains(['\'', '"', '{'])
        {
            return "py";
        }
        if starts(&["public class ", "import java."])
            || starts(&["package "]) && line.ends_with(';')
        {
            return "java";
        }
        if starts(&["interface ", "export interface "])
            || starts(&["type "]) && line.contains(" = ")
        {
            return "ts";
        }
        if starts(&["function ", "export default ", "const "])
            || starts(&["let "]) && line.contains('=')
        {
            return "js";
        }
        if starts(&["IDENTIFICATION DIVISION", "000100 IDENTIFICATION DIVISION"]) {
            return "cbl";
        }
        let sql = line.to_ascii_uppercase();
        if [
            "SELECT ",
            "INSERT INTO ",
            "CREATE TABLE ",
            "UPDATE ",
            "DELETE FROM ",
            "WITH ",
        ]
        .iter()
        .any(|p| sql.starts_with(p))
            && (lower.contains(" from ")
                || lower.contains(" into ")
                || lower.contains("table ")
                || text.ends_with(';'))
        {
            return "sql";
        }
    }
    if lines.iter().any(|s| {
        s.starts_with("# ")
            || s.starts_with("## ")
            || s.starts_with("### ")
            || s.starts_with("- [ ] ")
            || s.starts_with("- [x] ")
            || s.contains("](") && s.contains('[')
    }) || lines
        .get(1)
        .is_some_and(|s| s.len() >= 3 && s.chars().all(|c| c == '=' || c == '-'))
    {
        return "md";
    }
    for (delimiter, ext) in [(b'\t', "tsv"), (b',', "csv")] {
        if delimited(text, delimiter) {
            return ext;
        }
    }
    let values: Vec<_> = lines
        .iter()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            (!key.is_empty() && !key.contains([' ', ':', '/', '<', '>'])).then_some(value.trim())
        })
        .collect();
    if values.len() >= 2
        || !values.is_empty() && lines.iter().any(|s| s.starts_with('[') && s.ends_with(']'))
    {
        return if values.iter().any(|v| {
            v.starts_with(['"', '\'', '[', '{'])
                || v.parse::<f64>().is_ok()
                || matches!(*v, "true" | "false")
        }) {
            "toml"
        } else {
            "ini"
        };
    }
    if text.starts_with("---\n")
        || text.starts_with("---\r\n")
        || lines
            .iter()
            .filter(|s| {
                s.split_once(": ")
                    .is_some_and(|(k, _)| !k.is_empty() && !k.contains([' ', '/', '\\']))
            })
            .count()
            >= 2
    {
        return "yaml";
    }
    if lines
        .iter()
        .filter(|s| {
            s.starts_with("- ")
                || s.starts_with("* ")
                || s.starts_with("+ ")
                || s.starts_with("> ")
                || s.split_once(". ")
                    .is_some_and(|(n, _)| n.parse::<u32>().is_ok())
        })
        .count()
        >= 2
    {
        return "md";
    }
    if ['{', '}', ':', ';'].iter().all(|c| text.contains(*c)) {
        return "css";
    }
    "txt"
}

fn delimited(text: &str, delimiter: u8) -> bool {
    let dialect = yy_delimited::Dialect::new(&[delimiter], Some(b'"')).unwrap();
    let mut state = yy_delimited::LineState::default();
    let mut widths = Vec::new();
    for line in text.lines().take(128) {
        let (cells, next) = yy_delimited::split_line(line.as_bytes(), &dialect, state);
        if !next.in_quotes && (!line.is_empty() || state.in_quotes) {
            widths.push(cells.last().map_or(1, |cell| cell.field + 1));
        }
        state = next;
        if widths.len() == 32 {
            break;
        }
    }
    !state.in_quotes
        && widths.len() >= 2
        && widths[0] >= 2
        && widths.iter().all(|w| *w == widths[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_formats_match_yyeditor_save_defaults() {
        for (content, ext) in [
            ("", "txt"),
            ("普通のメモ😀\n本文", "txt"),
            ("42", "txt"),
            ("\u{feff}{\"日本語\":true}", "json"),
            ("[1,2,", "json"),
            ("{broken}", "txt"),
            ("{\"id\":1}\n{\"id\":2}", "jsonl"),
            ("# Title\n本文", "md"),
            ("```rust\nfn main() {}\n```", "md"),
            ("[link](https://example.com)", "md"),
            ("Title\n===", "md"),
            ("- one\n- two", "md"),
            ("<!DOCTYPE html><html></html>", "html"),
            ("<div>hello</div>", "html"),
            ("<?xml version=\"1.0\"?><root/>", "xml"),
            ("<root><item/></root>", "xml"),
            ("<root/>", "xml"),
            ("<?xml version=\"1.0\"?><svg/>", "svg"),
            ("<svg/>", "svg"),
            ("<?php echo 1; ?>", "php"),
            ("name,count\nAlice,2", "csv"),
            ("name\tcount\nAlice\t2", "tsv"),
            ("name,note\nAlice,\"a\nb\"", "csv"),
            ("one,two\nplain prose", "txt"),
            ("a,b\n1,2,3", "txt"),
            ("a,b\n1,\"unclosed", "txt"),
            ("[server]\nhost=localhost\nuser=alice", "ini"),
            ("[package]\nname = \"demo\"", "toml"),
            ("name=\"demo\"\nactive=true", "toml"),
            ("name: demo\nversion: 1", "yaml"),
            ("---\nitems:\n - a", "yaml"),
            ("#!/usr/bin/env python3\nprint(1)", "py"),
            ("#!/bin/bash\necho ok", "sh"),
            ("def hello():\n return 1", "py"),
            ("fn main() {}", "rs"),
            ("const answer = 1;", "js"),
            ("export interface Item {}", "ts"),
            ("#include <stdio.h>", "c"),
            ("#include <iostream>", "cpp"),
            ("using System;", "cs"),
            ("import java.util.List;", "java"),
            ("package main\nfunc main() {}", "go"),
            ("@echo off\necho hi", "bat"),
            ("SELECT * FROM users;", "sql"),
            ("body { color: red; }", "css"),
        ] {
            assert_eq!(infer(content.as_bytes()), ext, "{content:?}");
        }
        assert_eq!(
            infer(include_bytes!("../demo/outline-notes.yyoutline")),
            "json"
        );
    }

    #[test]
    fn names_and_priority_preserve_existing_paths() {
        let choose = |path: Option<&str>, mode, syntax| {
            suggest(path.map(Path::new), mode, syntax, b"fn main() {}")
        };
        assert_eq!(choose(None, None, None), Path::new("Untitled.rs"));
        assert_eq!(
            choose(None, Some("md"), Some("json")),
            Path::new("Untitled.md")
        );
        assert_eq!(
            choose(None, None, Some("typescript")),
            Path::new("Untitled.ts")
        );
        assert_eq!(choose(None, None, Some("none")), Path::new("Untitled.rs"));
        assert_eq!(
            choose(Some("dir/my file"), None, None),
            Path::new("dir/my file.rs")
        );
        for name in [
            "dir/FILE.HPP",
            "notes.txt",
            "Makefile",
            "Dockerfile",
            ".gitignore",
            ".env",
            ".bashrc",
        ] {
            assert_eq!(
                choose(Some(name), Some("md"), Some("python")),
                Path::new(name)
            );
        }
        assert_eq!(
            choose(Some("dir/notes.yyoutline"), Some("outline"), None),
            Path::new("dir/notes.json")
        );
        assert_eq!(
            choose(Some("notes.txt"), Some("outline"), None),
            Path::new("notes.json")
        );
        assert_eq!(
            choose(None, Some("outline"), None),
            Path::new("Untitled.json")
        );
    }

    #[test]
    fn sample_is_bounded_even_at_utf8_boundary() {
        let mut bytes = vec![b'x'; SAMPLE_BYTES as usize];
        bytes.extend_from_slice(b"\nfn main() {}");
        assert_eq!(infer(&bytes), "txt");
        bytes[SAMPLE_BYTES as usize - 1] = 0xe6;
        assert_eq!(infer(&bytes), "txt");
        assert_eq!(infer(b"\xff\xfe\x00invalid"), "txt");
    }
}
