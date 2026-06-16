//! The ratatui front-end: a cartridge list (left), now-playing + timeline + VU
//! (middle), and a transport/help row (bottom). Selecting a cartridge crossfades
//! to it. The audio/mixer threads run underneath; this is just the controller
//! loop with a face on it.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Gauge, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use crate::controller::Controller;
use crate::shared::Shared;

pub fn run(
    ctl: &mut Controller,
    running: Arc<AtomicBool>,
    underruns: Arc<AtomicU64>,
    watch_dir: PathBuf,
) -> Result<()> {
    if !std::io::stdout().is_terminal() {
        bail!("--tui needs an interactive terminal");
    }
    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, ctl, &running, &underruns, &watch_dir);
    ratatui::restore();
    result
}

fn run_loop(
    terminal: &mut DefaultTerminal,
    ctl: &mut Controller,
    running: &Arc<AtomicBool>,
    underruns: &Arc<AtomicU64>,
    watch_dir: &Path,
) -> Result<()> {
    let mut cartridges = scan(watch_dir);
    let mut list = ListState::default();
    if !cartridges.is_empty() {
        list.select(Some(0));
    }
    let mut status = format!("watching {}", watch_dir.display());
    let mut last_scan = Instant::now();

    while running.load(Ordering::Relaxed) {
        terminal.draw(|f| draw(f, &ctl.shared, &cartridges, &mut list, underruns, &status))?;

        ctl.reap();

        if last_scan.elapsed() > Duration::from_millis(1000) {
            cartridges = scan(watch_dir);
            if let Some(sel) = list.selected() {
                if sel >= cartridges.len() {
                    list.select(cartridges.len().checked_sub(1));
                }
            } else if !cartridges.is_empty() {
                list.select(Some(0));
            }
            last_scan = Instant::now();
        }

        if event::poll(Duration::from_millis(80))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        running.store(false, Ordering::Relaxed);
                        break;
                    }
                    KeyCode::Up => move_selection(&mut list, cartridges.len(), -1),
                    KeyCode::Down => move_selection(&mut list, cartridges.len(), 1),
                    KeyCode::Char('r') => {
                        cartridges = scan(watch_dir);
                        status = format!("rescanned — {} cartridge(s)", cartridges.len());
                    }
                    KeyCode::Enter => {
                        if let Some(path) = list.selected().and_then(|i| cartridges.get(i)) {
                            status = match ctl.crossfade_to(path) {
                                Ok(()) => format!("▶ {}", file_label(path)),
                                Err(e) => format!("! {e}"),
                            };
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn move_selection(list: &mut ListState, len: usize, delta: isize) {
    if len == 0 {
        return;
    }
    let cur = list.selected().unwrap_or(0) as isize;
    let next = (cur + delta).rem_euclid(len as isize) as usize;
    list.select(Some(next));
}

fn draw(
    f: &mut Frame,
    shared: &Shared,
    cartridges: &[PathBuf],
    list: &mut ListState,
    underruns: &Arc<AtomicU64>,
    status: &str,
) {
    let [body, help] = Layout::vertical([Constraint::Min(8), Constraint::Length(1)]).areas(f.area());
    let [left, right] = Layout::horizontal([Constraint::Percentage(38), Constraint::Min(20)]).areas(body);
    let [now_area, timeline_area, vu_area, tags_area] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(1),
    ])
    .areas(right);

    // Cartridge list.
    let items: Vec<ListItem> = if cartridges.is_empty() {
        vec![ListItem::new("(drop a .wasm into the watch dir)").style(Style::new().dim())]
    } else {
        cartridges.iter().map(|p| ListItem::new(file_label(p))).collect()
    };
    let listw = List::new(items)
        .block(Block::bordered().title(" Cartridges "))
        .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(listw, left, list);

    // Now playing.
    let now = shared.now();
    let title = if now.title.is_empty() { "—".to_string() } else { now.title.clone() };
    let now_lines = vec![
        Line::from(title.bold().cyan()),
        Line::from(if now.artist.is_empty() { String::new() } else { format!("by {}", now.artist) }),
        Line::from(if now.sample_rate > 0 { format!("{} Hz", now.sample_rate) } else { String::new() }),
    ];
    f.render_widget(
        Paragraph::new(now_lines).block(Block::bordered().title(" Now Playing ")),
        now_area,
    );

    // Timeline.
    let sr = if now.sample_rate > 0 { now.sample_rate as f64 } else { 48_000.0 };
    let elapsed = shared.frames_played().saturating_sub(now.origin_frame);
    let (ratio, label) = if now.duration_frames > 0 {
        let pos = elapsed % now.duration_frames;
        (
            pos as f64 / now.duration_frames as f64,
            format!("{} / {}", mmss(pos as f64 / sr), mmss(now.duration_frames as f64 / sr)),
        )
    } else if now.title.is_empty() {
        (0.0, "—".to_string())
    } else {
        (0.0, format!("{} (generative)", mmss(elapsed as f64 / sr)))
    };
    f.render_widget(
        Gauge::default()
            .block(Block::bordered().title(" Timeline "))
            .gauge_style(Style::new().fg(Color::Cyan))
            .ratio(ratio.clamp(0.0, 1.0))
            .label(label),
        timeline_area,
    );

    // VU meter.
    let vu = shared.vu().clamp(0.0, 1.0);
    let vu_db = if vu > 0.0001 {
        format!("{:.1} dB", 20.0 * vu.log10())
    } else {
        "-inf dB".to_string()
    };
    let vu_color = if vu > 0.95 {
        Color::Red
    } else if vu > 0.7 {
        Color::Yellow
    } else {
        Color::Green
    };
    f.render_widget(
        Gauge::default()
            .block(Block::bordered().title(" Output "))
            .gauge_style(Style::new().fg(vu_color))
            .ratio(vu as f64)
            .label(vu_db),
        vu_area,
    );

    // Tags + underruns.
    let under = underruns.load(Ordering::Relaxed);
    let mut tag_lines = vec![Line::from(if now.tags.is_empty() {
        String::new()
    } else {
        now.tags.join("  ·  ")
    })];
    if under > 0 {
        tag_lines.push(Line::from(format!("⚠ {under} underrun sample(s)").yellow()));
    }
    f.render_widget(
        Paragraph::new(tag_lines).block(Block::bordered().title(" Tags ")),
        tags_area,
    );

    // Help / transport.
    f.render_widget(
        Paragraph::new(Line::from(format!(
            " ↑/↓ select   ⏎ play/crossfade   r rescan   q quit    {status}"
        )))
        .style(Style::new().fg(Color::DarkGray)),
        help,
    );
}

fn mmss(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .trim_end_matches(".wasm")
        .to_string()
}

fn scan(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("wasm"))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::NowPlaying;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn draws_all_panels() {
        let shared = Shared::new();
        shared.set_now(NowPlaying {
            title: "Eutectic Point".to_string(),
            artist: "blackcap".to_string(),
            sample_rate: 48_000,
            duration_frames: 48_000 * 70,
            origin_frame: 0,
            tags: vec!["synth-metal".to_string()],
        });
        shared.add_frames(48_000 * 10); // 10 s in
        shared.set_vu(0.5);

        let cartridges = vec![PathBuf::from("/x/foo.wasm"), PathBuf::from("/x/bar.wasm")];
        let mut list = ListState::default();
        list.select(Some(0));
        let underruns = Arc::new(AtomicU64::new(0));

        let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
        term.draw(|f| draw(f, &shared, &cartridges, &mut list, &underruns, "watching"))
            .unwrap();

        let text: String = term
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();

        for needle in ["Cartridges", "Now Playing", "Timeline", "Output", "Eutectic Point", "foo", "0:10", "synth-metal"] {
            assert!(text.contains(needle), "rendered TUI missing {needle:?}");
        }
    }
}
