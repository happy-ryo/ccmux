mod app;
mod claude_monitor;
mod cli;
mod config;
mod filetree;
mod input;
mod ipc;
mod layout_config;
mod pane;
mod preview;
mod ui;
mod version_check;

use std::io;
use std::panic;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

fn main() -> Result<()> {
    // Parse CLI args. clap handles --help / --version and exits cleanly
    // before we enter raw mode below.
    let cli = cli::Cli::parse();
    cli.validate_exec()?;

    // Phase 3: subcommands (`ccmux list`, `ccmux send`, …) are IPC
    // clients and MUST be runnable from inside a ccmux pane — that's
    // the whole point. Dispatch them before the nested-TUI guard kicks
    // in, so the `CCMUX=1` env var set by the parent doesn't block
    // legitimate client invocations.
    if let Some(cmd) = cli.command.as_ref() {
        return run_ipc_client(cmd);
    }

    // No subcommand: we're about to launch another TUI. Refuse if we're
    // already inside a ccmux pane, since nesting vt100 parsers in
    // vt100 parsers produces unreadable output and confuses the mouse.
    if std::env::var("CCMUX").is_ok() {
        eprintln!("ccmux: already running inside a ccmux pane (nested instance not allowed).");
        eprintln!(
            "       Open a new tab with Alt+T (or Ctrl+T) or split with Ctrl+D / Ctrl+E instead."
        );
        std::process::exit(1);
    }

    // If a directory is passed as argument, cd into it first
    if let Some(dir) = &cli.dir {
        if dir.is_dir() {
            std::env::set_current_dir(dir)?;
        } else {
            eprintln!("ccmux: not a directory: {}", dir.display());
            std::process::exit(1);
        }
    }

    // Install panic hook to restore terminal state on crash
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), crossterm::event::DisableMouseCapture);
        let _ = execute!(io::stdout(), crossterm::event::DisableBracketedPaste);
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    execute!(stdout, crossterm::event::EnableMouseCapture)?;
    execute!(stdout, crossterm::event::EnableBracketedPaste)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Get initial terminal size
    let size = terminal.size()?;

    // Phase 3: start the IPC server BEFORE spawning child PTYs so the
    // first `CCMUX_SOCKET` value children see is the real one. Children
    // inherit env from this process (via portable-pty's CommandBuilder),
    // and we publish `CCMUX` as a "you're inside ccmux" flag here too.
    let our_pid = std::process::id();
    // Endpoint resolution can fail on Unix if we can't create the
    // owner-only socket directory (read-only FS, permission-constrained
    // mount, …). IPC is non-essential — fall through without it so the
    // TUI still works as a plain multiplexer, mirroring the IpcServer
    // soft-fail path below.
    let ipc_endpoint = match ipc::endpoint::endpoint_for_pid(our_pid) {
        Ok(ep) => Some(ep),
        Err(e) => {
            eprintln!("ccmux: IPC endpoint unavailable ({e}); external commands disabled.");
            None
        }
    };
    if let Some(ep) = &ipc_endpoint {
        std::env::set_var(ipc::endpoint::ENV_SOCKET, ep.as_str());
    }
    std::env::set_var("CCMUX", "1");

    // Session token derived from the process's start nanoseconds so a
    // client connecting through a stale socket file whose PID got
    // re-used cannot be silently fooled — the server echoes this token
    // on hello, and the client verifies it against `CCMUX_TOKEN`.
    let session_token = format!(
        "{}-{}",
        our_pid,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    // Publish the token before spawning panes so children inherit it.
    std::env::set_var(ipc::endpoint::ENV_TOKEN, &session_token);

    // Load user config + apply CLI override (CLI > file > default).
    let mut user_config = config::Config::load();
    user_config.apply_cli_overrides(cli.ime, cli.ime_freeze_panes);

    // Create app (spawns the initial pane, which captures the env above).
    let mut app = app::App::new(size.height, size.width)?;
    app.apply_config(&user_config);
    app.set_min_pane_size(cli.min_pane_width, cli.min_pane_height);

    // Keep the server handle alive for the process lifetime; its Drop
    // impl cleans up the Unix socket file on exit.
    let _ipc_server = match ipc_endpoint.clone() {
        Some(endpoint) => match ipc::server::IpcServer::spawn(
            endpoint,
            app.command_tx.clone(),
            session_token.clone(),
            app.event_bus.clone(),
        ) {
            Ok(server) => Some(server),
            Err(e) => {
                // IPC is non-essential for the TUI itself — fail soft so users
                // without the required socket permissions can still use ccmux
                // as a plain multiplexer.
                eprintln!("ccmux: IPC server failed to start ({e}); external commands disabled.");
                None
            }
        },
        None => None,
    };

    // Phase 1 (--exec): queue the requested command on the initial focused
    // pane. The command will be flushed into the PTY by the main event
    // loop once the shell prompt is ready (see `try_flush_startup`).
    if let Some(cmd) = cli.exec.as_deref() {
        let focused_id = app.ws().focused_pane_id;
        if let Some(pane) = app.ws_mut().panes.get_mut(&focused_id) {
            pane.queue_startup_command(cmd);
        }
    }

    // Phase 2 (--layout): expand a multi-pane layout from a TOML file.
    // Each leaf pane's command (if any) is queued via the same Phase 1
    // mechanism so all panes flush once their shells are ready.
    if let Some(layout_name) = cli.layout.as_deref() {
        let cfg = layout_config::LayoutConfig::load(layout_name)?;
        app.apply_layout(&cfg)?;
    }

    // Main event loop
    let result = run_event_loop(&mut terminal, &mut app);

    // Cleanup
    app.shutdown();
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture
    )?;
    execute!(
        terminal.backend_mut(),
        crossterm::event::DisableBracketedPaste
    )?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Handle an IPC subcommand (`ccmux send …`, `ccmux list`, etc.).
