//! Screens drawn into a test terminal.
use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn sample() -> App {
    let mut app = App::new();
    app.cfg = Config {
        links: vec![
            Link {
                name: "Dock 3 scanner".into(),
                tcp: "10.1.128.186:51236".into(),
                com: "COM23".into(),
            },
            Link {
                name: "Scale".into(),
                tcp: "10.1.128.190:4001".into(),
                com: "COM24".into(),
            },
        ],
    };
    app.cfg_note = None;
    app.svc = "Running".into();
    app.drv = "Running".into();
    app.installed = true;
    app.admin = false;
    app.traffic = vec![
        traffic::parse_line("2026-11-16 12:01:20.000 -- connected to 10.1.128.186:51236").unwrap(),
        traffic::parse_line("2026-11-16 12:01:29.120 RX 30 39 38 37 36 35 34 33 32 31 0D 0A | x")
            .unwrap(),
        traffic::parse_line("2026-11-16 12:01:40.002 TX 54 0D | T<CR>").unwrap(),
        traffic::parse_line(
            "2026-11-16 12:05:00.000 -- disconnected: device closed the connection",
        )
        .unwrap(),
    ];
    app
}

fn render(app: &App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.draw(f)).unwrap();
    let buf = t.backend().buffer().clone();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn progress(
    app: &App,
    plan: &'static Plan,
    states: Vec<St>,
    result: Option<Result<(), String>>,
) -> Screen {
    let (_tx, rx) = channel();
    let _ = app;
    Screen::Progress(Progress {
        plan,
        states,
        rx,
        result,
        since: Instant::now(),
        after: After::Main,
        success_note: "Your software can now open COM23.".into(),
    })
}

/// Install checklist states: steps before `upto` done, `upto` (or the last step) as `last`.
fn install_states(upto: usize, last: St) -> Vec<St> {
    let n = setup::INSTALL.steps.len();
    let upto = upto.min(n - 1);
    let mut v = vec![St::Pending; n];
    for s in v.iter_mut().take(upto) {
        *s = St::Done(String::new());
    }
    v[1] = St::Done(paths::installed_config().display().to_string());
    if cfg!(windows) {
        v[4] = St::Skip("none found".into());
    }
    v[upto] = last;
    v
}

#[test]
fn draws_every_screen_at_any_size() {
    let mut app = sample();
    let screens = |app: &App| -> Vec<Screen> {
        vec![
            Screen::Welcome,
            Screen::Form(app.form(None)),
            Screen::Form(app.form(Some(0))),
            progress(app, &setup::INSTALL, install_states(6, St::Running), None),
            progress(
                app,
                &setup::INSTALL,
                install_states(10, St::Done("1 connected".into())),
                Some(Ok(())),
            ),
            progress(
                app,
                &setup::INSTALL,
                install_states(6, St::Fail("COM5 is reserved by another device".into())),
                Some(Err("x".into())),
            ),
            Screen::Main,
        ]
    };
    for (w, h) in [(120, 40), (80, 24), (40, 12), (20, 6)] {
        for s in screens(&app) {
            app.screen = s;
            for fmt in [Fmt::Compact, Fmt::HexText, Fmt::Hex, Fmt::Text] {
                app.fmt = fmt;
                render(&app, w, h);
            }
        }
        app.screen = Screen::Main;
        for o in 0..6 {
            app.overlay = match o {
                0 => Overlay::Menu {
                    sel: 2,
                    filter: String::new(),
                },
                1 => Overlay::Admin {
                    open: "menu".into(),
                },
                2 => Overlay::ConfirmDelete(1),
                3 => Overlay::Update {
                    check: sample_update(),
                    sel: 2,
                },
                4 => Overlay::Notice {
                    title: "Update failed".into(),
                    lines: vec![crate::update::update_failed_message()],
                },
                _ => Overlay::ConfirmUninstall,
            };
            render(&app, w, h);
        }
        app.overlay = Overlay::None;
    }
}

#[test]
#[ignore = "prints screens; run with --ignored --nocapture"]
fn show_screens() {
    let mut app = sample();
    app.fmt = Fmt::HexText;
    let shots: Vec<(&str, Screen)> = vec![
        ("WELCOME", Screen::Welcome),
        ("FORM", Screen::Form(app.form(None))),
        (
            "PROGRESS",
            progress(&app, &setup::INSTALL, install_states(6, St::Running), None),
        ),
        (
            "DONE",
            progress(
                &app,
                &setup::INSTALL,
                {
                    let mut s = install_states(10, St::Done("1 connected".into()));
                    s[2] = St::Done(paths::installed_exe().display().to_string());
                    if cfg!(windows) {
                        s[6] = St::Done("COM23".into());
                    }
                    s
                },
                Some(Ok(())),
            ),
        ),
        ("MAIN", Screen::Main),
    ];
    for (name, s) in shots {
        app.screen = s;
        println!("===== {name}\n{}", render(&app, 100, 30));
    }
    app.screen = Screen::Main;
    app.overlay = Overlay::Menu {
        sel: 3,
        filter: String::new(),
    };
    println!("===== MENU\n{}", render(&app, 100, 30));
    app.update = Some(sample_update());
    app.overlay = Overlay::Update {
        check: sample_update(),
        sel: 2,
    };
    println!("===== UPDATE\n{}", render(&app, 100, 30));
}

fn sample_update() -> UpdateCheck {
    UpdateCheck {
        current_version: "1.0.0".into(),
        latest_version: "1.0.1".into(),
        update_available: true,
        release_url: "https://github.com/jackbrumley/comtap/releases/tag/v1.0.1".into(),
    }
}

#[test]
fn enter_opens_edit_form() {
    let mut app = sample();
    app.admin = true;
    app.key(KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE));
    assert!(
        matches!(app.screen, Screen::Form(_)),
        "Enter did not open the form"
    );
}

#[test]
fn shows_waiting_when_no_software_has_the_port() {
    let mut app = sample();
    app.status = Some(StatusFile {
        written: String::new(),
        written_unix: chrono::Local::now().timestamp(),
        pid: 0,
        links: vec![status::LinkStatus {
            name: "Dock 3 scanner".into(),
            com: "COM23".into(),
            tcp: "10.1.128.186:51236".into(),
            state: "connected".into(),
            software_open: Some(false),
            ..Default::default()
        }],
    });
    let s = render(&app, 110, 30);
    assert!(s.contains("waiting for COM23"), "{s}");
    assert!(s.contains("No software has COM23 open"), "{s}");
}

#[test]
fn header_shows_the_version_at_the_top_right() {
    let s = render(&sample(), 110, 30);
    let top = s.lines().next().unwrap();
    let version = concat!("v", env!("CARGO_PKG_VERSION"));
    assert!(top.contains(version), "{top}");
    assert!(top.find(version) > top.find("SERVICE RUNNING"), "{top}");
}

#[test]
fn update_dialog_waits_until_the_main_screen_is_free() {
    let mut app = sample();
    app.overlay = Overlay::Menu {
        sel: 0,
        filter: String::new(),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(Ok(sample_update())).unwrap();
    app.update_rx = Some(rx);
    app.poll_update_check();
    assert!(
        matches!(app.overlay, Overlay::Menu { .. }),
        "the menu was replaced"
    );
    app.overlay = Overlay::None;
    app.show_update_offer();
    assert!(
        matches!(app.overlay, Overlay::Update { .. }),
        "the update dialog did not open once the screen was free"
    );
    app.overlay = Overlay::None;
    app.show_update_offer();
    assert!(
        matches!(app.overlay, Overlay::None),
        "the dialog opened twice"
    );
}
