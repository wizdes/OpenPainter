//! `opcli dump-catalog`: the Mac app's menus, tools, panels and keys as JSON.
//!
//! The Swift files stay the source. No Swift builds on Windows, so this reader
//! takes the Swift text as compiled into this program and reads the few forms
//! those files use. Any form it does not know is an error that names the file
//! and line, so a change in the Swift cannot slip past as a wrong answer.
//!
//! `MenuSkeleton.swift` also fills two menus from the engine's effect list. This
//! file repeats those few rules in Rust, so the tests in `tests/catalog.rs` hold
//! the result against two files the Mac's own tests keep true:
//! `menu-skeleton.txt` and `docs/shortcuts.md`.
use serde_json::{Value, json};
use std::collections::HashMap;

/// The Swift and table text the catalog comes from.
#[derive(Clone)]
pub struct Sources {
    pub menu: String,
    pub tools: String,
    pub panels: String,
    pub zoom: String,
    pub selection: String,
    pub keys: String,
    pub icons: String,
}

impl Sources {
    /// The files as they were when this program was built.
    pub fn embedded() -> Self {
        Self {
            menu: include_str!("../../../mac/OpenPainterKit/Sources/Shell/MenuSkeleton.swift")
                .into(),
            tools: include_str!("../../../mac/OpenPainterKit/Sources/Shell/ToolState.swift").into(),
            panels: include_str!("../../../mac/OpenPainterKit/Sources/Shell/PanelLayout.swift")
                .into(),
            zoom: include_str!("../../../mac/OpenPainterKit/Sources/ZoomPresets.swift").into(),
            selection: include_str!(
                "../../../mac/OpenPainterKit/Sources/Shell/SelectionEditActions.swift"
            )
            .into(),
            keys: include_str!("../../../mac/OpenPainterKit/Sources/Shell/KeyboardShortcuts.swift")
                .into(),
            icons: include_str!("../../../docs/windows/icons.toml").into(),
        }
    }
}

/// One entry of the engine's `effects.list`.
#[derive(Clone, Debug)]
pub struct Effect {
    pub id: String,
    pub label: String,
    pub category: String,
}

/// Reads `effects.list` from the engine. It needs no document and no GPU.
pub fn engine_effects() -> Result<Vec<Effect>, Box<dyn std::error::Error>> {
    let engine = op_engine::Engine::with_options(
        None,
        op_engine::EngineOptions {
            force_cpu: true,
            ..Default::default()
        },
    )?;
    let bytes = engine.query(0, "effects.list", json!({}))?;
    let list: Vec<Value> = serde_json::from_slice(&bytes)?;
    list.iter()
        .map(|effect| {
            let field = |name: &str| {
                effect[name]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("effects.list: an entry has no \"{name}\": {effect}"))
            };
            Ok(Effect {
                id: field("id")?,
                label: field("label")?,
                category: field("category")?,
            })
        })
        .collect()
}

/// The JSON the command prints, with one line ending at the end.
pub fn render(sources: &Sources, effects: &[Effect]) -> Result<String, String> {
    let value = build(sources, effects)?;
    let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    Ok(text + "\n")
}

/// The whole catalog.
pub fn build(sources: &Sources, effects: &[Effect]) -> Result<Value, String> {
    let icons = icon_table(&sources.icons);
    let menus = read_menus(sources, effects)?;
    let tools = read_tools(&sources.tools)?;
    let shortcuts = read_keys(&sources.keys, &tools)?;
    let panels = panel_names(&sources.panels)?;
    let tool_rows: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({
                "id": tool.id,
                "label": tool.label,
                "key": tool.key,
                "symbol": tool.symbol,
                "functional": tool.functional,
                "options": tool.options,
                "icon": icons.get(&format!("tool:{}", tool.id)),
            })
        })
        .collect();
    let panel_rows: Vec<Value> = panels
        .iter()
        .enumerate()
        .map(|(index, name)| {
            json!({
                "id": name,
                "title": capitalized(name),
                "shortcut": format!("F{}", 5 + index),
                "icon": icons.get(&format!("panel:{name}")),
            })
        })
        .collect();
    Ok(json!({
        "schema": 1,
        "note": "Written by `opcli dump-catalog` from the Mac app's Swift files. Do not edit by hand.",
        "menus": menus.iter().map(|menu| menu.to_json("", &icons)).collect::<Vec<_>>(),
        "tools": tool_rows,
        "panels": panel_rows,
        "shortcuts": shortcuts,
    }))
}

/// One line per visible item, in the notation of `MenuSkeleton.describe`, for
/// the tests below.
#[cfg(test)]
fn describe(catalog: &Value) -> Vec<String> {
    fn walk(items: &[Value], lines: &mut Vec<String>) {
        for item in items {
            let path = item["path"].as_str().unwrap_or_default();
            match item["kind"].as_str() {
                Some("menu") => {
                    lines.push(path.to_owned());
                    walk(item["items"].as_array().map_or(&[], Vec::as_slice), lines);
                }
                Some("separator") => lines.push(path.to_owned()),
                _ => {
                    let mut line = path.to_owned();
                    if let Some(key) = item["shortcut"].as_str() {
                        line += &format!(" [{key}]");
                    }
                    if item["off"].as_bool() == Some(true) {
                        line += " (off)";
                    }
                    lines.push(line);
                }
            }
        }
    }
    let mut lines = Vec::new();
    walk(
        catalog["menus"].as_array().map_or(&[], Vec::as_slice),
        &mut lines,
    );
    lines
}