/// Resolves the endpoint from the `CCMUX_SOCKET` env var the parent
/// ccmux published to its child PTYs; prints the server's response to
/// stdout and exits with a non-zero code on error so shell scripts can
/// branch on it.
fn run_ipc_client(cmd: &cli::IpcCommand) -> Result<()> {
    // `--count 0` on `events` is a degenerate "drain zero events"
    // request. Short-circuit before any environment lookup so the
    // command is a true no-op: it must succeed even when run outside
    // a ccmux pane (where `CCMUX_SOCKET` would be unset).
    if let cli::IpcCommand::Events { count: Some(0), .. } = cmd {
        return Ok(());
    }

    let endpoint = ipc::endpoint::endpoint_from_env()
        .map_err(|e| anyhow::anyhow!("{e}; run this from inside a ccmux pane"))?;

    // `events` uses the subscription path (long-lived stream), not the
    // single-shot request/response path.
    if let cli::IpcCommand::Events { timeout, count } = cmd {
        return run_events(&endpoint, timeout.map(|d| d.into()), *count);
    }

    let request = cmd.to_request()?;
    let response = ipc::client::send_request(&endpoint, &request)?;
    match response {
        ipc::Response::Ok { data } => {
            // `null` → nothing to print; anything else goes out as
            // pretty JSON so shell scripts can `jq` it. We don't print
            // spurious newlines for empty responses so pipelines stay
            // tight.
            if !data.is_null() {
                let pretty =
                    serde_json::to_string_pretty(&data).unwrap_or_else(|_| data.to_string());
                println!("{pretty}");
            }
            Ok(())
        }
        ipc::Response::Hello { .. } | ipc::Response::Subscribed => {
            // These are handshake replies, never command responses.
            Err(anyhow::anyhow!("unexpected control response to command"))
        }
        ipc::Response::Err { message, code } => {
            if let Some(c) = code {
                Err(anyhow::anyhow!("[{c}] {message}"))
            } else {
                Err(anyhow::anyhow!("{message}"))
            }
        }
    }
}

