use anyhow::{bail, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    collections::HashSet,
    fs,
    io::{self, Write},
    path::Path,
    time::Duration,
};
use crate::{
    app::{activate, import_one, GcpFlags, Target},
    model::{Account, Registry},
    oauth, quota,
    store::Store,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    List,
    Switch,
    Remove,
}

// Vibrant Cyber/Neon color palette
pub const C_BANNER_1: Color = Color::Rgb { r: 244, g: 63, b: 142 };  // Rose / Magenta
pub const C_BANNER_2: Color = Color::Rgb { r: 168, g: 85, b: 247 };  // Electric Purple
pub const C_BANNER_3: Color = Color::Rgb { r: 99, g: 102, b: 241 };  // Indigo
pub const C_BANNER_4: Color = Color::Rgb { r: 59, g: 130, b: 246 };  // Sky Blue
pub const C_BANNER_5: Color = Color::Rgb { r: 6, g: 182, b: 212 };   // Bright Cyan
pub const C_BANNER_6: Color = Color::Rgb { r: 16, g: 185, b: 129 };  // Emerald Green

pub const C_BORDER: Color = Color::Rgb { r: 0, g: 220, b: 255 };     // Vibrant Neon Cyan
pub const C_BORDER_DIM: Color = Color::Rgb { r: 20, g: 110, b: 170 };// Muted Cyan Divider
pub const C_TITLE: Color = Color::Rgb { r: 251, g: 191, b: 36 };     // Glowing Amber / Gold

pub const C_ACC_COUNT: Color = Color::Rgb { r: 56, g: 189, b: 248 }; // Neon Cyan
pub const C_ACT_COUNT: Color = Color::Rgb { r: 52, g: 211, b: 153 }; // Neon Emerald
pub const C_INA_COUNT: Color = Color::Rgb { r: 251, g: 113, b: 133 }; // Coral Pink

pub const C_ACTIVE_DOT: Color = Color::Rgb { r: 74, g: 222, b: 128 }; // Glowing Spring Green
pub const C_IDLE_DOT: Color = Color::Rgb { r: 148, g: 163, b: 184 };  // Slate Gray

pub const C_SEL_BG: Color = Color::Rgb { r: 35, g: 45, b: 75 };       // Midnight Cyber Navy
pub const C_SEL_ID: Color = Color::Rgb { r: 253, g: 224, b: 71 };     // Bright Gold
pub const C_TEXT: Color = Color::Rgb { r: 241, g: 245, b: 249 };       // Crisp White
pub const C_MUTED: Color = Color::Rgb { r: 148, g: 163, b: 184 };      // Slate Muted
pub const C_DARK: Color = Color::Rgb { r: 71, g: 85, b: 105 };         // Divider Slate

pub const C_PROF_DEF: Color = Color::Rgb { r: 56, g: 189, b: 248 };   // Cyan DEFAULT
pub const C_PROF_ISO: Color = Color::Rgb { r: 192, g: 132, b: 252 };  // Lavender ISOLATED

pub const C_KEY: Color = Color::Rgb { r: 250, g: 204, b: 21 };         // Amber/Gold Keys
pub const C_PROMPT: Color = Color::Rgb { r: 74, g: 222, b: 128 };      // Emerald Prompt

// Aliases for legacy compatibility
pub const C_CYAN: Color = C_BORDER;
pub const C_GREEN: Color = C_ACT_COUNT;

struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, cursor::Hide)?;
        Ok(Self)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), ResetColor, cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

pub fn interact(accounts: &[Account], active: &str, mode: Mode) -> Result<Vec<usize>> {
    if accounts.is_empty() {
        bail!("no stored accounts");
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        bail!("interactive terminal required; pass an account query");
    }
    let _guard = TerminalGuard::enter()?;
    let mut selected = accounts.iter().position(|a| a.id == active).unwrap_or(0);
    let mut marked = HashSet::<usize>::new();
    let mut offset = 0usize;
    loop {
        let (_, height) = terminal::size()?;
        let visible = (height as usize).saturating_sub(6).max(1);
        if selected < offset {
            offset = selected;
        }
        if selected >= offset + visible {
            offset = selected + 1 - visible;
        }
        paint_legacy(accounts, active, mode, selected, &marked, offset, visible)?;
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let ev = event::read()?;
        let Event::Key(key) = ev else { continue };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(vec![]),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(vec![]),
            KeyCode::Up | KeyCode::Char('k') => {
                selected = selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                selected = (selected + 1).min(accounts.len() - 1);
            }
            KeyCode::Home => selected = 0,
            KeyCode::End => selected = accounts.len() - 1,
            KeyCode::PageUp => selected = selected.saturating_sub(visible),
            KeyCode::PageDown => selected = (selected + visible).min(accounts.len() - 1),
            KeyCode::Char(' ') if mode == Mode::Remove => {
                if !marked.insert(selected) {
                    marked.remove(&selected);
                }
            }
            KeyCode::Enter => {
                return Ok(match mode {
                    Mode::List => vec![],
                    Mode::Switch => vec![selected],
                    Mode::Remove => {
                        if marked.is_empty() {
                            vec![selected]
                        } else {
                            let mut v: Vec<_> = marked.into_iter().collect();
                            v.sort_unstable();
                            v
                        }
                    }
                })
            }
            KeyCode::Char(x) if x.is_ascii_digit() => {
                if let Some(n) = x.to_digit(10) {
                    if n > 0 && (n as usize) <= accounts.len() {
                        selected = n as usize - 1;
                    }
                }
            }
            _ => (),
        }
    }
}