// ---- The Swift reader ----

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Str(String),
    Word(String),
    Mark(char),
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    line: usize,
}

/// Splits Swift text into strings, words and single marks. Comments go away.
fn lex(file: &str, text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let (mut at, mut line) = (0, 1);
    while at < chars.len() {
        let c = chars[at];
        if c == '\n' {
            line += 1;
            at += 1;
        } else if c.is_whitespace() {
            at += 1;
        } else if c == '/' && chars.get(at + 1) == Some(&'/') {
            while at < chars.len() && chars[at] != '\n' {
                at += 1;
            }
        } else if c == '/' && chars.get(at + 1) == Some(&'*') {
            at += 2;
            while at + 1 < chars.len() && !(chars[at] == '*' && chars[at + 1] == '/') {
                if chars[at] == '\n' {
                    line += 1;
                }
                at += 1;
            }
            at += 2;
        } else if c == '"' {
            let start = line;
            let mut text = String::new();
            at += 1;
            loop {
                let Some(&next) = chars.get(at) else {
                    return Err(format!("{file}:{start}: a string has no end"));
                };
                at += 1;
                match next {
                    '"' => break,
                    '\n' => return Err(format!("{file}:{start}: a string has no end")),
                    '\\' => {
                        let escaped = chars.get(at).copied();
                        at += 1;
                        match escaped {
                            Some('"') => text.push('"'),
                            Some('\\') => text.push('\\'),
                            Some('n') => text.push('\n'),
                            // A value in \( ) stays as written, for text this reader never uses.
                            Some('(') => text.push_str("\\("),
                            other => {
                                return Err(format!(
                                    "{file}:{start}: unknown string escape {other:?}"
                                ));
                            }
                        }
                    }
                    other => text.push(other),
                }
            }
            tokens.push(Token {
                tok: Tok::Str(text),
                line: start,
            });
        } else if c.is_alphanumeric() || c == '_' || c == '$' {
            let start = at;
            while at < chars.len()
                && (chars[at].is_alphanumeric()
                    || chars[at] == '_'
                    || chars[at] == '$'
                    || (chars[at] == '.'
                        && chars[start].is_ascii_digit()
                        && chars.get(at + 1).is_some_and(char::is_ascii_digit)))
            {
                at += 1;
            }
            tokens.push(Token {
                tok: Tok::Word(chars[start..at].iter().collect()),
                line,
            });
        } else {
            tokens.push(Token {
                tok: Tok::Mark(c),
                line,
            });
            at += 1;
        }
    }
    Ok(tokens)
}

/// A place in a file's tokens, with the checks a small reader needs.
struct Reader<'a> {
    file: &'a str,
    tokens: &'a [Token],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(file: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            file,
            tokens,
            at: 0,
        }
    }

    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.at).map(|token| &token.tok)
    }

    /// An error that says where the reader stopped and what it found there.
    fn fail(&self, want: &str) -> String {
        let line = self
            .tokens
            .get(self.at)
            .or(self.tokens.last())
            .map_or(0, |token| token.line);
        let found: Vec<String> = self.tokens[self.at.min(self.tokens.len())..]
            .iter()
            .take(6)
            .map(|token| match &token.tok {
                Tok::Str(text) => format!("\"{text}\""),
                Tok::Word(word) => word.clone(),
                Tok::Mark(mark) => mark.to_string(),
            })
            .collect();
        format!(
            "{}:{line}: expected {want}, found `{}`. dump-catalog does not know this form; \
             teach engine/op-cli/src/catalog.rs",
            self.file,
            found.join(" ")
        )
    }

    fn is_mark(&self, mark: char) -> bool {
        self.peek() == Some(&Tok::Mark(mark))
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(w)) if w == word)
    }

    fn take_mark(&mut self, mark: char) -> bool {
        let found = self.is_mark(mark);
        if found {
            self.at += 1;
        }
        found
    }

    fn mark(&mut self, mark: char) -> Result<(), String> {
        if self.take_mark(mark) {
            Ok(())
        } else {
            Err(self.fail(&format!("`{mark}`")))
        }
    }

    fn word(&mut self) -> Result<String, String> {
        match self.peek() {
            Some(Tok::Word(word)) => {
                let word = word.clone();
                self.at += 1;
                Ok(word)
            }
            _ => Err(self.fail("a word")),
        }
    }

    fn this_word(&mut self, want: &str) -> Result<(), String> {
        if self.is_word(want) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.fail(&format!("`{want}`")))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        match self.peek() {
            Some(Tok::Str(text)) => {
                let text = text.clone();
                self.at += 1;
                Ok(text)
            }
            _ => Err(self.fail("a string")),
        }
    }

    /// Moves to just after the first place these words and marks appear in a row.
    fn find(&mut self, pattern: &[&str]) -> Result<(), String> {
        let hit = (self.at..self.tokens.len()).find(|&start| {
            pattern.iter().enumerate().all(|(offset, want)| {
                matches!(self.tokens.get(start + offset), Some(token) if text_of(&token.tok) == *want)
            })
        });
        match hit {
            Some(start) => {
                self.at = start + pattern.len();
                Ok(())
            }
            None => Err(format!(
                "{}: `{}` is gone. dump-catalog reads it; teach engine/op-cli/src/catalog.rs",
                self.file,
                pattern.join(" ")
            )),
        }
    }

    /// The tokens from an opening mark to its match, as one space-separated
    /// string, for comparing a closure with the one this file knows.
    fn balanced_text(&mut self, open: char, close: char) -> Result<String, String> {
        self.mark(open)?;
        let mut depth = 1;
        let mut words = Vec::new();
        loop {
            let Some(token) = self.tokens.get(self.at) else {
                return Err(self.fail(&format!("`{close}`")));
            };
            if token.tok == Tok::Mark(open) {
                depth += 1;
            } else if token.tok == Tok::Mark(close) {
                depth -= 1;
                if depth == 0 {
                    self.at += 1;
                    return Ok(words.join(" "));
                }
            }
            words.push(match &token.tok {
                Tok::Str(text) => format!("\"{text}\""),
                other => text_of(other),
            });
            self.at += 1;
        }
    }
}