/// Run `ccmux events` with optional stop budgets. Bounds the drain so
/// shell callers can poll inside a `/loop` cycle without hanging.
///
/// Architecture: a worker thread holds the subscription and forwards
/// events into a channel; the main thread selects on that channel with
/// a deadline, printing each event and decrementing the count budget
/// as we go. When main returns, the `Receiver` is dropped and the
/// worker's next `tx.send` fails, making its `on_event` callback
/// return `false` so the subscription exits cleanly. The worker may
/// still be blocked in `read_line` at that point; we detach it and
/// let the OS reap on process exit (CLI is short-lived).
fn run_events(
    endpoint: &ipc::endpoint::EndpointName,
    timeout: Option<std::time::Duration>,
    count: Option<usize>,
) -> Result<()> {
    use std::io::Write;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    // `--count 0` is a degenerate "drain zero events" request; honor it
    // by returning immediately so we never open a connection or spawn
    // a reader for it.
    if let Some(0) = count {
        return Ok(());
    }

    let (tx, rx) = mpsc::channel::<ipc::Event>();
    let endpoint_clone = endpoint.clone();
    std::thread::Builder::new()
        .name("ccmux-events-reader".into())
        .spawn(move || {
            let _ = ipc::client::subscribe_events(&endpoint_clone, |event| tx.send(event).is_ok());
        })
        .map_err(|e| anyhow::anyhow!("spawn events reader: {e}"))?;

    let deadline = timeout.map(|d| Instant::now() + d);
    let mut remaining = count;
    loop {
        let wait = match deadline {
            Some(d) => match d.checked_duration_since(Instant::now()) {
                Some(remaining_time) => remaining_time,
                None => return Ok(()),
            },
            None => Duration::from_secs(60 * 60 * 24 * 365),
        };
        match rx.recv_timeout(wait) {
            Ok(event) => {
                if let Ok(s) = serde_json::to_string(&event) {
                    println!("{s}");
                    let _ = std::io::stdout().flush();
                }
                if let Some(ref mut n) = remaining {
                    *n = n.saturating_sub(1);
                    if *n == 0 {
                        return Ok(());
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut app::App,
) -> Result<()> {
    let mut paste_buffer: Vec<u8> = Vec::new();

    loop {
        // Drain any PTY output events
        app.drain_pty_events();

        // Phase 3: dispatch any commands delivered from the IPC server
        // thread. No-op when the channel is empty, so it's cheap to call
        // every frame.
        app.drain_app_commands();

        // Phase 1 (--exec): flush queued startup commands once the shell
        // prompt is observed. This is a no-op for panes without a queued
        // command, so it's safe to run every frame.
        for ws in &mut app.workspaces {
            for pane in ws.panes.values_mut() {
                let _ = pane.try_flush_startup();
            }
        }

        // After paste, wait a few frames for PTY echo to settle
        if app.paste_cooldown > 0 {
            app.paste_cooldown -= 1;
            if app.paste_cooldown == 0 {
                app.dirty = true;
            }
        }

        // After a layout change (split/close/sidebar/terminal resize),
        // wait a few frames so child PTYs can respond to SIGWINCH with
        // a fresh redraw. Prevents the "old buffer at new size" flash.
        if app.resize_cooldown > 0 {
            app.resize_cooldown -= 1;
            if app.resize_cooldown == 0 {
                app.dirty = true;
            }
        }

        // Always-mode IME: pre-render hook that keeps the overlay
        // open whenever focus rests on a non-scrolled Claude pane,
        // so the host terminal's IME has an anchor from the start.
        // Idempotent and cheap.
        app.maybe_auto_open_always_overlay();

        // Only render when something changed (and no cooldown is active)
        if app.dirty && app.paste_cooldown == 0 && app.resize_cooldown == 0 {
            app.dirty = false;
            // While the IME overlay is open, Claude's thinking spinner keeps
            // dirtying cells inside the pane. ratatui-crossterm paints those
            // diffs by emitting MoveTo+Print without hiding the cursor, and
            // Windows Terminal/conpty leaks those cursor moves to the host
            // caret — producing rapid flicker between the pane and the
            // overlay row. Force-hide the cursor for the entire draw
            // transaction; ratatui will re-show and place it at the overlay
            // position via frame.set_cursor_position at flush time.
            // Scoped to Windows because conpty is the observed culprit; on
            // macOS / Linux terminals this wasn't seen and gating avoids
            // any unintended side effect.
            #[cfg(windows)]
            {
                if app.overlay.is_some() {
                    let _ = execute!(io::stdout(), crossterm::cursor::Hide);
                }
            }
            terminal.draw(|frame| {
                ui::render(app, frame);
            })?;
        }

        if app.should_quit {
            break;
        }

        // Poll for crossterm events with a short timeout (~30fps)
        if event::poll(Duration::from_millis(33))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let consumed = app.handle_key_event(key)?;
                    if !consumed {
                        // Collect rapid key events as potential paste
                        if let Some(bytes) = crate::app::key_event_to_bytes_pub(&key) {
                            paste_buffer.extend_from_slice(&bytes);
                            // Drain all immediately available key events (paste burst)
                            while event::poll(Duration::from_millis(1))? {
                                if let Event::Key(k) = event::read()? {
                                    if k.kind == KeyEventKind::Press {
                                        if app.handle_key_event(k)? {
                                            // Shortcut consumed — flush buffer first
                                            if !paste_buffer.is_empty() {
                                                flush_paste_buffer(app, &mut paste_buffer)?;
                                            }
                                            break;
                                        }
                                        if let Some(b) = crate::app::key_event_to_bytes_pub(&k) {
                                            paste_buffer.extend_from_slice(&b);
                                        }
                                    }
                                } else {
                                    break;
                                }
                            }
                            flush_paste_buffer(app, &mut paste_buffer)?;
                        }
                    }
                    app.dirty = true;
                }
                Event::Key(_) => {}
                Event::Paste(text) => {
                    app.forward_paste_to_pty(&text)?;
                    app.paste_cooldown = 5;
                    app.dirty = true;
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse_event(mouse);
                    app.dirty = true;
                }
                Event::Resize(cols, rows) => {
                    // Propagate the new terminal size to App so every
                    // pane's PTY gets a prompt SIGWINCH, and hold the
                    // paint for a few frames while the children redraw.
                    app.on_terminal_resize(cols, rows);
                }
                _ => {}
            }
        }
    }

    Ok(())
}

/// Flush accumulated key buffer to PTY. If multiple characters were collected
/// (indicating a paste), wrap in bracketed paste sequences only when the PTY
/// application has enabled the mode. Unconditional wrapping causes shells that
/// haven't opted in to display the escape sequences as literal text (issue #2).
fn flush_paste_buffer(app: &mut app::App, buffer: &mut Vec<u8>) -> Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }

    let focused_id = app.ws().focused_pane_id;
    if let Some(pane) = app.ws_mut().panes.get_mut(&focused_id) {
        pane.scroll_reset();
        if buffer.len() > 6 {
            if pane.is_bracketed_paste_enabled() {
                let mut data = Vec::with_capacity(buffer.len() + 12);
                data.extend_from_slice(b"\x1b[200~");
                data.extend_from_slice(buffer);
                data.extend_from_slice(b"\x1b[201~");
                pane.write_input(&data)?;
            } else {
                pane.write_input(buffer)?;
            }
            app.paste_cooldown = 5;
        } else {
            // Normal typing — send directly
            pane.write_input(buffer)?;
        }
    }
    buffer.clear();
    Ok(())
}