fn paint_legacy(
    accounts: &[Account],
    active: &str,
    mode: Mode,
    selected: usize,
    marked: &HashSet<usize>,
    offset: usize,
    visible: usize,
) -> Result<()> {
    let mut out = io::stdout();
    let (width, _) = terminal::size()?;
    let w = (width as usize).max(12);
    execute!(
        out,
        cursor::MoveTo(0, 0),
        terminal::Clear(ClearType::All),
        SetForegroundColor(C_CYAN),
        Print(" agy-auth-rust "),
        ResetColor
    )?;
    let title = match mode {
        Mode::List => "Accounts",
        Mode::Switch => "Select account",
        Mode::Remove => "Select accounts to remove",
    };
    execute!(out, Print(format!("| {title} | {} stored\r\n\r\n", accounts.len())))?;
    for (i, a) in accounts.iter().enumerate().skip(offset).take(visible) {
        let indicator = if i == selected { '>' } else { ' ' };
        let active_sign = if a.id == active { '*' } else { ' ' };
        let selected_sign = if marked.contains(&i) { 'x' } else { ' ' };
        let prefix = if mode == Mode::Remove {
            format!("{indicator} [{selected_sign}] {:02} {active_sign} ", i + 1)
        } else {
            format!("{indicator} {:02} {active_sign} ", i + 1)
        };
        let label = a.display();
        let text = format!("{prefix}{label}");
        let value: String = text.chars().take(w.saturating_sub(1)).collect();
        if i == selected {
            execute!(out, SetForegroundColor(C_GREEN))?;
        }
        execute!(out, Print(value), ResetColor, Print("\r\n"))?;
    }
    execute!(out, Print("\r\n"))?;
    let help = match mode {
        Mode::List => "↑↓ Move  |  q/Esc Close",
        Mode::Switch => "↑↓ Move  |  Enter Switch  |  q/Esc Cancel",
        Mode::Remove => "↑↓ Move  |  Space Mark  |  Enter Remove  |  q/Esc Cancel",
    };
    execute!(out, SetForegroundColor(C_MUTED), Print(help), ResetColor)?;
    out.flush()?;
    Ok(())
}

#[derive(Clone)]
struct Span {
    text: String,
    fg: Color,
    bg: Color,
}
type Line = Vec<Span>;

fn span(text: impl Into<String>, fg: Color) -> Span {
    Span {
        text: text.into(),
        fg,
        bg: Color::Reset,
    }
}

fn span_bg(text: impl Into<String>, fg: Color, bg: Color) -> Span {
    Span {
        text: text.into(),
        fg,
        bg,
    }
}

fn trunc_str(s: &str, max_len: usize) -> String {
    let count = s.chars().count();
    if count <= max_len {
        s.to_string()
    } else if max_len <= 2 {
        s.chars().take(max_len).collect()
    } else {
        let prefix: String = s.chars().take(max_len - 2).collect();
        format!("{prefix}..")
    }
}

struct LineBuilder {
    pad: String,
    width: usize, // 73
    inner: usize, // 71
}

impl LineBuilder {
    fn new(pad: &str) -> Self {
        Self {
            pad: pad.to_string(),
            width: 73,
            inner: 71,
        }
    }

    fn pad_span(&self) -> Span {
        span(&self.pad, Color::Reset)
    }

    fn raw_line(&self, spans: Vec<Span>) -> Line {
        let mut l = vec![self.pad_span()];
        l.extend(spans);
        l
    }

    fn box_top(&self, title: &str, border_col: Color, title_col: Color) -> Line {
        let prefix = format!("╭─ {title} ");
        let dashes = self.width.saturating_sub(prefix.chars().count() + 1);
        vec![
            self.pad_span(),
            span("╭─ ", border_col),
            span(title, title_col),
            span(format!(" {}╮", "─".repeat(dashes)), border_col),
        ]
    }

    fn box_top_badge(&self, title: &str, badge: &str, border_col: Color, title_col: Color, badge_col: Color) -> Line {
        let prefix = format!("╭─ {title} ");
        let suffix = format!(" {badge} ╮");
        let dashes = self.width.saturating_sub(prefix.chars().count() + suffix.chars().count());
        vec![
            self.pad_span(),
            span("╭─ ", border_col),
            span(title, title_col),
            span(format!(" {} ", "─".repeat(dashes)), border_col),
            span(badge, badge_col),
            span(" ╮", border_col),
        ]
    }