fn text_of(tok: &Tok) -> String {
    match tok {
        Tok::Str(text) | Tok::Word(text) => text.clone(),
        Tok::Mark(mark) => mark.to_string(),
    }
}

// ---- Menus ----

#[derive(Clone, Debug)]
enum Node {
    Item {
        title: String,
        shortcut: String,
        action: String,
    },
    Separator,
    Menu {
        title: String,
        items: Vec<Node>,
    },
}

impl Node {
    fn item(title: &str, shortcut: String, action: String) -> Self {
        Self::Item {
            title: title.to_owned(),
            shortcut,
            action,
        }
    }

    /// An item the Mac app shows but has not built: `Act.none`.
    fn off(title: &str, shortcut: String) -> Self {
        Self::item(title, shortcut, "none".to_owned())
    }

    fn to_json(&self, parent: &str, icons: &HashMap<String, String>) -> Value {
        let join = |title: &str| {
            if parent.is_empty() {
                title.to_owned()
            } else {
                format!("{parent} > {title}")
            }
        };
        match self {
            Self::Separator => json!({"kind": "separator", "path": join("---")}),
            Self::Menu { title, items } => {
                let path = join(title);
                json!({
                    "kind": "menu",
                    "title": title,
                    "items": items.iter().map(|item| item.to_json(&path, icons)).collect::<Vec<_>>(),
                    "path": path,
                })
            }
            Self::Item {
                title,
                shortcut,
                action,
            } => {
                let path = join(title);
                let mut row = json!({
                    "kind": "item",
                    "title": title,
                    "action": action,
                    "off": action == "none",
                });
                if !shortcut.is_empty() {
                    row["shortcut"] = json!(shortcut);
                }
                if let Some(icon) = icons.get(&format!("menu:{path}")) {
                    row["icon"] = json!(icon);
                }
                row["path"] = json!(path);
                row
            }
        }
    }
}

/// `describe` writes modifiers in this order, with these marks.
fn modifier_text(names: &[String], reader: &Reader) -> Result<String, String> {
    let mut out = String::new();
    for (name, mark) in [
        ("control", '⌃'),
        ("option", '⌥'),
        ("shift", '⇧'),
        ("command", '⌘'),
    ] {
        if names.iter().any(|n| n == name) {
            out.push(mark);
        }
    }
    for name in names {
        if !["control", "option", "shift", "command"].contains(&name.as_str()) {
            return Err(reader.fail(&format!("a known modifier, not `{name}`")));
        }
    }
    Ok(out)
}

/// The word Swift uses for a modifier set in `MenuSkeleton`, as modifier names.
fn named_modifiers(word: &str) -> Option<&'static [&'static str]> {
    Some(match word {
        "cmd" => &["command"],
        "shiftCmd" => &["command", "shift"],
        "optCmd" => &["command", "option"],
        "opt" => &["option"],
        "optShiftCmd" => &["command", "option", "shift"],
        _ => return None,
    })
}

fn read_modifiers(reader: &mut Reader) -> Result<String, String> {
    if reader.take_mark('[') {
        let mut names = Vec::new();
        while !reader.take_mark(']') {
            reader.mark('.')?;
            names.push(reader.word()?);
            if !reader.is_mark(']') {
                reader.mark(',')?;
            }
        }
        return modifier_text(&names, reader);
    }
    let word = reader.word()?;
    let Some(names) = named_modifiers(&word) else {
        reader.at -= 1;
        return Err(reader.fail("a modifier set"));
    };
    let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    modifier_text(&names, reader)
}

/// A function key's name, from the `NS…FunctionKey` word.
fn function_key(word: &str) -> Option<String> {
    let name = word.strip_prefix("NS")?.strip_suffix("FunctionKey")?;
    Some(name.to_owned())
}

/// The key of an item as `describe` shows it: upper case, or a key's name.
fn read_key(reader: &mut Reader) -> Result<String, String> {
    if let Some(Tok::Str(_)) = reader.peek() {
        return Ok(reader.string()?.to_uppercase());
    }
    if reader.is_word("deleteKey") {
        reader.at += 1;
        return Ok("⌫".to_owned());
    }
    if reader.is_word("key") {
        reader.at += 1;
        reader.mark('(')?;
        let word = reader.word()?;
        reader.mark(')')?;
        return function_key(&word).ok_or_else(|| reader.fail("an NS…FunctionKey word"));
    }
    Err(reader.fail("a key"))
}

