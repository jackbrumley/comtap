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
                enabled: true,
            },
            Link {
                name: "Scale".into(),
                tcp: "10.1.128.190:4001".into(),
                com: "COM24".into(),
                enabled: true,
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
            Screen::Device { idx: 0, sel: 2 },
            Screen::Monitor { idx: 0 },
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
        for screen in [Screen::Main, Screen::Device { idx: 1, sel: 0 }] {
            app.screen = screen;
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
}

#[test]
#[ignore = "prints screens; run with --ignored --nocapture"]
fn show_screens() {
    let mut app = sample();
    app.fmt = Fmt::HexText;
    app.row = 1;
    let shots: Vec<(&str, Screen)> = vec![
        ("WELCOME", Screen::Welcome),
        ("LIST", Screen::Main),
        ("DEVICE", Screen::Device { idx: 0, sel: 0 }),
        ("WATCH DATA", Screen::Monitor { idx: 0 }),
        ("EDIT", Screen::Form(app.form(Some(0)))),
        (
            "PROGRESS",
            progress(&app, &setup::INSTALL, install_states(6, St::Running), None),
        ),
    ];
    for (name, s) in shots {
        app.screen = s;
        println!("===== {name}\n{}", render(&app, 100, 30));
    }
    app.screen = Screen::Main;
    app.overlay = Overlay::Menu {
        sel: 0,
        filter: String::new(),
    };
    println!("===== MENU\n{}", render(&app, 100, 30));
}

fn sample_update() -> UpdateCheck {
    UpdateCheck {
        current_version: "1.0.0".into(),
        latest_version: "1.0.1".into(),
        update_available: true,
        release_url: "https://github.com/jackbrumley/comtap/releases/tag/v1.0.1".into(),
    }
}

fn press(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, event::KeyModifiers::NONE));
}

fn connected_but_unopened() -> StatusFile {
    StatusFile {
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
    }
}

#[test]
fn starts_on_the_device_list_without_traffic() {
    let s = render(&sample(), 110, 30);
    assert!(s.contains("+ Add a device"), "{s}");
    assert!(s.contains("Dock 3 scanner"), "{s}");
    assert!(!s.contains("Watch data"), "{s}");
    assert!(
        !s.contains("connected to 10.1.128.186"),
        "traffic on the list: {s}"
    );
    assert!(s.contains("↑↓ choose  Enter open  F1 menu"), "{s}");
}

#[test]
fn no_devices_explains_what_a_device_is() {
    let mut app = sample();
    app.cfg.links.clear();
    let s = render(&app, 110, 30);
    assert!(s.contains("+ Add a device"), "{s}");
    assert!(s.contains("No devices yet"), "{s}");
}

#[test]
fn enter_steps_through_list_device_page_and_watch_data() {
    let mut app = sample();
    app.admin = true;
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.screen, Screen::Device { idx: 0, sel: 0 }));
    let page = render(&app, 110, 30);
    for item in ["Watch data", "Edit settings", "Delete this device", "Back"] {
        assert!(page.contains(item), "{page}");
    }
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.screen, Screen::Monitor { idx: 0 }));
    let watch = render(&app, 110, 30);
    assert!(watch.contains("Watch data · Dock 3 scanner"), "{watch}");
    assert!(watch.contains("connected to 10.1.128.186"), "{watch}");
    press(&mut app, KeyCode::Right);
    assert_eq!(app.fmt, Fmt::Hex);
    press(&mut app, KeyCode::Esc);
    assert!(matches!(app.screen, Screen::Device { idx: 0, .. }));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert!(
        matches!(app.screen, Screen::Form(_)),
        "Edit settings did not open the form"
    );
    press(&mut app, KeyCode::Esc);
    assert!(
        matches!(app.screen, Screen::Device { idx: 0, .. }),
        "Esc left the device"
    );
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.overlay, Overlay::ConfirmDelete(0)));
    press(&mut app, KeyCode::Esc);
    assert!(matches!(app.overlay, Overlay::None));
    press(&mut app, KeyCode::Esc);
    assert!(matches!(app.screen, Screen::Main));
}

#[test]
fn letters_do_nothing_on_the_list() {
    let mut app = sample();
    app.admin = true;
    for c in ['q', 'a', 'd', 'u', 'l', ' ', 'e', 's'] {
        press(&mut app, KeyCode::Char(c));
        assert!(!app.quit, "{c} quit");
        assert!(matches!(app.screen, Screen::Main), "{c} changed the screen");
        assert!(matches!(app.overlay, Overlay::None), "{c} opened something");
    }
    press(&mut app, KeyCode::Esc);
    assert!(!app.quit, "Esc quit from the list");
    app.key(KeyEvent::new(
        KeyCode::Char('c'),
        event::KeyModifiers::CONTROL,
    ));
    assert!(app.quit, "Ctrl+C did not quit");
}

#[test]
fn view_only_add_and_edit_offer_admin() {
    let mut app = sample();
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.overlay, Overlay::Admin { ref open } if open == "add"));
    app.overlay = Overlay::None;
    app.screen = Screen::Device { idx: 1, sel: 1 };
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.overlay, Overlay::Admin { ref open } if open == "edit:1"));
}

#[test]
fn menu_has_service_quit_updates_and_logs_but_no_device_actions() {
    let app = sample();
    let labels: Vec<String> = app.menu_items().into_iter().map(|i| i.1).collect();
    assert_eq!(
        labels.first().map(String::as_str),
        Some(words::REOPEN_AS_ADMIN)
    );
    assert_eq!(
        labels.last().map(String::as_str),
        Some("Open the log folder")
    );
    assert!(labels.iter().any(|l| l == "Quit"), "{labels:?}");
    assert!(
        labels.iter().any(|l| l.starts_with("Check for updates")),
        "{labels:?}"
    );
    assert!(!labels.iter().any(|l| l.contains("device")), "{labels:?}");
}

#[test]
fn waiting_for_software_in_plain_words() {
    let mut app = sample();
    app.status = Some(connected_but_unopened());
    let list = render(&app, 110, 30);
    assert!(list.contains("Waiting for your software"), "{list}");
    app.screen = Screen::Device { idx: 0, sel: 0 };
    let page = render(&app, 110, 30);
    assert!(page.contains("no software has COM23 open"), "{page}");
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

#[test]
fn a_device_that_is_off_says_so_and_how_to_turn_it_on() {
    let mut app = sample();
    app.cfg.links[0].enabled = false;
    let list = render(&app, 120, 30);
    assert!(list.contains("○ Off"), "{list}");
    app.screen = Screen::Device { idx: 0, sel: 0 };
    let page = render(&app, 120, 30);
    assert!(page.contains("choose Edit settings"), "{page}");
}

#[test]
fn status_field_toggles_with_left_right() {
    let mut app = sample();
    app.admin = true;
    let mut f = app.form(Some(0));
    assert!(f.enabled);
    f.focus = form::F_ON;
    app.screen = Screen::Form(f);
    press(&mut app, KeyCode::Right);
    let Screen::Form(f) = &app.screen else {
        panic!("left the form")
    };
    assert!(!f.enabled);
    let s = render(&app, 110, 30);
    assert!(s.contains("Status") && s.contains(" Off "), "{s}");
}