    fn box_bottom(&self, border_col: Color) -> Line {
        vec![
            self.pad_span(),
            span(format!("╰{}╯", "─".repeat(self.inner)), border_col),
        ]
    }

    fn box_divider(&self, border_col: Color) -> Line {
        vec![
            self.pad_span(),
            span(format!("├{}┤", "─".repeat(self.inner)), border_col),
        ]
    }

    fn box_empty(&self, border_col: Color) -> Line {
        vec![
            self.pad_span(),
            span("│", border_col),
            span(" ".repeat(self.inner), Color::Reset),
            span("│", border_col),
        ]
    }

    fn box_row(&self, spans: Vec<Span>, border_col: Color) -> Line {
        let mut total_chars = 0;
        for s in &spans {
            total_chars += s.text.chars().count();
        }
        let mut l = Vec::with_capacity(spans.len() + 3);
        l.push(self.pad_span());
        l.push(span("│", border_col));
        l.extend(spans);
        if total_chars < self.inner {
            l.push(span(" ".repeat(self.inner - total_chars), Color::Reset));
        }
        l.push(span("│", border_col));
        l
    }
}

enum InputState {
    Normal,
    ConfirmDelete(String, String),
    PromptImport,
    PromptAdd,
}

pub fn dashboard(
    store: &Store,
    reg: &mut Registry,
    target: Target,
    close_desktop: bool,
    relaunch: bool,
) -> Result<Option<String>> {
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        bail!("interactive terminal required for switcher dashboard");
    }
    let _guard = TerminalGuard::enter()?;
    let mut selected = reg.accounts.iter().position(|a| a.id == reg.active).unwrap_or(0);
    let mut offset = 0usize;
    let mut input_buf = String::new();
    let mut status_msg = "Select account or enter command".to_string();
    let mut state = InputState::Normal;

    loop {
        if reg.accounts.is_empty() {
            bail!("no registered accounts in registry");
        }
        if selected >= reg.accounts.len() {
            selected = reg.accounts.len() - 1;
        }

        let (term_w, term_h) = terminal::size()?;
        let width = term_w as usize;
        let height = (term_h as usize).max(10);

        let pad_len = width.saturating_sub(73) / 2;
        let pad = " ".repeat(pad_len);

        let compact = height < 34;

        let vault_visible = if compact {
            3.min(reg.accounts.len())
        } else if height >= 46 {
            (height.saturating_sub(38)).clamp(4, 10).min(reg.accounts.len())
        } else {
            4.min(reg.accounts.len())
        };

        if selected < offset {
            offset = selected;
        }
        if selected >= offset + vault_visible {
            offset = selected + 1 - vault_visible;
        }

        let lines = if compact {
            build_compact_lines(reg, selected, offset, vault_visible, &input_buf, &status_msg, &state, &pad)
        } else {
            build_full_lines(reg, selected, offset, vault_visible, &input_buf, &status_msg, &state, &pad)
        };

        render_frame(&lines, height, width)?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let ev = event::read()?;
        let Event::Key(key) = ev else { continue };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        match &state {
            InputState::ConfirmDelete(id_to_del, email_to_del) => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let id = id_to_del.clone();
                    let email = email_to_del.clone();
                    let _ = fs::remove_file(store.paths.snapshot(&id));
                    let _ = fs::remove_file(store.paths.install_snapshot(&id));
                    reg.accounts.retain(|a| a.id != id);
                    if reg.active == id {
                        reg.active.clear();
                    }
                    let _ = reg.normalize();
                    let _ = store.save(reg);
                    selected = selected.min(reg.accounts.len().saturating_sub(1));
                    status_msg = format!("Account {email} deleted.");
                    state = InputState::Normal;
                    input_buf.clear();
                }
                _ => {
                    status_msg = "Delete canceled.".to_string();
                    state = InputState::Normal;
                    input_buf.clear();
                }
            },
            InputState::PromptAdd => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    match store.read_live() {
                        Ok(cred) => {
                            let email = cred.email.clone();
                            let _ = store.upsert(reg, &cred, "");
                            let _ = reg.normalize();
                            let _ = store.save(reg);
                            status_msg = format!("Added current CLI session for {email}.");
                        }
                        Err(e) => {
                            status_msg = format!("Failed to read live CLI credential: {e}");
                        }
                    }
                    state = InputState::Normal;
                    input_buf.clear();
                }
                KeyCode::Char('l') | KeyCode::Char('L') => {
                    status_msg = "Opening browser for Google OAuth login...".to_string();
                    let lines = if compact {
                        build_compact_lines(reg, selected, offset, vault_visible, &input_buf, &status_msg, &state, &pad)
                    } else {
                        build_full_lines(reg, selected, offset, vault_visible, &input_buf, &status_msg, &state, &pad)
                    };
                    let _ = render_frame(&lines, height, width);
                    match oauth::login(false, "", false) {
                        Ok(cred) => {
                            let email = cred.email.clone();
                            let _ = store.upsert(reg, &cred, "");
                            let _ = reg.normalize();
                            let _ = store.save(reg);
                            status_msg = format!("Signed in and added {email}.");
                        }
                        Err(e) => {
                            status_msg = format!("OAuth login failed: {e}");
                        }
                    }
                    state = InputState::Normal;
                    input_buf.clear();
                }
                _ => {
                    status_msg = "Add canceled.".to_string();
                    state = InputState::Normal;
                    input_buf.clear();
                }
            },
            InputState::PromptImport => match key.code {
                KeyCode::Enter => {
                    let path_str = input_buf.trim().to_string();
                    if path_str.is_empty() {
                        status_msg = "Import canceled (empty path).".to_string();
                    } else {
                        let path = Path::new(&path_str);
                        match import_one(store, reg, path, "", &GcpFlags::default(), &store.paths.key()) {
                            Ok(ids) => {
                                let _ = reg.normalize();
                                let _ = store.save(reg);
                                status_msg = format!("Imported {} account(s).", ids.len());
                            }
                            Err(e) => {
                                status_msg = format!("Import failed: {e}");
                            }
                        }
                    }
                    state = InputState::Normal;
                    input_buf.clear();
                }
                KeyCode::Esc => {
                    status_msg = "Import canceled.".to_string();
                    state = InputState::Normal;
                    input_buf.clear();
                }
                KeyCode::Backspace => {
                    input_buf.pop();
                }
                KeyCode::Char(c) => {
                    input_buf.push(c);
                }
                _ => (),
            },
            InputState::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(None),
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = selected.saturating_sub(1);
                    input_buf.clear();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1).min(reg.accounts.len().saturating_sub(1));
                    input_buf.clear();
                }
                KeyCode::Home => {
                    selected = 0;
                    input_buf.clear();
                }
                KeyCode::End => {
                    selected = reg.accounts.len().saturating_sub(1);
                    input_buf.clear();
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    state = InputState::PromptAdd;
                    status_msg = "Add current CLI login? [y/N] (or 'l' for OAuth browser login):".to_string();
                    input_buf.clear();
                }
                KeyCode::Char('d') | KeyCode::Char('D') => {
                    let acc = &reg.accounts[selected];
                    state = InputState::ConfirmDelete(acc.id.clone(), acc.email.clone());
                    status_msg = format!("Delete account {}? [y/N]:", acc.email);
                    input_buf.clear();
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    status_msg = "Fetching quotas from Code Assist API...".to_string();
                    let lines = if compact {
                        build_compact_lines(reg, selected, offset, vault_visible, &input_buf, &status_msg, &state, &pad)
                    } else {
                        build_full_lines(reg, selected, offset, vault_visible, &input_buf, &status_msg, &state, &pad)
                    };
                    render_frame(&lines, height, width)?;
                    let usage = quota::fetch_all(store, &reg.accounts);
                    for a in &mut reg.accounts {
                        if let Some((_, Ok(u))) = usage.iter().find(|(uid, _)| uid == &a.id) {
                            if !u.plan.is_empty() {
                                a.plan = Some(u.plan.clone());
                            }
                        }
                    }
                    let _ = reg.normalize();
                    let _ = store.save(reg);
                    status_msg = "Quotas refreshed and premium accounts resorted to top.".to_string();
                }
                KeyCode::Char('i') | KeyCode::Char('I') => {
                    state = InputState::PromptImport;
                    status_msg = "Enter path to credential or backup JSON:".to_string();
                    input_buf.clear();
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    input_buf.push(c);
                    if let Ok(num) = input_buf.parse::<usize>() {
                        if num >= 1 && num <= reg.accounts.len() {
                            selected = num - 1;
                            status_msg = format!("Selected #{num}. Press Enter to switch, or Backspace to clear.");
                        } else {
                            status_msg = format!("Row #{num} out of range (1-{})", reg.accounts.len());
                        }
                    }
                }
                KeyCode::Backspace => {
                    input_buf.pop();
                    if input_buf.is_empty() {
                        status_msg = "Select account or enter command".to_string();
                    } else if let Ok(num) = input_buf.parse::<usize>() {
                        if num >= 1 && num <= reg.accounts.len() {
                            selected = num - 1;
                        }
                    }
                }
                KeyCode::Enter => {
                    if !input_buf.is_empty() {
                        if let Ok(num) = input_buf.parse::<usize>() {
                            if num >= 1 && num <= reg.accounts.len() {
                                selected = num - 1;
                            }
                        }
                    }
                    let target_account = reg.accounts[selected].clone();
                    let target_id = target_account.id.clone();
                    let name = target_account.display();

                    drop(_guard);
                    let messages = activate(store, reg, &target_id, target, close_desktop, relaunch)?;
                    println!("Switched to {name}. Restart running agy sessions.");
                    for msg in messages {
                        eprintln!("warning: {msg}");
                    }
                    return Ok(Some(target_id));
                }
                _ => (),
            },
        }
    }
}