/// An `Act` as text: `selector:save:`, `command:layer.delete`, `reset_panels`, `none`.
fn read_action(reader: &mut Reader) -> Result<String, String> {
    reader.mark('.')?;
    let kind = reader.word()?;
    match kind.as_str() {
        "none" => Ok("none".to_owned()),
        "resetPanels" => Ok("reset_panels".to_owned()),
        "command" | "effect" => {
            reader.mark('(')?;
            let id = reader.string()?;
            reader.mark(')')?;
            Ok(format!("{kind}:{id}"))
        }
        "selector" => {
            reader.mark('(')?;
            reader.mark('#')?;
            reader.this_word("selector")?;
            reader.mark('(')?;
            let mut last = reader.word()?;
            while reader.take_mark('.') {
                last = reader.word()?;
            }
            let args = reader.balanced_text('(', ')')?;
            if args != "_ :" {
                return Err(reader.fail("a selector that takes one `_:`"));
            }
            reader.mark(')')?;
            reader.mark(')')?;
            Ok(format!("selector:{last}:"))
        }
        _ => {
            reader.at -= 1;
            Err(reader.fail("a known menu action"))
        }
    }
}

/// `key + modifiers` in `describe`'s order, or nothing when there is no key.
fn shortcut_text(key: &str, modifiers: &str) -> String {
    if key.is_empty() {
        String::new()
    } else {
        format!("{modifiers}{key}")
    }
}

/// Everything the menu bar reads besides `entries` itself.
struct MenuContext<'a> {
    file: &'a str,
    effects: &'a [Effect],
    adjustments: Vec<(String, String, String)>,
    effect_menus: Vec<(String, Vec<String>)>,
    repeat: Vec<Node>,
    zoom_labels: Vec<String>,
    paste_titles: Vec<String>,
    panels: Vec<String>,
}

const PASTE_CLOSURE: &str = "n , title in . item ( title , n = = 0 ? \"v\" : \"\" , n = = 0 ? optShiftCmd : [ ] , . selector ( # selector ( SelectionEditActions . pasteSelection ( _ : ) ) ) )";
const PANEL_CLOSURE: &str =
    ". item ( $0 . title , key ( NSF5FunctionKey - 5 + $0 . functionKey ) , [ ] , . panel ( $0 ) )";

