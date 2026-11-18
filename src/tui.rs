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
        SetForegroundColor(Color::Cyan),
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
            execute!(out, SetForegroundColor(Color::Green))?;
        }
        execute!(out, Print(value), ResetColor, Print("\r\n"))?;
    }
    execute!(out, Print("\r\n"))?;
    let help = match mode {
        Mode::List => "↑↓ Move  |  q/Esc Close",
        Mode::Switch => "↑↓ Move  |  Enter Switch  |  q/Esc Cancel",
        Mode::Remove => "↑↓ Move  |  Space Mark  |  Enter Remove  |  q/Esc Cancel",
    };
    execute!(out, SetForegroundColor(Color::DarkGrey), Print(help), ResetColor)?;
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
    let mut view_scroll = 0usize;
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
        let height = (term_h as usize).max(10);
        let vault_visible = if height >= 52 {
            (height.saturating_sub(44)).clamp(4, 12).min(reg.accounts.len())
        } else {
            4.min(reg.accounts.len())
        };

        if selected < offset {
            offset = selected;
        }
        if selected >= offset + vault_visible {
            offset = selected + 1 - vault_visible;
        }

        let lines = build_dashboard_lines(reg, selected, offset, vault_visible, &input_buf, &status_msg, &state);

        // Viewport scrolling
        let total_lines = lines.len();
        let max_scroll = total_lines.saturating_sub(height);

        match state {
            InputState::Normal => {
                if total_lines > height {
                    let vault_target_line = 26 + (selected.saturating_sub(offset));
                    if vault_target_line < view_scroll + 3 {
                        view_scroll = vault_target_line.saturating_sub(3);
                    } else if vault_target_line + 4 >= view_scroll + height {
                        view_scroll = (vault_target_line + 5).saturating_sub(height);
                    }
                    view_scroll = view_scroll.min(max_scroll);
                } else {
                    view_scroll = 0;
                }
            }
            _ => {
                view_scroll = max_scroll;
            }
        }

        render_frame(&lines, view_scroll, height, term_w)?;

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
                    let lines = build_dashboard_lines(reg, selected, offset, vault_visible, &input_buf, &status_msg, &state);
                    let _ = render_frame(&lines, view_scroll, height, term_w);
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
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                    return Ok(None);
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = selected.saturating_sub(1);
                    input_buf.clear();
                    status_msg = "Select account or enter command".to_string();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1).min(reg.accounts.len().saturating_sub(1));
                    input_buf.clear();
                    status_msg = "Select account or enter command".to_string();
                }
                KeyCode::Home => {
                    selected = 0;
                    input_buf.clear();
                }
                KeyCode::End => {
                    selected = reg.accounts.len().saturating_sub(1);
                    input_buf.clear();
                }
                KeyCode::PageUp => {
                    view_scroll = view_scroll.saturating_sub(6);
                }
                KeyCode::PageDown => {
                    view_scroll = (view_scroll + 6).min(max_scroll);
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
                    let lines = build_dashboard_lines(reg, selected, offset, vault_visible, &input_buf, &status_msg, &state);
                    render_frame(&lines, view_scroll, height, term_w)?;
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

fn render_frame(lines: &[Line], scroll_y: usize, height: usize, _width: u16) -> Result<()> {
    let mut out = io::stdout();
    execute!(out, cursor::MoveTo(0, 0))?;
    for (row_idx, line) in lines.iter().skip(scroll_y).take(height).enumerate() {
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
    out.flush()?;
    Ok(())
}

fn build_dashboard_lines(
    reg: &Registry,
    selected: usize,
    offset: usize,
    vault_visible: usize,
    input_buf: &str,
    status_msg: &str,
    state: &InputState,
) -> Vec<Line> {
    let total_count = reg.accounts.len();
    let active_count = if reg.accounts.iter().any(|a| a.id == reg.active) { 1 } else { 0 };
    let inactive_count = total_count.saturating_sub(active_count);

    let mut lines = Vec::with_capacity(70);

    // 1. ASCII Art Banner
    let banner_fg = Color::Cyan;
    lines.push(vec![span("    █████╗  ██████╗ ██╗   ██╗      █████╗ ██╗   ██╗████████╗██╗  ██╗", banner_fg)]);
    lines.push(vec![span("   ██╔══██╗██╔════╝ ╚██╗ ██╔╝     ██╔══██╗██║   ██║╚══██╔══╝██║  ██║", banner_fg)]);
    lines.push(vec![span("   ███████║██║  ███╗ ╚████╔╝      ███████║██║   ██║   ██║   ███████║", banner_fg)]);
    lines.push(vec![span("   ██╔══██║██║   ██║  ╚██╔╝       ██╔══██║██║   ██║   ██║   ██╔══██║", banner_fg)]);
    lines.push(vec![span("   ██║  ██║╚██████╔╝   ██║        ██║  ██║╚██████╔╝   ██║   ██║  ██║", banner_fg)]);
    lines.push(vec![span("   ╚═╝  ╚═╝ ╚═════╝    ╚═╝        ╚═╝  ╚═╝ ╚═════╝    ╚═╝   ╚═╝  ╚═╝", banner_fg)]);
    lines.push(vec![span("", Color::Reset)]);

    // 2. Subheader
    lines.push(vec![
        span("   A G Y  A U T H  /  R U S T", Color::White),
        span("                         ", Color::Reset),
        span("ACCOUNT MANAGER", Color::DarkGrey),
    ]);
    lines.push(vec![span("   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", Color::DarkCyan)]);
    lines.push(vec![
        span("   MULTI-ACCOUNT CONTROL", Color::DarkGrey),
        span("                                     ", Color::Reset),
        span("● ", Color::Green),
        span("SYSTEM READY", Color::White),
    ]);
    lines.push(vec![span("", Color::Reset)]);
    lines.push(vec![span("", Color::Reset)]);

    // 3. DASHBOARD Card
    let border_color = Color::DarkCyan;
    lines.push(vec![span("   ╭─ DASHBOARD ───────────────────────────────────────────────────────────╮", border_color)]);
    lines.push(vec![span("   │                                                                      │", border_color)]);
    lines.push(vec![
        span("   │", border_color),
        span("    ACCOUNTS              ACTIVE               INACTIVE               ", Color::White),
        span("│", border_color),
    ]);
    lines.push(vec![
        span("   │", border_color),
        span(format!("       {:02}                   {:02}                    {:02}                  ", total_count, active_count, inactive_count), Color::Cyan),
        span("│", border_color),
    ]);
    lines.push(vec![
        span("   │", border_color),
        span("    ━━━━━━━━━            ━━━━━━━━━             ━━━━━━━━━              ", Color::DarkCyan),
        span("│", border_color),
    ]);
    lines.push(vec![span("   │                                                                      │", border_color)]);
    lines.push(vec![span("   ╰──────────────────────────────────────────────────────────────────────╯", border_color)]);
    lines.push(vec![span("", Color::Reset)]);
    lines.push(vec![span("", Color::Reset)]);

    // 4. ACCOUNT VAULT Card
    lines.push(vec![span("   ╭─ ACCOUNT VAULT ───────────────────────────────────────────────────────╮", border_color)]);
    lines.push(vec![span("   │                                                                      │", border_color)]);
    lines.push(vec![
        span("   │", border_color),
        span("  ID   STATUS      ACCOUNT / ALIAS             PROFILE       ADDED    ", Color::DarkGrey),
        span("│", border_color),
    ]);
    lines.push(vec![
        span("   │", border_color),
        span("  ──   ─────────   ──────────────────────────  ───────────   ──────────", Color::DarkGrey),
        span("│", border_color),
    ]);
    lines.push(vec![span("   │                                                                      │", border_color)]);

    for i in offset..(offset + vault_visible).min(total_count) {
        let acc = &reg.accounts[i];
        let is_active = acc.id == reg.active;
        let is_selected = i == selected;

        let id_str = format!("{:02}", i + 1);
        let status_bullet = if is_active { "●" } else { "○" };
        let status_text = if is_active { "ACTIVE" } else { "IDLE  " };
        let status_col = if is_active { Color::Green } else { Color::DarkGrey };

        let name = if acc.alias.is_empty() {
            acc.email.clone()
        } else {
            format!("{} ({})", acc.alias, acc.email)
        };
        let name_trunc: String = name.chars().take(26).collect();
        let name_padded = format!("{:<26}", name_trunc);

        let profile = if is_active || acc.has_desktop { "DEFAULT    " } else { "ISOLATED   " };
        let added = acc.added_at.format("%Y-%m-%d").to_string();

        let row_bg = if is_selected { Color::Rgb { r: 25, g: 50, b: 85 } } else { Color::Reset };
        let text_fg = if is_selected { Color::White } else { Color::Reset };

        lines.push(vec![
            span("   │", border_color),
            span_bg("  ", text_fg, row_bg),
            span_bg(id_str, if is_selected { Color::Yellow } else { Color::White }, row_bg),
            span_bg("   ", text_fg, row_bg),
            span_bg(status_bullet, status_col, row_bg),
            span_bg(" ", text_fg, row_bg),
            span_bg(status_text, status_col, row_bg),
            span_bg("    ", text_fg, row_bg),
            span_bg(name_padded, text_fg, row_bg),
            span_bg(" ", text_fg, row_bg),
            span_bg(profile, if is_active { Color::Cyan } else { text_fg }, row_bg),
            span_bg("  ", text_fg, row_bg),
            span_bg(added, text_fg, row_bg),
            span_bg(" ", text_fg, row_bg),
            span("│", border_color),
        ]);
    }

    lines.push(vec![span("   │                                                                      │", border_color)]);
    lines.push(vec![span("   ├──────────────────────────────────────────────────────────────────────┤", border_color)]);
    lines.push(vec![
        span("   │", border_color),
        span("  ● ACTIVE: ", Color::Green),
        span(format!("{:02}", active_count), Color::White),
        span("          ○ INACTIVE: ", Color::DarkGrey),
        span(format!("{:02}", inactive_count), Color::White),
        span("          TOTAL: ", Color::White),
        span(format!("{:02}", total_count), Color::Cyan),
        span("             ", Color::Reset),
        span("│", border_color),
    ]);
    lines.push(vec![span("   ╰──────────────────────────────────────────────────────────────────────╯", border_color)]);
    lines.push(vec![span("", Color::Reset)]);

    // 5. ACCOUNT DETAILS Card
    let cur_acc = &reg.accounts[selected];
    lines.push(vec![span("   ╭─ ACCOUNT DETAILS ─────────────────────────────────────────────────────╮", border_color)]);
    lines.push(vec![span("   │                                                                      │", border_color)]);
    lines.push(vec![
        span("   │", border_color),
        span("  SELECTED ACCOUNT                                       ", Color::White),
        span(format!("[ {:02} / {:02} ]", selected + 1, total_count), Color::Cyan),
        span("  ", Color::Reset),
        span("│", border_color),
    ]);
    lines.push(vec![span("   │                                                                      │", border_color)]);

    let email_display = cur_acc.display();
    let email_trunc: String = email_display.chars().take(56).collect();
    lines.push(vec![
        span("   │", border_color),
        span("  EMAIL       ", Color::DarkGrey),
        span(format!("{:<56}", email_trunc), Color::White),
        span("│", border_color),
    ]);

    let prof_display = if cur_acc.id == reg.active || cur_acc.has_desktop { "DEFAULT" } else { "ISOLATED" };
    lines.push(vec![
        span("   │", border_color),
        span("  PROFILE     ", Color::DarkGrey),
        span(format!("{:<56}", prof_display), Color::White),
        span("│", border_color),
    ]);

    let is_cur_active = cur_acc.id == reg.active;
    lines.push(vec![
        span("   │", border_color),
        span("  STATUS      ", Color::DarkGrey),
        span(if is_cur_active { "● ACTIVE" } else { "○ IDLE  " }, if is_cur_active { Color::Green } else { Color::DarkGrey }),
        span(format!("{:<48}", ""), Color::Reset),
        span("│", border_color),
    ]);

    let added_display = cur_acc.added_at.format("%Y-%m-%d").to_string();
    lines.push(vec![
        span("   │", border_color),
        span("  ADDED       ", Color::DarkGrey),
        span(format!("{:<56}", added_display), Color::White),
        span("│", border_color),
    ]);

    let usage_text = match cur_acc.plan.as_deref() {
        Some(p) if !p.is_empty() => format!("{p}  ·  Verified Plan Tier"),
        _ => "N/A  ·  AGY AUTH API data unavailable".to_string(),
    };
    let usage_trunc: String = usage_text.chars().take(56).collect();
    lines.push(vec![
        span("   │", border_color),
        span("  USAGE       ", Color::DarkGrey),
        span(format!("{:<56}", usage_trunc), if cur_acc.plan.is_some() { Color::Cyan } else { Color::DarkGrey }),
        span("│", border_color),
    ]);
    lines.push(vec![span("   │                                                                      │", border_color)]);
    lines.push(vec![span("   ╰──────────────────────────────────────────────────────────────────────╯", border_color)]);
    lines.push(vec![span("", Color::Reset)]);

    // 6. COMMAND CENTER Card
    lines.push(vec![span("   ╭─ COMMAND CENTER ─────────────────────────────────────────────────────╮", border_color)]);
    lines.push(vec![span("   │                                                                      │", border_color)]);
    let switch_bracket = format!("[1-{}]", total_count);
    let cmd_line1 = format!("  {:<6} SWITCH    [A] ADD         [D] DELETE                          ", switch_bracket);
    let cmd_line1_trunc: String = format!("{:<70}", cmd_line1).chars().take(70).collect();
    lines.push(vec![
        span("   │", border_color),
        span(cmd_line1_trunc, Color::White),
        span("│", border_color),
    ]);
    lines.push(vec![
        span("   │", border_color),
        span("  [I]   IMPORT    [R] REFRESH     [Q] QUIT                            ", Color::White),
        span("│", border_color),
    ]);
    lines.push(vec![span("   │                                                                      │", border_color)]);
    lines.push(vec![span("   ╰──────────────────────────────────────────────────────────────────────╯", border_color)]);
    lines.push(vec![span("", Color::Reset)]);

    // 7. TERMINAL Card
    lines.push(vec![span("   ╭─ TERMINAL ───────────────────────────────────────────────────────────╮", border_color)]);
    lines.push(vec![span("   │                                                                      │", border_color)]);

    let prompt_display = match state {
        InputState::Normal => {
            if input_buf.is_empty() {
                "_".to_string()
            } else {
                format!("{input_buf}_")
            }
        }
        InputState::PromptImport => format!("{input_buf}_"),
        InputState::ConfirmDelete(_, _) => "_".to_string(),
        InputState::PromptAdd => "_".to_string(),
    };
    let prompt_trunc: String = prompt_display.chars().take(66).collect();
    lines.push(vec![
        span("   │", border_color),
        span("  ❯ ", Color::Green),
        span(format!("{:<66}", prompt_trunc), Color::White),
        span("│", border_color),
    ]);
    lines.push(vec![span("   │                                                                      │", border_color)]);

    let status_trunc: String = status_msg.chars().take(68).collect();
    lines.push(vec![
        span("   │", border_color),
        span(format!("  {:<68}", status_trunc), Color::Yellow),
        span("│", border_color),
    ]);
    lines.push(vec![span("   ╰──────────────────────────────────────────────────────────────────────╯", border_color)]);
    lines.push(vec![span("", Color::Reset)]);

    // 8. Bottom Footer
    lines.push(vec![
        span("   AGY AUTH / RUST", Color::White),
        span("                                            ", Color::Reset),
        span("● ", Color::Green),
        span("READY", Color::White),
    ]);
    lines.push(vec![span("   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", Color::DarkCyan)]);

    lines
}
