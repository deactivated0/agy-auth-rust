use anyhow::{bail,Result};
use crossterm::{cursor,event::{self,Event,KeyCode,KeyEventKind,KeyModifiers},execute,style::{Color,Print,ResetColor,SetForegroundColor},terminal::{self,ClearType,EnterAlternateScreen,LeaveAlternateScreen}};
use std::{collections::HashSet,io::{self,Write},time::Duration};
use crate::model::Account;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Mode {List,Switch,Remove}
struct TerminalGuard;
impl TerminalGuard {
    fn enter()->Result<Self>{terminal::enable_raw_mode()?;execute!(io::stdout(),EnterAlternateScreen,cursor::Hide)?;Ok(Self)}
}
impl Drop for TerminalGuard {fn drop(&mut self){let _=execute!(io::stdout(),ResetColor,cursor::Show,LeaveAlternateScreen);let _=terminal::disable_raw_mode();}}

pub fn interact(accounts:&[Account],active:&str,mode:Mode)->Result<Vec<usize>>{
    if accounts.is_empty(){bail!("no stored accounts");}
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {bail!("interactive terminal required; pass an account query");}
    let _guard=TerminalGuard::enter()?;
    let mut selected=accounts.iter().position(|a|a.id==active).unwrap_or(0);
    let mut marked=HashSet::<usize>::new();
    let mut offset=0usize;
    loop {
        let (_,height)=terminal::size()?;
        let visible=(height as usize).saturating_sub(6).max(1);
        if selected<offset{offset=selected;}
        if selected>=offset+visible{offset=selected+1-visible;}
        paint(accounts,active,mode,selected,&marked,offset,visible)?;
        if !event::poll(Duration::from_millis(200))?{continue;}
        let ev=event::read()?;
        let Event::Key(key)=ev else{continue};
        if key.kind==KeyEventKind::Release{continue;}
        match key.code {
            KeyCode::Char('q')|KeyCode::Esc=>return Ok(vec![]),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL)=>return Ok(vec![]),
            KeyCode::Up|KeyCode::Char('k')=>{selected=selected.saturating_sub(1);}
            KeyCode::Down|KeyCode::Char('j')=>{selected=(selected+1).min(accounts.len()-1);}
            KeyCode::Home=>selected=0,
            KeyCode::End=>selected=accounts.len()-1,
            KeyCode::PageUp=>selected=selected.saturating_sub(visible),
            KeyCode::PageDown=>selected=(selected+visible).min(accounts.len()-1),
            KeyCode::Char(' ') if mode==Mode::Remove=>{if !marked.insert(selected){marked.remove(&selected);}}
            KeyCode::Enter=>return Ok(match mode{Mode::List=>vec![],Mode::Switch=>vec![selected],Mode::Remove=>{if marked.is_empty(){vec![selected]}else{let mut v:Vec<_>=marked.into_iter().collect();v.sort_unstable();v}}}),
            KeyCode::Char(x) if x.is_ascii_digit()=>{
                if let Some(n)=x.to_digit(10) {if n>0&&(n as usize)<=accounts.len(){selected=n as usize-1;}}
            }
            _=>(),
        }
    }
}
fn paint(accounts:&[Account],active:&str,mode:Mode,selected:usize,marked:&HashSet<usize>,offset:usize,visible:usize)->Result<()> {
    let mut out=io::stdout();
    let (width,_)=terminal::size()?;
    let w=(width as usize).max(12);
    execute!(out,cursor::MoveTo(0,0),terminal::Clear(ClearType::All),SetForegroundColor(Color::Cyan),Print(" AgyBuff Auth "),ResetColor)?;
    let title=match mode{Mode::List=>"Accounts",Mode::Switch=>"Select account",Mode::Remove=>"Select accounts to remove"};
    execute!(out,Print(format!("| {title} | {} stored\r\n\r\n",accounts.len())))?;
    for (i,a) in accounts.iter().enumerate().skip(offset).take(visible) {
        let indicator=if i==selected{'>'}else{' '};
        let active_sign=if a.id==active{'*'}else{' '};
        let selected_sign=if marked.contains(&i){'x'}else{' '};
        let prefix=if mode==Mode::Remove{format!("{indicator} [{selected_sign}] {:02} {active_sign} ",i+1)}else{format!("{indicator} {:02} {active_sign} ",i+1)};
        let label=a.display();
        let text=format!("{prefix}{label}");
        let value: String=text.chars().take(w.saturating_sub(1)).collect();
        if i==selected{execute!(out,SetForegroundColor(Color::Green))?;}
        execute!(out,Print(value),ResetColor,Print("\r\n"))?;
    }
    execute!(out,Print("\r\n"))?;
    let help=match mode{Mode::List=>"↑↓ Move  |  q/Esc Close",Mode::Switch=>"↑↓ Move  |  Enter Switch  |  q/Esc Cancel",Mode::Remove=>"↑↓ Move  |  Space Mark  |  Enter Remove  |  q/Esc Cancel"};
    execute!(out,SetForegroundColor(Color::DarkGrey),Print(help),ResetColor)?;
    out.flush()?;Ok(())
}