impl MenuContext<'_> {
    fn off_or_effect(&self, title: &str, category: &str) -> Node {
        self.effects
            .iter()
            .find(|effect| effect.category == category && effect.label == title)
            .map_or_else(
                || Node::off(title, String::new()),
                |effect| Node::item(title, String::new(), format!("effect:{}", effect.id)),
            )
    }

    /// `MenuSkeleton.adjustments`.
    fn adjustments_menu(&self) -> Vec<Node> {
        let registered: Vec<&Effect> = self
            .effects
            .iter()
            .filter(|effect| effect.category == "Adjustments")
            .collect();
        let mut out: Vec<Node> = registered
            .iter()
            .map(|effect| {
                let key = self
                    .adjustments
                    .iter()
                    .find(|(title, _, _)| *title == effect.label)
                    .map_or_else(String::new, |(_, key, mods)| shortcut_text(key, mods));
                Node::item(&effect.label, key, format!("effect:{}", effect.id))
            })
            .collect();
        let missing: Vec<_> = self
            .adjustments
            .iter()
            .filter(|(title, _, _)| !registered.iter().any(|effect| effect.label == *title))
            .collect();
        if !missing.is_empty() {
            out.push(Node::Separator);
            out.extend(
                missing
                    .iter()
                    .map(|(title, key, mods)| Node::off(title, shortcut_text(key, mods))),
            );
        }
        out
    }

    /// `MenuSkeleton.effectsMenu`, after its first two entries.
    fn effects_menu(&self) -> Vec<Node> {
        fn submenu(effect: &Effect) -> &str {
            effect
                .category
                .strip_prefix("Effects/")
                .unwrap_or(&effect.category)
        }
        let registered: Vec<&Effect> = self
            .effects
            .iter()
            .filter(|effect| !["Adjustments", "Edit", "Layers"].contains(&effect.category.as_str()))
            .collect();
        let mut placed: Vec<&str> = Vec::new();
        let mut slots: HashMap<String, Vec<Node>> = HashMap::new();
        for (name, titles) in &self.effect_menus {
            let mut slot = Vec::new();
            for title in titles {
                let found = registered
                    .iter()
                    .find(|effect| effect.label == *title && !placed.contains(&effect.id.as_str()));
                slot.push(match found {
                    Some(effect) => {
                        placed.push(&effect.id);
                        Node::item(title, String::new(), format!("effect:{}", effect.id))
                    }
                    None => Node::off(title, String::new()),
                });
            }
            slots.insert(name.clone(), slot);
        }
        let mut names: Vec<String> = self
            .effect_menus
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        for effect in &registered {
            if !placed.contains(&effect.id.as_str()) && !names.iter().any(|n| n == submenu(effect))
            {
                names.push(submenu(effect).to_owned());
            }
        }
        let mut out = self.repeat.clone();
        for name in names {
            let mut entries = slots.remove(&name).unwrap_or_default();
            let more: Vec<&&Effect> = registered
                .iter()
                .filter(|effect| !placed.contains(&effect.id.as_str()) && submenu(effect) == name)
                .collect();
            if !entries.is_empty() && !more.is_empty() {
                entries.push(Node::Separator);
            }
            entries.extend(more.iter().map(|effect| {
                Node::item(
                    &effect.label,
                    String::new(),
                    format!("effect:{}", effect.id),
                )
            }));
            out.push(Node::Menu {
                title: name,
                items: entries,
            });
        }
        out
    }

    /// `editEffects` in `MenuSkeleton.entries`.
    fn edit_effects(&self) -> Vec<Node> {
        self.effects
            .iter()
            .filter(|effect| effect.category == "Edit")
            .map(|effect| {
                Node::item(
                    &effect.label,
                    String::new(),
                    format!("effect:{}", effect.id),
                )
            })
            .collect()
    }

    /// One list expression: lists joined with `+`.
    fn list(&self, reader: &mut Reader) -> Result<Vec<Node>, String> {
        let mut out = self.list_term(reader)?;
        while reader.take_mark('+') {
            out.extend(self.list_term(reader)?);
        }
        Ok(out)
    }

    fn list_term(&self, reader: &mut Reader) -> Result<Vec<Node>, String> {
        if reader.take_mark('[') {
            let mut out = Vec::new();
            while !reader.take_mark(']') {
                out.push(self.entry(reader)?);
                if !reader.is_mark(']') {
                    reader.mark(',')?;
                }
            }
            return Ok(out);
        }
        if reader.is_mark('(') {
            // `(editEffects.isEmpty ? [] : [.separator] + editEffects)`.
            let text = reader.balanced_text('(', ')')?;
            if text != "editEffects . isEmpty ? [ ] : [ . separator ] + editEffects" {
                reader.at -= 1;
                return Err(reader.fail("the Edit menu's effect list"));
            }
            let edit = self.edit_effects();
            return Ok(if edit.is_empty() {
                edit
            } else {
                let mut out = vec![Node::Separator];
                out.extend(edit);
                out
            });
        }
        let word = reader.word()?;
        match word.as_str() {
            "zoomPresetEntries" => Ok(self
                .zoom_labels
                .iter()
                .map(|label| Node::item(label, String::new(), "selector:zoomToPreset:".to_owned()))
                .collect()),
            "adjustments" | "effectsMenu" => {
                reader.mark('(')?;
                reader.this_word("effects")?;
                reader.mark(')')?;
                Ok(if word == "adjustments" {
                    self.adjustments_menu()
                } else {
                    self.effects_menu()
                })
            }
            "SelectionClipboard" => {
                reader.mark('.')?;
                reader.this_word("pasteTitles")?;
                reader.mark('.')?;
                reader.this_word("enumerated")?;
                reader.mark('(')?;
                reader.mark(')')?;
                reader.mark('.')?;
                reader.this_word("map")?;
                let text = reader.balanced_text('{', '}')?;
                if text != PASTE_CLOSURE {
                    return Err(format!(
                        "{}: the Paste Selection list changed shape: `{text}`; teach engine/op-cli/src/catalog.rs",
                        self.file
                    ));
                }
                Ok(self
                    .paste_titles
                    .iter()
                    .enumerate()
                    .map(|(index, title)| {
                        Node::item(
                            title,
                            if index == 0 {
                                "⌥⇧⌘V".to_owned()
                            } else {
                                String::new()
                            },
                            "selector:pasteSelection:".to_owned(),
                        )
                    })
                    .collect())
            }
            "PanelKind" => {
                reader.mark('.')?;
                reader.this_word("allCases")?;
                reader.mark('.')?;
                reader.this_word("map")?;
                let text = reader.balanced_text('{', '}')?;
                if text != PANEL_CLOSURE {
                    return Err(format!(
                        "{}: the Window menu's panel list changed shape: `{text}`; teach engine/op-cli/src/catalog.rs",
                        self.file
                    ));
                }
                Ok(self
                    .panels
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        Node::item(
                            &capitalized(name),
                            format!("F{}", 5 + index),
                            format!("panel:{name}"),
                        )
                    })
                    .collect())
            }
            _ => {
                reader.at -= 1;
                Err(reader.fail("a list of menu entries"))
            }
        }
    }

    fn entry(&self, reader: &mut Reader) -> Result<Node, String> {
        if reader.take_mark('.') {
            let form = reader.word()?;
            return match form.as_str() {
                "separator" => Ok(Node::Separator),
                "submenu" => {
                    reader.mark('(')?;
                    let title = reader.string()?;
                    reader.mark(',')?;
                    let items = self.list(reader)?;
                    reader.mark(')')?;
                    Ok(Node::Menu { title, items })
                }
                "item" => {
                    reader.mark('(')?;
                    let title = reader.string()?;
                    reader.mark(',')?;
                    let key = read_key(reader)?;
                    reader.mark(',')?;
                    let modifiers = read_modifiers(reader)?;
                    reader.mark(',')?;
                    let action = read_action(reader)?;
                    reader.mark(')')?;
                    Ok(Node::item(&title, shortcut_text(&key, &modifiers), action))
                }
                _ => {
                    reader.at -= 1;
                    Err(reader.fail("a menu entry"))
                }
            };
        }
        let word = reader.word()?;
        match word.as_str() {
            // `i(title, key = "", modifiers = cmd, action = .none)`.
            "i" => {
                reader.mark('(')?;
                let title = reader.string()?;
                let mut key = String::new();
                let mut modifiers = "⌘".to_owned();
                let mut action = "none".to_owned();
                if reader.take_mark(',') {
                    key = read_key(reader)?;
                    if reader.take_mark(',') {
                        modifiers = read_modifiers(reader)?;
                        if reader.take_mark(',') {
                            action = read_action(reader)?;
                        }
                    }
                }
                reader.mark(')')?;
                Ok(Node::item(&title, shortcut_text(&key, &modifiers), action))
            }
            "layerItem" => {
                reader.mark('(')?;
                let title = reader.string()?;
                reader.mark(')')?;
                Ok(self.off_or_effect(&title, "Layers"))
            }
            _ => {
                reader.at -= 1;
                Err(reader.fail("a menu entry"))
            }
        }
    }
}

