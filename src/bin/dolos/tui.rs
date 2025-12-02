use tokio_util::sync::CancellationToken;

pub fn spawn_tui(exit: CancellationToken) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // run blocking terminal UI in a dedicated thread to avoid blocking the async runtime
        let exit2 = exit.clone();
        let handle = std::thread::spawn(move || {
            if let Err(e) = run_blocking(exit2) {
                eprintln!("TUI error: {}", e);
            }
        });

        // wait for either exit cancelled or the thread finished
        tokio::select! {
            _ = exit.cancelled() => {
                // ensure thread will stop by cancelling token (thread checks it)
            }
            _ = tokio::task::spawn_blocking(move || {
                let _ = handle.join();
            }) => {}
        }
    })
}

fn run_blocking(exit: CancellationToken) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::{self, Event, KeyCode};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    use ratatui::widgets::{Block, Borders, Paragraph};
    use ratatui::{backend::CrosstermBackend, Terminal};

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    let backend = CrosstermBackend::new(&mut stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| {
            let area = f.area();
            let text = "Dolos daemon running.\nPress 'q' or ESC to quit.";
            let p =
                Paragraph::new(text).block(Block::default().title("Dolos").borders(Borders::ALL));
            f.render_widget(p, area);
        })?;

        // poll for events with a small timeout so we can also react to cancellation
        if event::poll(std::time::Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        exit.cancel();
                        break;
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        if exit.is_cancelled() {
            break;
        }
    }

    // restore terminal
    disable_raw_mode()?;
    terminal.show_cursor()?;

    Ok(())
}