fn render_frame(lines: &[Line], term_h: usize, _term_w: usize) -> Result<()> {
    let mut out = io::stdout();
    execute!(out, cursor::MoveTo(0, 0))?;
    let render_rows = lines.len().min(term_h);
    for (row_idx, line) in lines.iter().take(render_rows).enumerate() {
        execute!(out, cursor::MoveTo(0, row_idx as u16), terminal::Clear(ClearType::CurrentLine))?;
        for span in line {
            if span.bg != Color::Reset {
                execute!(out, SetBackgroundColor(span.bg))?;
            }
            if span.fg != Color::Reset {
                execute!(out, SetForegroundColor(span.fg))?;
            }
            execute!(out, Print(&span.text), ResetColor)?;
        }
    }
    // Clear leftover lines below rendered frame
    for r in render_rows..term_h {
        execute!(out, cursor::MoveTo(0, r as u16), terminal::Clear(ClearType::CurrentLine))?;
    }
    out.flush()?;
    Ok(())
}

fn build_full_lines(
    reg: &Registry,
    selected: usize,
    offset: usize,
    vault_visible: usize,
    input_buf: &str,
    status_msg: &str,
    state: &InputState,
    pad: &str,
) -> Vec<Line> {
    let b = LineBuilder::new(pad);
    let total_count = reg.accounts.len();
    let active_count = if reg.accounts.iter().any(|a| a.id == reg.active) { 1 } else { 0 };
    let inactive_count = total_count.saturating_sub(active_count);

    let mut lines = Vec::with_capacity(50);

    // 1. Neon Glowing ASCII Art Banner
    let banner = [
        ("    █████╗  ██████╗ ██╗   ██╗      █████╗ ██╗   ██╗████████╗██╗  ██╗     ", C_BANNER_1),
        ("   ██╔══██╗██╔════╝ ╚██╗ ██╔╝     ██╔══██╗██║   ██║╚══██╔══╝██║  ██║     ", C_BANNER_2),
        ("   ███████║██║  ███╗ ╚████╔╝      ███████║██║   ██║   ██║   ███████║     ", C_BANNER_3),
        ("   ██╔══██║██║   ██║  ╚██╔╝       ██╔══██║██║   ██║   ██║   ██╔══██║     ", C_BANNER_4),
        ("   ██║  ██║╚██████╔╝   ██║        ██║  ██║╚██████╔╝   ██║   ██║  ██║     ", C_BANNER_5),
        ("   ╚═╝  ╚═╝ ╚═════╝    ╚═╝        ╚═╝  ╚═╝ ╚═════╝    ╚═╝   ╚═╝  ╚═╝     ", C_BANNER_6),
    ];
    for (line_str, col) in banner {
        lines.push(b.raw_line(vec![span(line_str, col)]));
    }

    // 2. Subheader
    lines.push(b.raw_line(vec![
        span(format!("   {:<48}", "A G Y  A U T H  /  R U S T"), C_TEXT),
        span(format!("{:>22}", "ACCOUNT MANAGER"), C_ACC_COUNT),
    ]));
    lines.push(b.raw_line(vec![span("━".repeat(73), C_BORDER_DIM)]));
    lines.push(b.raw_line(vec![
        span(format!("   {:<50}", "MULTI-ACCOUNT CONTROL"), C_MUTED),
        span("● ", C_ACT_COUNT),
        span("SYSTEM READY   ", C_TEXT),
    ]));

    // 3. DASHBOARD Card
    lines.push(b.box_top("DASHBOARD", C_BORDER, C_TITLE));
    lines.push(b.box_empty(C_BORDER));
    lines.push(b.box_row(vec![
        span(format!("{:^23}", "ACCOUNTS"), C_ACC_COUNT),
        span(format!("{:^25}", "ACTIVE"), C_ACT_COUNT),
        span(format!("{:^23}", "INACTIVE"), C_INA_COUNT),
    ], C_BORDER));
    lines.push(b.box_row(vec![
        span(format!("{:^23}", format!("{:02}", total_count)), C_ACC_COUNT),
        span(format!("{:^25}", format!("{:02}", active_count)), C_ACT_COUNT),
        span(format!("{:^23}", format!("{:02}", inactive_count)), C_INA_COUNT),
    ], C_BORDER));
    lines.push(b.box_row(vec![
        span(format!("{:^23}", "━━━━━━━━━"), C_ACC_COUNT),
        span(format!("{:^25}", "━━━━━━━━━"), C_ACT_COUNT),
        span(format!("{:^23}", "━━━━━━━━━"), C_INA_COUNT),
    ], C_BORDER));
    lines.push(b.box_empty(C_BORDER));
    lines.push(b.box_bottom(C_BORDER));

    // 4. ACCOUNT VAULT Card
    lines.push(b.box_top("ACCOUNT VAULT", C_BORDER, C_TITLE));
    lines.push(b.box_row(vec![
        span("  ID   STATUS     ACCOUNT / ALIAS             PROFILE       ADDED      ", C_TITLE)
    ], C_BORDER));
    lines.push(b.box_row(vec![
        span("  ──   ─────────  ──────────────────────────  ───────────   ────────── ", C_DARK)
    ], C_BORDER));

    for i in offset..(offset + vault_visible).min(total_count) {
        let acc = &reg.accounts[i];
        let is_active = acc.id == reg.active;
        let is_selected = i == selected;

        let id_str = format!("{:02}", i + 1);
        let (dot, st_txt, st_col) = if is_active {
            ("●", "ACTIVE", C_ACTIVE_DOT)
        } else {
            ("○", "IDLE  ", C_IDLE_DOT)
        };

        let name = if acc.alias.is_empty() {
            acc.email.clone()
        } else {
            format!("{} ({})", acc.alias, acc.email)
        };
        let name_padded = format!("{:<26}", trunc_str(&name, 26));

        let (prof_str, prof_col) = if is_active || acc.has_desktop {
            ("DEFAULT    ", C_PROF_DEF)
        } else {
            ("ISOLATED   ", C_PROF_ISO)
        };
        let added_str = acc.added_at.format("%Y-%m-%d").to_string();

        let row_bg = if is_selected { C_SEL_BG } else { Color::Reset };
        let text_col = if is_selected { C_TEXT } else { C_TEXT };

        lines.push(b.box_row(vec![
            span_bg("  ", text_col, row_bg),
            span_bg(id_str, if is_selected { C_SEL_ID } else { C_ACC_COUNT }, row_bg),
            span_bg("   ", text_col, row_bg),
            span_bg(dot, st_col, row_bg),
            span_bg(" ", text_col, row_bg),
            span_bg(st_txt, st_col, row_bg),
            span_bg("   ", text_col, row_bg),
            span_bg(name_padded, if !acc.alias.is_empty() { C_TITLE } else { text_col }, row_bg),
            span_bg("  ", text_col, row_bg),
            span_bg(prof_str, prof_col, row_bg),
            span_bg("  ", text_col, row_bg),
            span_bg(added_str, C_MUTED, row_bg),
            span_bg(" ", text_col, row_bg),
        ], C_BORDER));
    }

    lines.push(b.box_divider(C_BORDER));
    lines.push(b.box_row(vec![
        span("  ● ACTIVE: ", C_ACT_COUNT),
        span(format!("{:02}", active_count), C_TEXT),
        span("          ○ INACTIVE: ", C_INA_COUNT),
        span(format!("{:02}", inactive_count), C_TEXT),
        span("          TOTAL: ", C_TITLE),
        span(format!("{:02}", total_count), C_ACC_COUNT),
    ], C_BORDER));
    lines.push(b.box_bottom(C_BORDER));

    // 5. ACCOUNT DETAILS Card
    let cur_acc = &reg.accounts[selected];
    let badge = format!("[ {:02} / {:02} ]", selected + 1, total_count);
    lines.push(b.box_top_badge("ACCOUNT DETAILS", &badge, C_BORDER, C_TITLE, C_KEY));

    let email_display = cur_acc.display();
    lines.push(b.box_row(vec![
        span("  EMAIL       ", C_TITLE),
        span(format!("{:<57}", trunc_str(&email_display, 57)), C_TEXT),
    ], C_BORDER));

    let prof_display = if cur_acc.id == reg.active || cur_acc.has_desktop { "DEFAULT" } else { "ISOLATED" };
    lines.push(b.box_row(vec![
        span("  PROFILE     ", C_TITLE),
        span(format!("{:<57}", prof_display), if cur_acc.id == reg.active || cur_acc.has_desktop { C_PROF_DEF } else { C_PROF_ISO }),
    ], C_BORDER));

    let is_cur_active = cur_acc.id == reg.active;
    lines.push(b.box_row(vec![
        span("  STATUS      ", C_TITLE),
        span(if is_cur_active { "● ACTIVE" } else { "○ IDLE  " }, if is_cur_active { C_ACTIVE_DOT } else { C_IDLE_DOT }),
    ], C_BORDER));

    let added_display = cur_acc.added_at.format("%Y-%m-%d").to_string();
    lines.push(b.box_row(vec![
        span("  ADDED       ", C_TITLE),
        span(format!("{:<57}", added_display), C_TEXT),
    ], C_BORDER));

    let (usage_text, usage_col) = match cur_acc.plan.as_deref() {
        Some(p) if !p.is_empty() => (format!("{p}  ·  Verified Tier"), C_ACTIVE_DOT),
        _ => ("N/A  ·  AGY AUTH API data unavailable".to_string(), C_MUTED),
    };
    lines.push(b.box_row(vec![
        span("  USAGE       ", C_TITLE),
        span(format!("{:<57}", trunc_str(&usage_text, 57)), usage_col),
    ], C_BORDER));
    lines.push(b.box_bottom(C_BORDER));

    // 6. COMMAND CENTER Card
    lines.push(b.box_top("COMMAND CENTER", C_BORDER, C_TITLE));
    lines.push(b.box_row(vec![
        span("  ", C_TEXT),
        span(format!("[1-{}]", total_count), C_KEY),
        span(" SWITCH       ", C_TEXT),
        span("[A]", C_KEY),
        span(" ADD         ", C_TEXT),
        span("[D]", C_KEY),
        span(" DELETE", C_TEXT),
    ], C_BORDER));
    lines.push(b.box_row(vec![
        span("  ", C_TEXT),
        span("[I]", C_KEY),
        span("    IMPORT       ", C_TEXT),
        span("[R]", C_KEY),
        span(" REFRESH     ", C_TEXT),
        span("[Q]", C_KEY),
        span(" QUIT", C_TEXT),
    ], C_BORDER));
    lines.push(b.box_bottom(C_BORDER));

    // 7. TERMINAL Card
    lines.push(b.box_top("TERMINAL", C_BORDER, C_TITLE));
    let prompt_display = match state {
        InputState::Normal => if input_buf.is_empty() { "_".to_string() } else { format!("{input_buf}_") },
        InputState::PromptImport => format!("{input_buf}_"),
        InputState::ConfirmDelete(_, _) | InputState::PromptAdd => "_".to_string(),
    };
    lines.push(b.box_row(vec![
        span("  ❯ ", C_PROMPT),
        span(format!("{:<67}", trunc_str(&prompt_display, 67)), C_TEXT),
    ], C_BORDER));
    lines.push(b.box_row(vec![
        span("  ", C_TEXT),
        span(format!("{:<69}", trunc_str(status_msg, 69)), C_TITLE),
    ], C_BORDER));
    lines.push(b.box_bottom(C_BORDER));

    // 8. Bottom Footer
    lines.push(b.raw_line(vec![
        span(format!("   {:<50}", "AGY AUTH / RUST"), C_TEXT),
        span("● ", C_ACT_COUNT),
        span("READY   ", C_TEXT),
    ]));
    lines.push(b.raw_line(vec![span("━".repeat(73), C_BORDER_DIM)]));

    lines
}