fn read_menus(sources: &Sources, effects: &[Effect]) -> Result<Vec<Node>, String> {
    const FILE: &str = "MenuSkeleton.swift";
    let tokens = lex(FILE, &sources.menu)?;

    let mut reader = Reader::new(FILE, &tokens);
    reader.find(&["static", "let", "paintNetAdjustments"])?;
    reader.find(&["="])?;
    reader.mark('[')?;
    let mut adjustments = Vec::new();
    while !reader.take_mark(']') {
        reader.mark('(')?;
        let title = reader.string()?;
        reader.mark(',')?;
        let key = reader.string()?.to_uppercase();
        reader.mark(',')?;
        let modifiers = read_modifiers(&mut reader)?;
        reader.mark(')')?;
        adjustments.push((title, key, modifiers));
        if !reader.is_mark(']') {
            reader.mark(',')?;
        }
    }

    let mut reader = Reader::new(FILE, &tokens);
    reader.find(&["static", "let", "paintNetEffects"])?;
    reader.find(&["="])?;
    reader.mark('[')?;
    let mut effect_menus = Vec::new();
    while !reader.take_mark(']') {
        reader.mark('(')?;
        let name = reader.string()?;
        reader.mark(',')?;
        effect_menus.push((name, string_list(&mut reader)?));
        reader.mark(')')?;
        if !reader.is_mark(']') {
            reader.mark(',')?;
        }
    }

    let mut context = MenuContext {
        file: FILE,
        effects,
        adjustments,
        effect_menus,
        repeat: Vec::new(),
        zoom_labels: zoom_labels(&sources.zoom)?,
        paste_titles: paste_titles(&sources.selection)?,
        panels: panel_names(&sources.panels)?,
    };

    let mut reader = Reader::new(FILE, &tokens);
    reader.find(&["static", "func", "effectsMenu"])?;
    reader.find(&["var", "out"])?;
    reader.find(&["="])?;
    context.repeat = context.list_term(&mut reader)?;

    let mut reader = Reader::new(FILE, &tokens);
    reader.find(&["static", "func", "entries"])?;
    reader.find(&["return", "["])?;
    reader.at -= 1;
    let top = context.list(&mut reader)?;
    for node in &top {
        if !matches!(node, Node::Menu { .. }) {
            return Err(format!(
                "{FILE}: the menu bar holds an entry that is not a menu"
            ));
        }
    }
    Ok(top)
}

fn string_list(reader: &mut Reader) -> Result<Vec<String>, String> {
    reader.mark('[')?;
    let mut out = Vec::new();
    while !reader.take_mark(']') {
        out.push(reader.string()?);
        if !reader.is_mark(']') {
            reader.mark(',')?;
        }
    }
    Ok(out)
}

fn paste_titles(text: &str) -> Result<Vec<String>, String> {
    const FILE: &str = "SelectionEditActions.swift";
    let tokens = lex(FILE, text)?;
    let mut reader = Reader::new(FILE, &tokens);
    reader.find(&["pasteTitles", "="])?;
    string_list(&mut reader)
}

/// `ZoomPresets.values`, largest first, with `ZoomPresets.label`'s text.
fn zoom_labels(text: &str) -> Result<Vec<String>, String> {
    const FILE: &str = "ZoomPresets.swift";
    let tokens = lex(FILE, text)?;
    let mut reader = Reader::new(FILE, &tokens);
    reader.find(&["static", "let", "values"])?;
    reader.find(&["="])?;
    reader.mark('[')?;
    let mut labels = Vec::new();
    while !reader.take_mark(']') {
        let number = reader.word()?;
        let value: f64 = number
            .parse()
            .map_err(|_| reader.fail("a zoom preset number"))?;
        let percent = value * 100.0;
        labels.push(if percent < 10.0 && percent.round() != percent {
            format!("{percent:.1}%")
        } else {
            format!("{}%", percent.round() as i64)
        });
        if !reader.is_mark(']') {
            reader.mark(',')?;
        }
    }
    labels.reverse();
    Ok(labels)
}

fn panel_names(text: &str) -> Result<Vec<String>, String> {
    const FILE: &str = "PanelLayout.swift";
    let tokens = lex(FILE, text)?;
    let mut reader = Reader::new(FILE, &tokens);
    reader.find(&["enum", "PanelKind"])?;
    reader.find(&["{", "case"])?;
    let mut names = vec![reader.word()?];
    while reader.take_mark(',') {
        names.push(reader.word()?);
    }
    Ok(names)
}

/// Swift's `capitalized` for the plain lower-case words this file passes.
fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

// ---- Tools ----

#[derive(Debug, Clone)]
struct Tool {
    id: String,
    label: String,
    key: String,
    symbol: String,
    functional: bool,
    options: String,
}

/// One `switch self` property: for each case word, the value's text.
fn tool_switch(
    tokens: &[Token],
    property: &str,
) -> Result<(HashMap<String, Tok>, Option<Tok>), String> {
    const FILE: &str = "ToolState.swift";
    let mut reader = Reader::new(FILE, tokens);
    reader.find(&["public", "var", property])?;
    reader.find(&["switch", "self", "{"])?;
    let mut values = HashMap::new();
    let mut fallback = None;
    while !reader.take_mark('}') {
        if reader.is_word("default") {
            reader.at += 1;
            reader.mark(':')?;
            fallback = Some(read_value(&mut reader)?);
            continue;
        }
        reader.this_word("case")?;
        let mut names = Vec::new();
        loop {
            reader.mark('.')?;
            names.push(reader.word()?);
            if !reader.take_mark(',') {
                break;
            }
        }
        reader.mark(':')?;
        let value = read_value(&mut reader)?;
        for name in names {
            values.insert(name, value.clone());
        }
    }
    Ok((values, fallback))
}

/// A switch value: a string, `true`, `false`, or `.name`.
fn read_value(reader: &mut Reader) -> Result<Tok, String> {
    if reader.take_mark('.') {
        return Ok(Tok::Word(reader.word()?));
    }
    match reader.peek() {
        Some(Tok::Str(_)) => Ok(Tok::Str(reader.string()?)),
        Some(Tok::Word(word)) if word == "true" || word == "false" => Ok(Tok::Word(reader.word()?)),
        _ => Err(reader.fail("a string, true, false or .name")),
    }
}

fn read_tools(text: &str) -> Result<Vec<Tool>, String> {
    const FILE: &str = "ToolState.swift";
    let tokens = lex(FILE, text)?;
    let mut reader = Reader::new(FILE, &tokens);
    reader.find(&["enum", "PaintTool"])?;
    reader.find(&["{"])?;
    let mut ids = Vec::new();
    while reader.is_word("case") {
        reader.at += 1;
        ids.push(reader.word()?);
        while reader.take_mark(',') {
            ids.push(reader.word()?);
        }
    }
    let (labels, _) = tool_switch(&tokens, "label")?;
    let (keys, _) = tool_switch(&tokens, "key")?;
    let (symbols, _) = tool_switch(&tokens, "symbol")?;
    let (functional, functional_rest) = tool_switch(&tokens, "isFunctional")?;
    let (options, options_rest) = tool_switch(&tokens, "options")?;
    let text_for = |table: &HashMap<String, Tok>, property: &str, id: &str| match table.get(id) {
        Some(tok) => Ok(text_of(tok)),
        None => Err(format!("{FILE}: PaintTool.{property} has no case for {id}")),
    };
    ids.iter()
        .map(|id| {
            let on = functional.get(id).or(functional_rest.as_ref());
            let Some(on) = on else {
                return Err(format!(
                    "{FILE}: PaintTool.isFunctional has no case for {id}"
                ));
            };
            let option = options.get(id).or(options_rest.as_ref());
            let Some(option) = option else {
                return Err(format!("{FILE}: PaintTool.options has no case for {id}"));
            };
            Ok(Tool {
                id: id.clone(),
                label: text_for(&labels, "label", id)?,
                key: text_for(&keys, "key", id)?,
                symbol: text_for(&symbols, "symbol", id)?,
                functional: text_of(on) == "true",
                options: text_of(option),
            })
        })
        .collect()
}

// ---- Keys that are not menu items ----

fn key_row(action: &str, win: &str, mac: &str, scope: &str, note: &str) -> Value {
    json!({"action": action, "win": win, "mac": mac, "scope": scope, "note": note})
}

/// `KeyboardShortcuts.canvas`: the tool letters, then the listed keys.
fn read_keys(text: &str, tools: &[Tool]) -> Result<Vec<Value>, String> {
    const FILE: &str = "KeyboardShortcuts.swift";
    let mut rows = Vec::new();
    let mut letters: Vec<&str> = Vec::new();
    for tool in tools {
        if !letters.contains(&tool.key.as_str()) {
            letters.push(&tool.key);
        }
    }
    for letter in letters {
        let group: Vec<&Tool> = tools.iter().filter(|tool| tool.key == letter).collect();
        let upper = letter.to_uppercase();
        for (index, tool) in group.iter().enumerate() {
            let left = group.len() - index;
            let presses = if index == 0 {
                upper.clone()
            } else {
                format!("{upper} ×{}", index + 1)
            };
            let (win, mac) = if group.len() > 1 {
                let times = if left > 1 {
                    format!(" x{left}")
                } else {
                    String::new()
                };
                let mac_times = if left > 1 {
                    format!(" ×{left}")
                } else {
                    String::new()
                };
                (
                    format!("{presses} or Shift+{upper}{times}"),
                    format!("{presses} or ⇧{upper}{mac_times}"),
                )
            } else {
                (upper.clone(), presses.clone())
            };
            let note = if !tool.functional {
                "The tool is in the panel but not built yet"
            } else if group.len() > 1 {
                "Shared letter: each press picks the next tool"
            } else {
                ""
            };
            rows.push(key_row(&tool.label, &win, &mac, "Canvas", note));
        }
    }

    let tokens = lex(FILE, text)?;
    let mut reader = Reader::new(FILE, &tokens);
    reader.find(&["public", "static", "var", "canvas"])?;
    reader.find(&["out", "+", "=", "["])?;
    while !reader.take_mark(']') {
        reader.this_word("KeyShortcut")?;
        reader.mark('(')?;
        let action = reader.string()?;
        let mut fields: HashMap<String, String> = HashMap::new();
        while reader.take_mark(',') {
            let name = reader.word()?;
            reader.mark(':')?;
            fields.insert(name, reader.string()?);
        }
        reader.mark(')')?;
        let field = |name: &str| {
            fields
                .get(name)
                .cloned()
                .ok_or_else(|| format!("{FILE}: the key \"{action}\" has no `{name}:`"))
        };
        rows.push(key_row(
            &action,
            &field("win")?,
            &field("mac")?,
            &field("scope")?,
            fields.get("note").map_or("", String::as_str),
        ));
        if !reader.is_mark(']') {
            reader.mark(',')?;
        }
    }
    Ok(rows)
}