fn build_compact_lines(
    reg: &Registry,
    selected: usize,
    offset: usize,
    vault_visible: usize,
    input_buf: &str,
    status_msg: &str,
    state: &InputState,
    pad: &str,
) -> Vec<Line> {
    let b = LineBuilder::new(pad);
    let total_count = reg.accounts.len();
    let active_count = if reg.accounts.iter().any(|a| a.id == reg.active) { 1 } else { 0 };
    let inactive_count = total_count.saturating_sub(active_count);

    let mut lines = Vec::with_capacity(25);

    // 1. Compact Header (2 lines)
    lines.push(b.raw_line(vec![
        span(format!("{:^73}", "█████╗  ██████╗ ██╗   ██╗      █████╗ ██╗   ██╗████████╗██╗  ██╗"), C_BANNER_2),
    ]));
    lines.push(b.raw_line(vec![span("━".repeat(73), C_BORDER_DIM)]));

    // 2. DASHBOARD Stat Box (5 lines)
    lines.push(b.box_top("DASHBOARD", C_BORDER, C_TITLE));
    lines.push(b.box_row(vec![
        span(format!("{:^23}", "ACCOUNTS"), C_ACC_COUNT),
        span(format!("{:^25}", "ACTIVE"), C_ACT_COUNT),
        span(format!("{:^23}", "INACTIVE"), C_INA_COUNT),
    ], C_BORDER));
    lines.push(b.box_row(vec![
        span(format!("{:^23}", format!("{:02}", total_count)), C_ACC_COUNT),
        span(format!("{:^25}", format!("{:02}", active_count)), C_ACT_COUNT),
        span(format!("{:^23}", format!("{:02}", inactive_count)), C_INA_COUNT),
    ], C_BORDER));
    lines.push(b.box_row(vec![
        span(format!("{:^23}", "━━━━━━━━━"), C_ACC_COUNT),
        span(format!("{:^25}", "━━━━━━━━━"), C_ACT_COUNT),
        span(format!("{:^23}", "━━━━━━━━━"), C_INA_COUNT),
    ], C_BORDER));
    lines.push(b.box_bottom(C_BORDER));

    // 3. ACCOUNT VAULT Card (3 + vault_visible + 2 lines = 8 lines)
    lines.push(b.box_top("ACCOUNT VAULT", C_BORDER, C_TITLE));
    lines.push(b.box_row(vec![
        span("  ID   STATUS     ACCOUNT / ALIAS             PROFILE       ADDED      ", C_TITLE)
    ], C_BORDER));
    lines.push(b.box_row(vec![
        span("  ──   ─────────  ──────────────────────────  ───────────   ────────── ", C_DARK)
    ], C_BORDER));

    for i in offset..(offset + vault_visible).min(total_count) {
        let acc = &reg.accounts[i];
        let is_active = acc.id == reg.active;
        let is_selected = i == selected;

        let id_str = format!("{:02}", i + 1);
        let (dot, st_txt, st_col) = if is_active {
            ("●", "ACTIVE", C_ACTIVE_DOT)
        } else {
            ("○", "IDLE  ", C_IDLE_DOT)
        };

        let name = if acc.alias.is_empty() {
            acc.email.clone()
        } else {
            format!("{} ({})", acc.alias, acc.email)
        };
        let name_padded = format!("{:<26}", trunc_str(&name, 26));

        let (prof_str, prof_col) = if is_active || acc.has_desktop {
            ("DEFAULT    ", C_PROF_DEF)
        } else {
            ("ISOLATED   ", C_PROF_ISO)
        };
        let added_str = acc.added_at.format("%Y-%m-%d").to_string();

        let row_bg = if is_selected { C_SEL_BG } else { Color::Reset };
        let text_col = if is_selected { C_TEXT } else { C_TEXT };

        lines.push(b.box_row(vec![
            span_bg("  ", text_col, row_bg),
            span_bg(id_str, if is_selected { C_SEL_ID } else { C_ACC_COUNT }, row_bg),
            span_bg("   ", text_col, row_bg),
            span_bg(dot, st_col, row_bg),
            span_bg(" ", text_col, row_bg),
            span_bg(st_txt, st_col, row_bg),
            span_bg("   ", text_col, row_bg),
            span_bg(name_padded, if !acc.alias.is_empty() { C_TITLE } else { text_col }, row_bg),
            span_bg("  ", text_col, row_bg),
            span_bg(prof_str, prof_col, row_bg),
            span_bg("  ", text_col, row_bg),
            span_bg(added_str, C_MUTED, row_bg),
            span_bg(" ", text_col, row_bg),
        ], C_BORDER));
    }

    lines.push(b.box_row(vec![
        span("  ● ACTIVE: ", C_ACT_COUNT),
        span(format!("{:02}", active_count), C_TEXT),
        span("          ○ INACTIVE: ", C_INA_COUNT),
        span(format!("{:02}", inactive_count), C_TEXT),
        span("          TOTAL: ", C_TITLE),
        span(format!("{:02}", total_count), C_ACC_COUNT),
    ], C_BORDER));
    lines.push(b.box_bottom(C_BORDER));

    // 4. Compact ACCOUNT DETAILS Card (4 lines)
    let cur_acc = &reg.accounts[selected];
    let badge = format!("[ {:02} / {:02} ]", selected + 1, total_count);
    lines.push(b.box_top_badge("ACCOUNT DETAILS", &badge, C_BORDER, C_TITLE, C_KEY));

    let email_display = cur_acc.display();
    lines.push(b.box_row(vec![
        span("  EMAIL: ", C_TITLE),
        span(format!("{:<62}", trunc_str(&email_display, 62)), C_TEXT),
    ], C_BORDER));

    let prof_display = if cur_acc.id == reg.active || cur_acc.has_desktop { "DEFAULT" } else { "ISOLATED" };
    let is_cur_active = cur_acc.id == reg.active;
    let (usage_text, usage_col) = match cur_acc.plan.as_deref() {
        Some(p) if !p.is_empty() => (format!("{p} (Verified)"), C_ACTIVE_DOT),
        _ => ("N/A".to_string(), C_MUTED),
    };
    lines.push(b.box_row(vec![
        span("  PROFILE: ", C_TITLE),
        span(format!("{:<10}", prof_display), if cur_acc.id == reg.active || cur_acc.has_desktop { C_PROF_DEF } else { C_PROF_ISO }),
        span("   STATUS: ", C_TITLE),
        span(if is_cur_active { "● ACTIVE  " } else { "○ IDLE    " }, if is_cur_active { C_ACTIVE_DOT } else { C_IDLE_DOT }),
        span("   PLAN: ", C_TITLE),
        span(format!("{:<19}", trunc_str(&usage_text, 19)), usage_col),
    ], C_BORDER));
    lines.push(b.box_bottom(C_BORDER));

    // 5. Compact COMMAND & TERMINAL Card (4 lines)
    lines.push(b.box_top("COMMAND CENTER & TERMINAL", C_BORDER, C_TITLE));
    lines.push(b.box_row(vec![
        span("  ", C_TEXT),
        span(format!("[1-{}]", total_count), C_KEY),
        span(" SWITCH   ", C_TEXT),
        span("[A]", C_KEY),
        span(" ADD   ", C_TEXT),
        span("[D]", C_KEY),
        span(" DEL   ", C_TEXT),
        span("[I]", C_KEY),
        span(" IMP   ", C_TEXT),
        span("[R]", C_KEY),
        span(" REFRESH   ", C_TEXT),
        span("[Q]", C_KEY),
        span(" QUIT", C_TEXT),
    ], C_BORDER));

    let prompt_display = match state {
        InputState::Normal => if input_buf.is_empty() { "_".to_string() } else { format!("{input_buf}_") },
        InputState::PromptImport => format!("{input_buf}_"),
        InputState::ConfirmDelete(_, _) | InputState::PromptAdd => "_".to_string(),
    };
    let prompt_val = trunc_str(&prompt_display, 12);
    let status_val = trunc_str(status_msg, 50);
    lines.push(b.box_row(vec![
        span("  ❯ ", C_PROMPT),
        span(format!("{:<12}", prompt_val), C_TEXT),
        span("  ·  ", C_MUTED),
        span(format!("{:<50}", status_val), C_TITLE),
    ], C_BORDER));
    lines.push(b.box_bottom(C_BORDER));

    lines
}