// ---- Icons ----

/// `use` to icon name, from the rows of `docs/windows/icons.toml`.
fn icon_table(text: &str) -> HashMap<String, String> {
    let mut table = HashMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("use = \"") {
            current = rest
                .strip_suffix('"')
                .map(|name| name.replace("\\\"", "\""));
        } else if let Some(rest) = line.strip_prefix("file = \"") {
            let name = rest
                .strip_suffix('"')
                .and_then(|file| file.rsplit('/').next())
                .and_then(|file| file.strip_suffix(".svg"));
            if let (Some(key), Some(name)) = (current.take(), name) {
                table.insert(key, name.to_owned());
            }
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effects() -> Vec<Effect> {
        engine_effects().expect("the engine lists its effects")
    }

    fn rebuilt(change: impl Fn(&mut Sources)) -> Result<Value, String> {
        let mut sources = Sources::embedded();
        change(&mut sources);
        build(&sources, &effects())
    }

    fn lines_after(change: impl Fn(&mut Sources)) -> Vec<String> {
        describe(&rebuilt(change).expect("the changed Swift still reads"))
    }

    #[test]
    fn swapping_two_items_in_the_swift_moves_them_in_the_catalog() {
        let before = lines_after(|_| ());
        let after = lines_after(|s| {
            s.menu = s
                .menu
                .replace("\"Zoom In\"", "\"Swap\"")
                .replace("\"Zoom Out\"", "\"Zoom In\"")
                .replace("\"Swap\"", "\"Zoom Out\"");
        });
        assert_ne!(before, after);
        let at = |lines: &[String], text: &str| lines.iter().position(|l| l.starts_with(text));
        assert!(at(&after, "View > Zoom Out") < at(&after, "View > Zoom In"));
    }

    #[test]
    fn a_new_shortcut_in_the_swift_reaches_the_catalog() {
        let after = lines_after(|s| {
            s.menu = s.menu.replace(
                ".item(\"Rulers\", \"r\", optCmd,",
                ".item(\"Rulers\", \"u\", shiftCmd,",
            );
        });
        assert!(
            after.contains(&"View > Rulers [⇧⌘U]".to_owned()),
            "{after:?}"
        );
    }

    #[test]
    fn a_menu_form_the_reader_does_not_know_fails_by_file_and_line() {
        let error = rebuilt(|s| {
            s.menu = s.menu.replace(
                ".item(\"Rulers\", \"r\", optCmd,",
                ".fancy(\"Rulers\", \"r\", optCmd,",
            );
        })
        .unwrap_err();
        assert!(error.contains("MenuSkeleton.swift:"), "{error}");
        assert!(error.contains("fancy"), "{error}");
    }

    #[test]
    fn an_unknown_modifier_fails_by_name() {
        let error = rebuilt(|s| {
            s.menu = s.menu.replace("\"`\", [.control], ", "\"`\", [.hyper], ");
        })
        .unwrap_err();
        assert!(error.contains("hyper"), "{error}");
    }

    #[test]
    fn a_changed_panel_list_fails_by_name() {
        let error = rebuilt(|s| {
            s.menu = s.menu.replace(
                "key(NSF5FunctionKey - 5 + $0.functionKey)",
                "key(NSF9FunctionKey - 5 + $0.functionKey)",
            );
        })
        .unwrap_err();
        assert!(error.contains("panel list"), "{error}");
    }

    #[test]
    fn a_tool_with_no_label_fails_by_name() {
        let error = rebuilt(|s| {
            s.tools = s.tools.replace("case .pencil: \"Pencil\"\n", "");
        })
        .unwrap_err();
        assert!(error.contains("label"), "{error}");
        assert!(error.contains("pencil"), "{error}");
    }

    #[test]
    fn a_key_row_with_a_missing_field_fails_by_name() {
        let error = rebuilt(|s| {
            s.keys = s.keys.replace(
                "KeyShortcut(\"Brush width −1\", win: \"[\", mac: \"[\", scope: \"Canvas\")",
                "KeyShortcut(\"Brush width −1\", win: \"[\", scope: \"Canvas\")",
            );
        })
        .unwrap_err();
        assert!(error.contains("Brush width −1"), "{error}");
        assert!(error.contains("mac"), "{error}");
    }

    #[test]
    fn a_missing_table_fails_and_says_what_is_gone() {
        let error = rebuilt(|s| s.menu = s.menu.replace("paintNetEffects", "other")).unwrap_err();
        assert!(error.contains("paintNetEffects"), "{error}");
    }

    #[test]
    fn zoom_labels_follow_the_swift_format() {
        let labels = zoom_labels("static let values: [Double] = [0.005, 0.5, 1, 32]").unwrap();
        assert_eq!(labels, ["3200%", "100%", "50%", "0.5%"]);
    }

    #[test]
    fn the_icon_table_reads_a_row_with_an_escaped_quote() {
        let table = icon_table(
            "use = \"menu:Edit > Invert (\\\"xor\\\")\"\nfile = \"windows/assets/icons/a-b.svg\"\n",
        );
        assert_eq!(table["menu:Edit > Invert (\"xor\")"], "a-b");
    }

    #[test]
    fn a_string_with_no_end_fails_by_line() {
        let error = lex("x.swift", "let a = 1\nlet b = \"open\n").unwrap_err();
        assert!(error.contains("x.swift:2"), "{error}");
    }
}
